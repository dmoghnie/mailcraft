//! MailCraft's rules run by an IMAP server (issue #243): each written as Sieve (RFC 5228) in
//! MailCraft's own script, beside its automatic replies, so mail is sorted as it arrives even
//! while MailCraft is closed or another device reads it first.
//!
//! A rule says what MailCraft's does, as Sieve can say it: its tests on the message's headers
//! (case-insensitive, as MailCraft's are) and size, combined as the rule combines them (all, any,
//! not), its flags and categories with `imap4flags` (RFC 5232), a folder with `fileinto`. What
//! Sieve can't say (a rule for mail the user sends; the start of a message's text, its
//! importance, a question for MailCraft's AI, a sender or recipient that is exactly a name;
//! attachments on a server without the `mime` extension, RFC 5703) is refused, and the rule
//! stays MailCraft's.
//! The rules are kept as MailCraft wrote them in a line of the script ([`KeptRules`]), never read
//! back out of Sieve.

use serde::{Deserialize, Serialize};

use mailcraft_model::FolderRole;
use mailcraft_model::organize::{CONDITION_DEPTH, Condition, Rule, RuleAction, TextOp};

use super::sieve_str;

/// MailCraft's rules on the server as it keeps them in its script: the rules, in the order they
/// run, and the Sieve they were written as, with the extensions it needs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptRules {
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub require: Vec<String>,
    #[serde(default)]
    pub sieve: String,
}

/// What writing a rule needs to know of the account: its folders by role (for Archive, Delete
/// and the like), its categories' keywords by name, and what its server's Sieve can do.
#[derive(Clone, Debug, Default)]
pub struct Context {
    pub roles: Vec<(FolderRole, String)>,
    pub keywords: Vec<(String, String)>,
    pub extensions: Vec<String>,
}

impl Context {
    fn has(&self, ext: &str) -> bool {
        self.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext))
    }

    fn folder(&self, role: FolderRole) -> Result<&str, String> {
        self.roles.iter().find(|(r, _)| *r == role).map(|(_, p)| p.as_str()).ok_or_else(|| format!("this account has no {role:?} folder"))
    }

    /// A category's keyword: the one it was made with, else the one a new category of that name
    /// gets.
    fn keyword(&self, name: &str) -> String {
        self.keywords
            .iter()
            .find(|(n, _)| n.trim().eq_ignore_ascii_case(name.trim()))
            .map_or_else(|| mailcraft_model::keyword_for(name), |(_, k)| k.clone())
    }
}

/// `rules` written as Sieve, those turned on in their order: the script's part and the
/// extensions it needs; or why one can't run on the server.
pub fn written(rules: &[Rule], cx: &Context) -> Result<KeptRules, String> {
    let mut require: Vec<&str> = Vec::new();
    let mut sieve = String::new();
    for rule in rules {
        // Checked even when off: it may be turned on without being written again.
        let (test, actions, needs) = rule_parts(rule, cx).map_err(|e| format!("{}: {e}", rule.name))?;
        for n in needs {
            if !cx.has(n) {
                return Err(format!("{}: this server's Sieve can't {}", rule.name, what_needs(n)));
            }
            if !require.contains(&n) {
                require.push(n);
            }
        }
        if rule.enabled {
            sieve.push_str(&format!("# {}\r\nif {test} {{\r\n{actions}}}\r\n", one_line(&rule.name)));
        }
    }
    Ok(KeptRules { rules: rules.to_vec(), require: require.into_iter().map(str::to_string).collect(), sieve })
}

/// What an extension lets a rule do, for when the server lacks it.
fn what_needs(ext: &str) -> &'static str {
    match ext {
        "fileinto" => "file mail into a folder (fileinto)",
        "imap4flags" => "mark, flag or categorize mail (imap4flags)",
        "mime" => "tell whether mail has attachments (mime)",
        _ => "run this rule",
    }
}

/// A rule's name in a comment line: on one line.
fn one_line(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// A rule's test, its actions (indented lines), and the extensions they need.
fn rule_parts(rule: &Rule, cx: &Context) -> Result<(String, String, Vec<&'static str>), String> {
    if rule.sent {
        return Err("a rule for mail you send runs in MailCraft, not on the server".into());
    }
    if rule.filter.tests().is_empty() || rule.actions.is_empty() {
        return Err("a rule needs a condition and an action".into());
    }
    let mut needs = Vec::new();
    let test = test(&rule.filter, cx, &mut needs, 0)?;
    // Flags first: the folder (or the Inbox) takes the message with them.
    let mut flags = Vec::new();
    let mut folder: Option<String> = None;
    for a in &rule.actions {
        match a {
            RuleAction::MarkRead => flags.push("\\Seen".to_string()),
            RuleAction::Flag => flags.push("\\Flagged".to_string()),
            RuleAction::Pin => flags.push(mailcraft_model::PINNED.to_string()),
            RuleAction::Categorize(name) => flags.push(cx.keyword(name)),
            RuleAction::MoveTo(path) => {
                folder.get_or_insert_with(|| path.clone());
            }
            RuleAction::MoveToRole(role) => {
                let p = cx.folder(*role)?.to_string();
                folder.get_or_insert(p);
            }
            RuleAction::Delete => {
                let p = cx.folder(FolderRole::Trash)?.to_string();
                folder.get_or_insert(p);
            }
        }
    }
    let mut actions = String::new();
    if !flags.is_empty() {
        needs.push("imap4flags");
        let list: Vec<String> = flags.iter().map(|f| sieve_str(f)).collect();
        actions.push_str(&format!("    addflag [{}];\r\n", list.join(", ")));
    }
    if let Some(path) = folder {
        needs.push("fileinto");
        // A folder's name as Sieve takes it: in UTF-8, where IMAP names it in modified UTF-7.
        actions.push_str(&format!("    fileinto {};\r\n", sieve_str(&mailcraft_model::utf7::decode(&path))));
    }
    if rule.stop {
        actions.push_str("    stop;\r\n");
    }
    Ok((test, actions, needs))
}

/// A condition as a Sieve test, its parts combined as it combines them. Text is looked for as
/// MailCraft does, in any case; none at all matches nothing (MailCraft's empty text matches no
/// mail, Sieve's would match all), as do all or any of nothing.
fn test(c: &Condition, cx: &Context, needs: &mut Vec<&'static str>, depth: usize) -> Result<String, String> {
    if depth > CONDITION_DEPTH {
        return Err("its conditions nest too deep".into());
    }
    let mut each = |of: &[Condition], with: &str| -> Result<String, String> {
        let tests: Vec<String> = of.iter().map(|c| test(c, cx, needs, depth + 1)).collect::<Result<_, _>>()?;
        Ok(match tests.as_slice() {
            [] => "false".into(),
            [one] => one.clone(),
            _ => format!("{with}({})", tests.join(", ")),
        })
    };
    Ok(match c {
        Condition::All { of } => each(of, "allof")?,
        Condition::Any { of } => each(of, "anyof")?,
        Condition::Not { of } => format!("not {}", test(of, cx, needs, depth + 1)?),
        Condition::From { value, op } => person(&["from"], *op, value)?,
        Condition::To { value, op } => person(&["to", "cc"], *op, value)?,
        Condition::Subject { value, op } => text(&["subject"], *op, value),
        Condition::AnyText { value, op: TextOp::Contains } => text(&["from", "to", "cc", "subject"], TextOp::Contains, value),
        Condition::AnyText { value, op } => {
            format!("anyof({}, {})", text(&["subject"], *op, value), person(&["from", "to", "cc"], *op, value)?)
        }
        // As MailCraft tells bulk mail (`mime::is_bulk`): a list's headers, a precedence of bulk,
        // list or junk, mail sent automatically, or from a no-reply address.
        Condition::Bulk => "anyof(exists \"list-id\", exists \"list-unsubscribe\", header :is \"precedence\" [\"bulk\", \"list\", \"junk\"], \
             allof(exists \"auto-submitted\", not header :is \"auto-submitted\" \"no\"), \
             address :localpart :is \"from\" [\"noreply\", \"no-reply\", \"donotreply\"])"
            .into(),
        Condition::HasAttachment => {
            if !cx.has("mime") {
                return Err("this server's Sieve can't tell whether mail has attachments (mime)".into());
            }
            needs.push("mime");
            "header :mime :anychild :contains \"content-disposition\" \"attachment\"".into()
        }
        // Kilobytes of 1024 bytes, as MailCraft counts them and Sieve's `K` is.
        Condition::Larger { kb } => format!("size :over {kb}K"),
        Condition::Smaller { kb } => format!("size :under {kb}K"),
        Condition::Preview { .. } => return Err("the start of a message's text is looked at by MailCraft, not the server".into()),
        Condition::Importance { .. } => return Err("a message's importance is looked at by MailCraft, not the server".into()),
        Condition::Asks { .. } => return Err("a rule that asks MailCraft's AI runs in MailCraft, not on the server".into()),
    })
}

/// A header's text compared as `op` says: `:matches` for the start or end, the text's own `*`,
/// `?` and `\\` taken as themselves.
fn text(headers: &[&str], op: TextOp, value: &str) -> String {
    let t = value.trim();
    if t.is_empty() {
        return "false".into();
    }
    let (how, t) = match op {
        TextOp::Contains => (":contains", t.to_string()),
        TextOp::Is => (":is", t.to_string()),
        TextOp::StartsWith => (":matches", format!("{}*", wild_free(t))),
        TextOp::EndsWith => (":matches", format!("*{}", wild_free(t))),
    };
    format!("header {how} {} {}", names(headers), sieve_str(&t))
}

/// A sender or recipient compared as MailCraft does: its name and address together, or its
/// address. "Is" a name can't be told in Sieve, nor where a name's end meets the address.
fn person(headers: &[&str], op: TextOp, value: &str) -> Result<String, String> {
    let t = value.trim();
    if t.is_empty() {
        return Ok("false".into());
    }
    let at = names(headers);
    Ok(match op {
        TextOp::Contains => text(headers, op, t),
        TextOp::Is if t.contains('@') => format!("address :all :is {at} {}", sieve_str(t)),
        TextOp::Is => return Err("a sender or recipient that is exactly a name is looked for by MailCraft, not the server".into()),
        _ if t.contains(char::is_whitespace) => {
            return Err("a sender or recipient's start or end with a space in it is looked for by MailCraft, not the server".into());
        }
        // The name's start (as written, or in quotes), or the address's.
        TextOp::StartsWith => {
            let w = wild_free(t);
            format!(
                "anyof(header :matches {at} [{}, {}], address :all :matches {at} {})",
                sieve_str(&format!("{w}*")),
                sieve_str(&format!("\"{w}*")),
                sieve_str(&format!("{w}*"))
            )
        }
        // The address's end: it's last, after the name.
        TextOp::EndsWith => format!("address :all :matches {at} {}", sieve_str(&format!("*{}", wild_free(t)))),
    })
}

/// Header names as a Sieve string or list.
fn names(headers: &[&str]) -> String {
    let names: Vec<String> = headers.iter().map(|h| sieve_str(h)).collect();
    if names.len() == 1 { names.concat() } else { format!("[{}]", names.join(", ")) }
}

/// Text for `:matches` whose own wildcards (`*`, `?`) and backslashes are taken as themselves.
fn wild_free(t: &str) -> String {
    t.chars().fold(String::new(), |mut out, c| {
        if matches!(c, '*' | '?' | '\\') {
            out.push('\\');
        }
        out.push(c);
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cx() -> Context {
        Context {
            roles: vec![(FolderRole::Trash, "Trash".into()), (FolderRole::Archive, "Archive".into())],
            keywords: vec![("Important".into(), "$label1".into())],
            extensions: ["fileinto", "imap4flags", "include", "vacation"].map(String::from).to_vec(),
        }
    }

    fn rule(name: &str, conditions: Vec<Condition>, actions: Vec<RuleAction>) -> Rule {
        Rule { name: name.into(), enabled: true, account: None, filter: Condition::all(conditions), actions, stop: false, sent: false }
    }

    fn from(t: &str) -> Condition {
        Condition::From { value: t.into(), op: TextOp::Contains }
    }

    fn to(t: &str) -> Condition {
        Condition::To { value: t.into(), op: TextOp::Contains }
    }

    fn subject(t: &str) -> Condition {
        Condition::Subject { value: t.into(), op: TextOp::Contains }
    }

    #[test]
    fn rules_are_written_as_sieve() {
        let invoices = Rule {
            stop: true,
            ..rule(
                "Invoices",
                vec![from("billing@shop.example"), subject("Invoice")],
                vec![RuleAction::MoveTo("Bills/&AMk-t&AOk-".into()), RuleAction::MarkRead, RuleAction::Categorize("Important".into())],
            )
        };
        let lists = Rule {
            filter: Condition::Any { of: vec![Condition::Bulk, to("team@")] },
            ..rule("Lists", vec![], vec![RuleAction::MoveToRole(FolderRole::Archive)])
        };
        let off = Rule { enabled: false, ..rule("Off", vec![subject("x")], vec![RuleAction::Delete]) };
        let k = written(&[invoices.clone(), lists, off], &cx()).unwrap();
        assert_eq!(k.require, ["imap4flags", "fileinto"]);
        let s = &k.sieve;
        assert!(s.contains("if allof(header :contains \"from\" \"billing@shop.example\", header :contains \"subject\" \"Invoice\") {"), "{s}");
        assert!(s.contains("    addflag [\"\\\\Seen\", \"$label1\"];\r\n    fileinto \"Bills/Été\";\r\n    stop;\r\n}"), "{s}");
        assert!(s.contains("if anyof(anyof(exists \"list-id\""), "{s}");
        assert!(s.contains("header :contains [\"to\", \"cc\"] \"team@\") {\r\n    fileinto \"Archive\";"), "{s}");
        assert!(!s.contains("Off"), "a rule turned off isn't written: {s}");
        assert_eq!(k.rules.len(), 3, "but kept");
    }

    #[test]
    fn what_sieve_cant_say_is_refused() {
        let sent = Rule { sent: true, ..rule("Sent", vec![subject("x")], vec![RuleAction::Flag]) };
        assert!(written(&[sent], &cx()).unwrap_err().contains("mail you send"));
        let attached = rule("Files", vec![Condition::HasAttachment], vec![RuleAction::Flag]);
        assert!(written(std::slice::from_ref(&attached), &cx()).unwrap_err().contains("attachments"));
        let mut with_mime = cx();
        with_mime.extensions.push("mime".into());
        let k = written(&[attached], &with_mime).unwrap();
        assert!(k.require.contains(&"mime".to_string()) && k.sieve.contains(":mime :anychild"));
        let mut bare = cx();
        bare.extensions.retain(|e| e != "imap4flags");
        assert!(written(&[rule("Flag", vec![subject("x")], vec![RuleAction::Flag])], &bare).unwrap_err().contains("imap4flags"));
        let mut no_trash = cx();
        no_trash.roles.clear();
        assert!(written(&[rule("Bin", vec![subject("x")], vec![RuleAction::Delete])], &no_trash).is_err());
    }

    /// A rule's conditions combined as it combines them (any within all, not), each text test as
    /// it compares, and sizes in kilobytes of 1024 bytes.
    #[test]
    fn combined_conditions_and_comparisons_are_written_as_sieve() {
        let filter = Condition::All {
            of: vec![
                Condition::Any { of: vec![from("shop.example"), Condition::Larger { kb: 500 }] },
                Condition::Not { of: Box::new(Condition::Subject { value: "Re: *".into(), op: TextOp::StartsWith }) },
                Condition::From { value: "Zoe@Studio.example".into(), op: TextOp::Is },
                Condition::To { value: "@team.example".into(), op: TextOp::EndsWith },
                Condition::Smaller { kb: 20 },
            ],
        };
        let r = Rule { filter, ..rule("Shop", vec![], vec![RuleAction::Flag]) };
        let k = written(&[r], &cx()).unwrap();
        let s = &k.sieve;
        assert!(s.contains("if allof(anyof(header :contains \"from\" \"shop.example\", size :over 500K), "), "{s}");
        assert!(s.contains(r#"not header :matches "subject" "Re: \\**", "#), "the text's own * is itself: {s}");
        assert!(s.contains(r#"address :all :is "from" "Zoe@Studio.example", "#), "{s}");
        assert!(s.contains(r#"address :all :matches ["to", "cc"] "*@team.example", size :under 20K) {"#), "{s}");

        let starts = rule("Starts", vec![Condition::From { value: "Zoe".into(), op: TextOp::StartsWith }], vec![RuleAction::Flag]);
        let k = written(&[starts], &cx()).unwrap();
        assert!(k.sieve.contains(r#"anyof(header :matches "from" ["Zoe*", "\"Zoe*"], address :all :matches "from" "Zoe*")"#), "{}", k.sieve);
        // All or any of nothing matches nothing, as in MailCraft.
        let none = Rule { filter: Condition::all(vec![subject("x"), Condition::Any { of: vec![] }]), ..rule("None", vec![], vec![RuleAction::Flag]) };
        assert!(written(&[none], &cx()).unwrap().sieve.contains("allof(header :contains \"subject\" \"x\", false)"));
    }

    /// What only MailCraft can tell is refused, the rule kept MailCraft's: the start of a
    /// message's text, its importance, a question for its AI, a sender that is exactly a name,
    /// and a rule that tests nothing.
    #[test]
    fn what_only_mailcraft_tells_is_refused() {
        use mailcraft_model::organize::Importance;
        for (c, said) in [
            (Condition::Preview { value: "hello".into(), op: TextOp::Contains }, "start of a message's text"),
            (Condition::Importance { value: Importance::High }, "importance"),
            (Condition::Asks { question: "Is it a contract?".into() }, "asks MailCraft's AI"),
            (Condition::From { value: "Zoe Park".into(), op: TextOp::Is }, "exactly a name"),
            (Condition::To { value: "Zoe Park".into(), op: TextOp::EndsWith }, "with a space"),
        ] {
            let r = rule("Mine", vec![subject("x"), c], vec![RuleAction::Flag]);
            let e = written(&[r], &cx()).unwrap_err();
            assert!(e.starts_with("Mine: ") && e.contains(said), "{e}");
        }
        let nothing = Rule { filter: Condition::all(vec![]), ..rule("Nothing", vec![], vec![RuleAction::Flag]) };
        assert!(written(&[nothing], &cx()).unwrap_err().contains("needs a condition"));
        let mut deep = subject("x");
        for _ in 0..=CONDITION_DEPTH + 1 {
            deep = Condition::Not { of: Box::new(deep) };
        }
        let deep = Rule { filter: deep, ..rule("Deep", vec![], vec![RuleAction::Flag]) };
        assert!(written(&[deep], &cx()).unwrap_err().contains("nest too deep"));
    }

    /// What's typed can't break out of its string or comment: quotes, backslashes and line ends
    /// stay inside; empty text matches nothing.
    #[test]
    fn typed_text_stays_in_its_string() {
        let evil = rule("Name\r\n}\r\ndiscard;\r\n#", vec![subject("\"; discard; \\"), from("  ")], vec![RuleAction::Flag]);
        let k = written(&[evil], &cx()).unwrap();
        assert!(k.sieve.starts_with("# Name  }  discard;  #\r\n"), "{}", k.sieve);
        assert!(k.sieve.contains(r#"header :contains "subject" "\"; discard; \\""#), "{}", k.sieve);
        assert!(k.sieve.contains("false"), "empty text matches nothing");
        assert_eq!(k.sieve.matches("discard;").count(), 2, "both inside strings or the comment: {}", k.sieve);
    }
}

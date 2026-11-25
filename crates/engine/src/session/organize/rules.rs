//! Inbox rules: kept, checked, saved to and read from a file, and run on mail as it arrives (or
//! now, on a folder's mail), their actions made ordinary changes; a message whose rules wait on
//! MailCraft's AI's answer to a rule's question runs on once it comes (`ai/rule_questions.rs`).

use super::*;

/// What running the rules on some mail did: the messages not to announce, how many a rule
/// matched, and how many wait on MailCraft's AI.
#[derive(Default)]
struct RulesRan {
    quiet: Vec<MessageKey>,
    matched: usize,
    waiting: usize,
}

impl Session {
    pub fn set_rules(&mut self, rules: Vec<Rule>) -> CmdResult {
        self.check_rules(&rules)?;
        self.rules = rules;
        if let Some(store) = &self.store
            && let Err(e) = store.save_doc(RULES_DOC, &self.rules)
        {
            return Err(CmdError::Failed(format!("the rules couldn't be saved: {e}")));
        }
        Ok(serde_json::json!({ "rules": self.rules.len() }))
    }

    /// Whether `rules` can take the place of the rules here: at most 500, each named, each
    /// category they file mail under in the list, and the rules here read (not saved over).
    fn check_rules(&self, rules: &[Rule]) -> Result<(), CmdError> {
        if rules.len() > 500 {
            return Err(CmdError::Params("at most 500 rules".into()));
        }
        for r in rules {
            if r.name.trim().is_empty() {
                return Err(CmdError::Params("every rule needs a name".into()));
            }
            // Past the limits (a hostile file or answer): one that looks at nothing, or for an
            // empty text, never matches and is kept, as rules were before they combined.
            if let Some(why) = r.filter.past_limits() {
                return Err(CmdError::Params(format!("rule {}: {why}", r.name)));
            }
            for a in &r.actions {
                if let RuleAction::Categorize(c) = a
                    && self.category(c).is_none()
                {
                    return Err(CmdError::Params(format!("rule {}: no category called {c}", r.name)));
                }
            }
        }
        if self.docs_unread.contains(RULES_DOC) {
            return Err(CmdError::Failed("Your rules couldn't be read, so they aren't saved over".into()));
        }
        Ok(())
    }

    /// The rules as a file to keep or take to another computer (Outlook's Export Rules): each
    /// rule's account named by its address, which another computer knows it by.
    pub fn rules_file(&self) -> Result<Vec<u8>, CmdError> {
        let rules: Vec<serde_json::Value> = self
            .rules
            .iter()
            .map(|r| {
                let mut v = serde_json::to_value(r).unwrap_or_default();
                if let (Some(id), Some(o)) = (&r.account, v.as_object_mut()) {
                    let email = self.account(id).map(|a| a.config.email.clone()).unwrap_or_default();
                    o.insert("account".into(), serde_json::Value::Null);
                    o.insert("accountEmail".into(), serde_json::Value::String(email));
                }
                v
            })
            .collect();
        // The categories the rules file mail under, with their keywords: another computer
        // makes a missing one with the same keyword, so both mark mail the same way.
        let used: Vec<&mailcraft_model::organize::Category> = self
            .prefs
            .categories
            .iter()
            .filter(|c| self.rules.iter().flat_map(|r| &r.actions).any(|a| matches!(a, RuleAction::Categorize(n) if n.eq_ignore_ascii_case(&c.name))))
            .collect();
        let doc = serde_json::json!({ "mailcraftRules": 1, "rules": rules, "categories": used });
        serde_json::to_vec_pretty(&doc).map_err(|e| CmdError::Failed(e.to_string()))
    }

    /// Add the rules in a file made by [`Session::rules_file`] (Outlook's Import Rules) after
    /// those here, or in place of them (`replace`). A rule for an account this computer doesn't
    /// have comes in turned off; a category one files mail under is made when it's missing; a
    /// rule already here (the same in every part) isn't added twice. What came in, and how many
    /// were turned off.
    pub fn import_rules(&mut self, bytes: &[u8], replace: bool) -> CmdResult {
        if bytes.len() > 4 << 20 {
            return Err(CmdError::Failed("that file is too big for a rules file".into()));
        }
        // A file saved again by an editor may start with a byte order mark.
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        let doc: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| CmdError::Failed("not a MailCraft rules file".into()))?;
        let list = match (doc.get("mailcraftRules"), doc.get("rules").and_then(serde_json::Value::as_array)) {
            (Some(_), Some(list)) => list.clone(),
            _ => return Err(CmdError::Failed("not a MailCraft rules file".into())),
        };
        let known: Vec<mailcraft_model::organize::Category> =
            doc.get("categories").cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        let mut rules = if replace { Vec::new() } else { self.rules.clone() };
        let (mut added, mut off) = (0usize, 0usize);
        // The categories the rules read file mail under (those already here among them too).
        let mut wanted: Vec<String> = Vec::new();
        for (i, v) in list.into_iter().take(500).enumerate() {
            let email = v.get("accountEmail").and_then(serde_json::Value::as_str).map(str::to_string);
            let mut rule: Rule =
                serde_json::from_value(v).map_err(|e| CmdError::Failed(format!("not a MailCraft rules file: rule {}: {e}", i + 1)))?;
            rule.account = None;
            if let Some(email) = email.filter(|e| !e.is_empty()) {
                match self.accounts.iter().find(|a| a.config.email.eq_ignore_ascii_case(&email)) {
                    Some(a) => rule.account = Some(a.config.id.clone()),
                    // Kept for that account only: off until it's chosen again.
                    None => {
                        rule.enabled = false;
                        off += 1;
                    }
                }
            }
            wanted.extend(rule.actions.iter().filter_map(|a| if let RuleAction::Categorize(c) = a { Some(c.clone()) } else { None }));
            if !rules.contains(&rule) {
                rules.push(rule);
                added += 1;
            }
        }
        // Every rule read: the categories they file mail under are made, and taken back when
        // the rules are refused all the same (too many, the rules here unread), so a failed
        // import leaves nothing behind. Made ones only go on the end of the list.
        let had = self.prefs.categories.len();
        let made = self.make_categories(&wanted, &known).and_then(|()| self.check_rules(&rules));
        if let Err(e) = made {
            self.prefs.categories.truncate(had);
            return Err(e);
        }
        self.set_rules(rules)?;
        Ok(serde_json::json!({ "added": added, "turnedOff": off, "rules": self.rules.len() }))
    }

    /// Make the categories called `names` that aren't in the list: with the keyword and colour
    /// they have where rules came from (`known`), when that keyword is free here.
    fn make_categories(&mut self, names: &[String], known: &[mailcraft_model::organize::Category]) -> Result<(), CmdError> {
        for c in names {
            if self.category(c).is_some() {
                continue;
            }
            let made = known.iter().find(|k| k.name.eq_ignore_ascii_case(c)).and_then(|k| self.add_category(c, Some(k.color), Some(&k.keyword)).ok());
            if made.is_none() {
                self.add_category(c, None, None)?;
            }
        }
        Ok(())
    }

    /// Run the rules on messages: their actions become ordinary changes. Returns the keys the
    /// rules marked read or sent to Deleted Items or Junk (no need to announce those). A message
    /// in Sent has the rules for mail sent, any other the rest (issue #202).
    pub(crate) fn run_rules(&mut self, msgs: &[Summary]) -> Vec<MessageKey> {
        if self.rules.is_empty() {
            return Vec::new();
        }
        self.apply_rules(None, msgs).quiet
    }

    /// [`Session::run_rules`] with rule `only`, or those kept: the keys not to announce, how
    /// many of `msgs` a rule matched, and how many wait on MailCraft's AI's answer to a rule's
    /// question (`ai/rule_questions.rs`) before their rules can say.
    fn apply_rules<S: std::borrow::Borrow<Summary>>(&mut self, only: Option<Rule>, msgs: &[S]) -> RulesRan {
        let mut ran = RulesRan::default();
        let mut changes = Batch::default();
        let rules = only.clone().map_or_else(|| self.rules.clone(), |r| vec![r]);
        let (sending, arriving): (Vec<Rule>, Vec<Rule>) = rules.into_iter().partition(|r| r.sent);
        for m in msgs {
            let m: &Summary = m.borrow();
            // One waiting on MailCraft's AI for its rules is theirs till the answer comes.
            if self.rule_question_waits(&m.key) {
                ran.waiting += 1;
                continue;
            }
            let rules = if self.in_sent(m) { &sending } else { &arriving };
            let Some(actions) = self.rules_decide(rules, m, only.clone(), Vec::new()) else {
                ran.waiting += 1;
                continue;
            };
            if actions.is_empty() {
                continue;
            }
            ran.matched += 1;
            self.act_on(&mut changes, &mut ran.quiet, m, actions);
        }
        self.apply_changes(changes);
        ran
    }

    /// The rules run again on `m` once MailCraft's AI answered their question about it, with all
    /// it answered (`answers`): rule `only`, or those kept.
    pub(crate) fn rules_answered(&mut self, m: &Summary, only: Option<Rule>, answers: Vec<(String, bool)>) {
        let sent = self.in_sent(m);
        let rules = only.clone().map_or_else(|| self.rules.clone(), |r| vec![r]);
        let rules: Vec<Rule> = rules.into_iter().filter(|r| r.sent == sent).collect();
        let Some(actions) = self.rules_decide(&rules, m, only, answers) else { return };
        let mut changes = Batch::default();
        // Announced when it came: nothing to keep quiet now.
        self.act_on(&mut changes, &mut Vec::new(), m, actions);
        self.apply_changes(changes);
    }

    /// Whether `m` is in its account's Sent folder: the rules for mail sent are its.
    fn in_sent(&self, m: &Summary) -> bool {
        self.folder(&m.key.account, &m.key.folder).is_some_and(|f| f.role == FolderRole::Sent)
    }

    /// What rules do to message `m` (`actions`) added to `changes`, and it to `quiet` when it
    /// isn't to be announced (marked read, or sent to Deleted Items or Junk).
    fn act_on(&mut self, changes: &mut Batch, quiet: &mut Vec<MessageKey>, m: &Summary, actions: Vec<RuleAction>) {
        if actions.is_empty() {
            return;
        }
        let a = m.key.account.clone();
        let keys = vec![m.key.clone()];
        let mut dest: Option<String> = None;
        for act in actions {
            match act {
                RuleAction::MarkRead => {
                    changes.add(&a, Op::SetFlag { keys: keys.clone(), flag: Flag::Seen, on: true });
                    quiet.push(m.key.clone());
                }
                RuleAction::Flag => changes.add(&a, Op::SetFlag { keys: keys.clone(), flag: Flag::Flagged, on: true }),
                RuleAction::Pin => changes.add(&a, Op::SetKeyword { keys: keys.clone(), keyword: PINNED.into(), on: true }),
                RuleAction::Categorize(c) => {
                    if let Some(kw) = self.category(&c).map(|c| c.keyword.clone()) {
                        changes.add(&a, Op::SetKeyword { keys: keys.clone(), keyword: kw, on: true });
                    }
                }
                RuleAction::MoveTo(f) => dest = self.folder(&a, &f).map(|f| f.path.clone()).or(dest),
                RuleAction::MoveToRole(r) => dest = self.account(&a).and_then(|x| x.folder(r)).map(|f| f.path.clone()).or(dest),
                RuleAction::Delete => dest = self.account(&a).and_then(|x| x.folder(FolderRole::Trash)).map(|f| f.path.clone()).or(dest),
            }
        }
        // Moves last (`apply_changes`), so flags and categories are set on the message before
        // it leaves. Mail filed in a folder is still new mail (announced, as Outlook does);
        // mail a rule sends to Deleted Items or Junk isn't.
        if let Some(to) = dest.filter(|to| *to != m.key.folder) {
            let role = self.folder(&a, &to).map(|f| f.role);
            if matches!(role, Some(FolderRole::Trash | FolderRole::Junk)) {
                quiet.push(m.key.clone());
            }
            changes.add(&a, Op::Move { keys, to });
        }
    }

    /// Run the rules (or one) now, on the list on screen or (`inbox`) on the mail already in the
    /// Inbox of the accounts they apply to (and in Sent, for the rules for mail sent), as
    /// Outlook's "run this rule now".
    pub fn run_rules_now(&mut self, only: Option<usize>, inbox: bool) -> CmdResult {
        // The one asked for runs even when it's turned off; the rules kept stay as they are.
        let rules = match only {
            Some(i) => vec![self.rules.get(i).cloned().map(|r| Rule { enabled: true, ..r }).ok_or_else(|| CmdError::Params(format!("no rule {i}")))?],
            None => self.rules.clone(),
        };
        let msgs: Vec<SharedSummary> = if inbox {
            let only_account = only.and(rules.first()).and_then(|r| r.account.clone());
            let roles: Vec<FolderRole> = [(FolderRole::Inbox, false), (FolderRole::Sent, true)]
                .into_iter()
                .filter(|(_, sent)| rules.iter().any(|r| r.sent == *sent))
                .map(|(role, _)| role)
                .collect();
            let inboxes: Vec<(String, String)> = self
                .accounts
                .iter()
                .filter(|a| only_account.as_ref().is_none_or(|o| *o == a.config.id))
                .flat_map(|a| roles.iter().filter_map(|r| a.folder(*r).map(|f| (a.config.id.clone(), f.path.clone()))))
                .collect();
            self.folders_ready(&inboxes)?;
            inboxes
                .iter()
                .flat_map(|(a, f)| self.folder_state(a, f).messages.iter().filter(|m| !m.flags.deleted).cloned().collect::<Vec<_>>())
                .collect()
        } else {
            self.list.clone()
        };
        let ran = self.apply_rules(only.and_then(|_| rules.into_iter().next()), &msgs);
        let n = ran.matched;
        let what = if n == 1 { "1 message".to_string() } else { format!("{n} messages") };
        let waiting = match ran.waiting {
            0 => String::new(),
            1 => "; 1 waits on MailCraft's AI".into(),
            w => format!("; {w} wait on MailCraft's AI"),
        };
        self.notice(format!("Rules ran on the mail there now: {what} matched{waiting}"), false);
        Ok(serde_json::json!({ "matched": n, "waiting": ran.waiting }))
    }
}

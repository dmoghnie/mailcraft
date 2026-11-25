//! Rules in plain words, as text2sql writes a query: the person's words go to the chat model
//! with the rule's own schema (the account's folders in it) and the categories' names, never any
//! mail; the rule it fills in is read straight into the rule MailCraft runs, kept as a proposal,
//! and added only when asked, with a category it names made then.

use mailcraft_model::organize::{Condition, RuleAction, WORDS_DEPTH};

use super::*;

const PROOF: &str = "Your proof for the festival poster is ready";
const WORDS: &str = "proofs and invoices from the print shop go to Projects and get the Print category";

fn ready(name: &str) -> Session {
    let mut s = sample_session(name);
    s.run("ai.allow", &json!({"account": "sample", "reach": "local"})).unwrap();
    local(&mut s, "Local");
    s.take_jobs();
    s
}

/// The rule asked for, answered `text`; what `rules.proposed` says then.
fn written(s: &mut Session, text: &str) -> Value {
    s.run("rules.fromWords", &json!({"words": WORDS})).unwrap();
    let (id, ..) = asked(s).pop().expect("a question");
    answer(s, id, text);
    s.run("rules.proposed", &json!({})).unwrap()["proposal"].clone()
}

/// The model's answer, as the schema has it: the print shop's proofs or invoices, to "projects"
/// (as the model wrote it), with a category there isn't yet.
fn print_shop() -> Value {
    json!({"rule": {
        "name": "Print shop",
        "where": {"if": "all", "of": [
            {"if": "from", "value": "printshop.example", "op": "endsWith"},
            {"if": "any", "of": [{"if": "subject", "value": "proof", "op": "contains"}, {"if": "subject", "value": "invoice", "op": "contains"}]},
        ]},
        "actions": [{"do": "moveTo", "value": "projects"}, {"do": "categorize", "value": "Print"}],
        "stop": true,
        "sent": false,
    }, "whyNot": null})
}

/// The question carries the rule's schema, with the account's folders to move to, and the words
/// and categories; no mail goes. The answer is the rule MailCraft runs, once added: it moves the
/// print shop's proof from the Inbox to Projects.
#[test]
fn a_rule_is_written_to_its_schema_and_runs_once_added() {
    let mut s = ready("ai-rule-words");
    let v = s.run("rules.fromWords", &json!({"words": WORDS})).unwrap();
    assert_eq!(v["proposal"]["writing"], true, "{v}");
    let (id, _, _, chat) = asked(&mut s).pop().expect("a question");
    let schema = chat.answer_schema.clone().expect("the answer kept to the rule's schema");
    let folders = &schema["$defs"]["action"]["anyOf"][0]["properties"]["value"]["enum"];
    assert!(folders.as_array().is_some_and(|f| f.contains(&json!("Projects/Short film"))), "{folders}");
    assert!(schema["$defs"].get(format!("where{WORDS_DEPTH}")).is_some());
    let q = &chat.turns[0].text;
    assert!(q.contains(WORDS) && q.contains("Important, Work") && q.contains("Projects/Short film"), "{q}");
    assert!(chat.system.contains("\"$defs\""), "the schema in the instructions too, for a server that doesn't show it");
    assert!(!q.contains(PROOF) && !chat.system.contains(PROOF), "no mail goes");
    answer(&mut s, id, &print_shop().to_string());
    let p = s.run("rules.proposed", &json!({})).unwrap()["proposal"].clone();
    assert_eq!((p["writing"].as_bool(), p["error"].as_str()), (Some(false), None), "{p}");
    assert_eq!(p["rule"]["account"], "sample", "a move is for the account whose folder it is");
    assert!(s.rules.is_empty(), "nothing added yet");
    assert!(s.rule_proposal().is_some_and(|r| r.rule.is_some() && !r.writing()));

    s.run("rules.addProposed", &json!({})).unwrap();
    let r = s.rules.first().expect("added").clone();
    assert_eq!(r.actions, [RuleAction::MoveTo("Projects".into()), RuleAction::Categorize("Print".into())], "the folder as it's called");
    assert!(matches!(&r.filter, Condition::All { of } if of.len() == 2));
    assert!(s.category("Print").is_some(), "the category it names made");
    assert_eq!(s.run("rules.proposed", &json!({})).unwrap()["proposal"], Value::Null);
    assert!(matches!(s.run("rules.addProposed", &json!({})), Err(CmdError::Failed(_))), "none left to add");
    assert_eq!(s.run("rules.run", &json!({"index": 0, "inbox": true})).unwrap()["matched"], 1);
    assert!(!s.folder_state("sample", "INBOX").messages.iter().any(|m| m.subject == PROOF), "the proof moved");
}

/// A server that wrapped its answer (in a Markdown fence) is read from its braces.
#[test]
fn an_answer_in_a_fence_is_read() {
    let mut s = ready("ai-rule-words-fence");
    let p = written(&mut s, &format!("```json\n{}\n```", print_shop()));
    assert!(p["rule"].is_object() && p["error"].is_null(), "{p}");
}

/// What can't be a rule says why, and nothing's there to add: the model's own why, an answer
/// that isn't the schema's, a folder that isn't there, a rule that does nothing, an empty text,
/// conditions past the limit.
#[test]
fn a_rule_that_cant_be_run_is_not_proposed() {
    let mut s = ready("ai-rule-words-refused");
    let rule = |where_: Value, actions: Value| {
        json!({"rule": {"name": "R", "where": where_, "actions": actions, "stop": false, "sent": false}, "whyNot": null}).to_string()
    };
    let mut deep = json!({"if": "bulk"});
    for _ in 0..7 {
        deep = json!({"if": "not", "of": deep});
    }
    let flag = json!([{"do": "flag"}]);
    for (said, why) in [
        (
            json!({"rule": null, "whyNot": "A rule can't read what's in the attachments."}).to_string(),
            "the model says it can't be a rule: A rule can't read",
        ),
        ("Sure, I'd move them.".to_string(), "the model's rule couldn't be read"),
        (rule(json!({"if": "bulk"}), json!([{"do": "moveTo", "value": "Receipts"}])), "there's no folder called Receipts"),
        (rule(json!({"if": "bulk"}), json!([])), "missing its name or what it does"),
        (rule(json!({"if": "from", "value": " ", "op": "is"}), flag.clone()), "a text it looks for is empty"),
        (rule(deep, flag), "nest too deep"),
    ] {
        let p = written(&mut s, &said);
        assert!(p["rule"].is_null() && p["error"].as_str().is_some_and(|e| e.contains(why)), "{said}: {p}");
    }
    assert!(matches!(s.run("rules.addProposed", &json!({})), Err(CmdError::Failed(_))));
    s.run("rules.dropProposed", &json!({})).unwrap();
    assert!(s.rule_proposal().is_none());
}

/// Nothing goes without words, to an account there isn't, or when the account's mail may not
/// go to a model.
#[test]
fn nothing_goes_without_words_or_leave() {
    let mut s = ready("ai-rule-words-asked");
    assert!(matches!(s.run("rules.fromWords", &json!({"words": " "})), Err(CmdError::Params(_))));
    assert!(matches!(s.run("rules.fromWords", &json!({"words": WORDS, "account": "nobody"})), Err(CmdError::Params(_))));
    let mut off = sample_session("ai-rule-words-off");
    local(&mut off, "Local");
    off.take_jobs();
    assert!(matches!(off.run("rules.fromWords", &json!({"words": WORDS})), Err(CmdError::Disabled(_))), "the account's mail may not go");
    assert!(asked(&mut off).is_empty());
}

//! The JSON Schema of a rule as a model fills it in (MailCraft's AI writing a rule from the
//! person's words, as text2sql writes a query): what [`Rule`] reads, moving mail only to the
//! account's folders, its conditions nested no deeper than a schema without recursion says
//! (Anthropic's structured output takes none). The answer is read straight into a `Rule`.

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{Rule, TextOp};

/// How deep the conditions of a rule written from words combine: all, any or not, three deep
/// over the tests (within [`super::CONDITION_DEPTH`]).
pub const WORDS_DEPTH: usize = 3;

/// A model's answer: the rule, or why what was asked can't be one.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleAnswer {
    pub rule: Option<Rule>,
    pub why_not: Option<String>,
}

/// The schema of a [`RuleAnswer`] whose rule moves mail only to `folders` (none: it moves mail
/// to no folder of its own, only to Archive or Junk).
pub fn rule_answer_schema(folders: &[String]) -> Value {
    let mut defs = Map::new();
    defs.insert("test".into(), json!({ "anyOf": tests() }));
    for depth in 1..=WORDS_DEPTH {
        let inner = if depth == 1 { "#/$defs/test".to_string() } else { format!("#/$defs/where{}", depth - 1) };
        defs.insert(format!("where{depth}"), combined(&inner));
    }
    defs.insert("action".into(), json!({ "anyOf": actions(folders) }));
    let rule = object(
        "A rule MailCraft runs on each message as it arrives (or, `sent`, on each the person sends).",
        &[
            ("name", json!({"type": "string", "description": "A short name for the rule, in the person's language."})),
            ("where", json!({"$ref": format!("#/$defs/where{WORDS_DEPTH}"), "description": "What the rule looks at, as a query's where clause."})),
            (
                "actions",
                json!({"type": "array", "items": {"$ref": "#/$defs/action"}, "description": "What it does to a message it matches, in order."}),
            ),
            ("stop", json!({"type": "boolean", "description": "true: no later rule runs on a message this one matched."})),
            ("sent", json!({"type": "boolean", "description": "true only for mail the person sends, not mail arriving."})),
        ],
    );
    defs.insert("rule".into(), rule);
    let mut answer = object(
        "The rule asked for, or why it can't be one.",
        &[
            ("rule", json!({"anyOf": [{"$ref": "#/$defs/rule"}, {"type": "null"}]})),
            (
                "whyNot",
                json!({"anyOf": [{"type": "string"}, {"type": "null"}], "description": "When no rule can do what was asked: why, in one sentence; else null."}),
            ),
        ],
    );
    answer["$defs"] = Value::Object(defs);
    answer
}

/// An object of exactly `properties`, each of them required (as strict structured output asks).
fn object(description: &str, properties: &[(&str, Value)]) -> Value {
    let required: Vec<&str> = properties.iter().map(|(k, _)| *k).collect();
    let properties: Map<String, Value> = properties.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect();
    json!({"type": "object", "description": description, "additionalProperties": false, "required": required, "properties": properties})
}

/// One of a tagged enum's variants: its tag `key` naming it `name`, with `more` beside it.
fn tagged(key: &str, name: &str, description: &str, more: &[(&str, Value)]) -> Value {
    let tag = (key, json!({"type": "string", "enum": [name]}));
    let properties: Vec<(&str, Value)> = std::iter::once(tag).chain(more.iter().cloned()).collect();
    object(description, &properties)
}

/// The tests of one field: every [`super::Condition`] that isn't all, any or not.
fn tests() -> Vec<Value> {
    let ops: Vec<&str> = TextOp::ALL.iter().map(|o| o.code()).collect();
    let text = |name: &str, description: &str| {
        let value = json!({"type": "string", "description": "A word, a name or an address, compared ignoring case."});
        let op = json!({"type": "string", "enum": ops});
        tagged("if", name, description, &[("value", value), ("op", op)])
    };
    let kb = || json!({"type": "integer", "description": "Kilobytes."});
    vec![
        text("from", "The sender's name or address."),
        text("to", "Any To or Cc recipient's name or address."),
        text("subject", "The subject."),
        text("anyText", "The subject, the sender or a recipient."),
        text("preview", "The start of the message's text."),
        tagged("if", "hasAttachment", "It has attachments.", &[]),
        tagged("if", "bulk", "Newsletters, mailing lists and automatic mail.", &[]),
        tagged(
            "if",
            "importance",
            "Marked by its sender as of high or low importance.",
            &[("value", json!({"type": "string", "enum": ["high", "low"]}))],
        ),
        tagged("if", "larger", "Larger than this.", &[("kb", kb())]),
        tagged("if", "smaller", "Smaller than this.", &[("kb", kb())]),
        tagged(
            "if",
            "asks",
            "A yes-or-no question about the message that MailCraft's AI answers from its text as it arrives, for what the other tests can't say \
             (whether it's a contract, a complaint, about a project). The message's text goes to the AI for each one it decides: only when no \
             other test can say it, and at most one in a rule.",
            &[("question", json!({"type": "string", "description": "The question, answered yes or no about one message."}))],
        ),
    ]
}

/// A test, or all, any or not of conditions `inner` says.
fn combined(inner: &str) -> Value {
    let list = json!({"type": "array", "items": {"$ref": inner}});
    json!({"anyOf": [
        {"$ref": "#/$defs/test"},
        tagged("if", "all", "Every one of these.", &[("of", list.clone())]),
        tagged("if", "any", "At least one of these.", &[("of", list)]),
        tagged("if", "not", "Not this.", &[("of", json!({"$ref": inner}))]),
    ]})
}

/// What a rule can do; a move only to one of `folders`.
fn actions(folders: &[String]) -> Vec<Value> {
    let moves =
        (!folders.is_empty()).then(|| tagged("do", "moveTo", "Move it to this folder.", &[("value", json!({"type": "string", "enum": folders}))]));
    let role = json!({"type": "string", "enum": ["archive", "junk"]});
    let category = json!({"type": "string", "description": "A category's name: one there is, or a new one."});
    moves
        .into_iter()
        .chain([
            tagged("do", "moveToRole", "Move it to Archive or Junk.", &[("value", role)]),
            tagged("do", "categorize", "Give it a category.", &[("value", category)]),
            tagged("do", "markRead", "Mark it read.", &[]),
            tagged("do", "flag", "Flag it for follow-up.", &[]),
            tagged("do", "pin", "Pin it to the top of the list.", &[]),
            tagged("do", "delete", "Move it to Deleted Items.", &[]),
        ])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FolderRole;
    use crate::organize::{Condition, Importance, RuleAction};

    const FOLDERS: [&str; 2] = ["Projects", "Projects/Short film"];

    fn schema() -> Value {
        rule_answer_schema(&FOLDERS.map(String::from))
    }

    /// Whether `v` keeps to `schema`, a part of `root` (the keywords this module writes).
    fn fits(root: &Value, schema: &Value, v: &Value) -> bool {
        if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
            return fits(root, &root["$defs"][r.trim_start_matches("#/$defs/")], v);
        }
        if let Some(any) = schema.get("anyOf").and_then(Value::as_array) {
            return any.iter().any(|s| fits(root, s, v));
        }
        if schema.get("enum").and_then(Value::as_array).is_some_and(|e| !e.contains(v)) {
            return false;
        }
        match schema["type"].as_str() {
            Some("object") => v.as_object().is_some_and(|o| {
                let props = &schema["properties"];
                schema["required"].as_array().unwrap().iter().all(|k| o.contains_key(k.as_str().unwrap()))
                    && o.iter().all(|(k, x)| props.get(k).is_some_and(|s| fits(root, s, x)))
            }),
            Some("array") => v.as_array().is_some_and(|a| a.iter().all(|x| fits(root, &schema["items"], x))),
            Some("string") => v.is_string(),
            Some("integer") => v.is_u64() || v.is_i64(),
            Some("boolean") => v.is_boolean(),
            Some("null") => v.is_null(),
            _ => false,
        }
    }

    /// `c` as a model writes it: each text test with its comparison said, as the schema asks.
    fn written(c: &Condition) -> Value {
        let mut v = serde_json::to_value(c).unwrap();
        say_ops(&mut v);
        v
    }

    fn say_ops(v: &mut Value) {
        let Some(o) = v.as_object_mut() else { return };
        if o.get("value").is_some_and(Value::is_string) && o["if"] != "importance" && !o.contains_key("op") {
            o.insert("op".into(), json!("contains"));
        }
        match o.get_mut("of") {
            Some(Value::Array(of)) => of.iter_mut().for_each(say_ops),
            Some(of) => say_ops(of),
            None => {}
        }
    }

    /// One test of each kind. A new kind fails to build here until it's added, and to the schema.
    fn every_test() -> Vec<Condition> {
        let text = |op| -> String { format!("{op:?}") };
        let tests = vec![
            Condition::From { value: text(TextOp::Is), op: TextOp::Is },
            Condition::To { value: text(TextOp::StartsWith), op: TextOp::StartsWith },
            Condition::Subject { value: text(TextOp::EndsWith), op: TextOp::EndsWith },
            Condition::AnyText { value: text(TextOp::Contains), op: TextOp::Contains },
            Condition::Preview { value: "invoice".into(), op: TextOp::Contains },
            Condition::HasAttachment,
            Condition::Bulk,
            Condition::Importance { value: Importance::High },
            Condition::Importance { value: Importance::Low },
            Condition::Larger { kb: 500 },
            Condition::Smaller { kb: 20 },
            Condition::Asks { question: "Is it a contract to sign?".into() },
        ];
        for t in &tests {
            match t {
                Condition::From { .. } | Condition::To { .. } | Condition::Subject { .. } | Condition::AnyText { .. } | Condition::Preview { .. } => {
                }
                Condition::HasAttachment | Condition::Bulk | Condition::Importance { .. } | Condition::Larger { .. } | Condition::Smaller { .. } => {}
                Condition::Asks { .. } => {}
                Condition::All { .. } | Condition::Any { .. } | Condition::Not { .. } => panic!("a test, not a combination"),
            }
        }
        tests
    }

    /// The rule as its answer says it: what the model fills in, no more.
    fn answer_of(rule: &Rule) -> Value {
        json!({"rule": {"name": rule.name, "where": written(&rule.filter), "actions": rule.actions, "stop": rule.stop, "sent": rule.sent}, "whyNot": null})
    }

    fn rule(filter: Condition, actions: Vec<RuleAction>) -> Rule {
        Rule { name: "Receipts".into(), enabled: true, account: None, filter, actions, stop: true, sent: false }
    }

    /// Every test, nested as deep as the schema goes, and every action fit the schema, and the
    /// answer reads back as the rule it says: a model fills in what MailCraft runs.
    #[test]
    fn every_rule_the_schema_allows_reads_back() {
        let s = schema();
        let tests = every_test();
        let deepest =
            Condition::all(vec![Condition::Any { of: tests.clone() }, Condition::Not { of: Box::new(Condition::all(vec![Condition::Bulk])) }]);
        assert_eq!(deepest.depth(), WORDS_DEPTH + 1);
        let actions = vec![
            RuleAction::MoveTo("Projects/Short film".into()),
            RuleAction::MoveToRole(FolderRole::Archive),
            RuleAction::MoveToRole(FolderRole::Junk),
            RuleAction::Categorize("Receipt".into()),
            RuleAction::MarkRead,
            RuleAction::Flag,
            RuleAction::Pin,
            RuleAction::Delete,
        ];
        let r = rule(deepest, actions);
        let answer = answer_of(&r);
        assert!(fits(&s, &s, &answer), "{answer}");
        let back: RuleAnswer = serde_json::from_value(answer).unwrap();
        assert_eq!((back.rule, back.why_not), (Some(r), None));
        let no: RuleAnswer = serde_json::from_value(json!({"rule": null, "whyNot": "A rule can't read attachments."})).unwrap();
        assert!(no.rule.is_none() && no.why_not.is_some());
        for t in tests {
            assert!(fits(&s, &s, &answer_of(&rule(t.clone(), vec![RuleAction::Flag]))), "{t:?}");
        }
    }

    /// What MailCraft can't run doesn't fit: a folder that isn't the account's, conditions
    /// nested past the schema's depth, a property it doesn't have.
    #[test]
    fn what_mailcraft_cant_run_doesnt_fit() {
        let s = schema();
        let flag = vec![RuleAction::Flag];
        let elsewhere = answer_of(&rule(Condition::Bulk, vec![RuleAction::MoveTo("Receipts".into())]));
        assert!(!fits(&s, &s, &elsewhere));
        let mut deep = Condition::Bulk;
        for _ in 0..=WORDS_DEPTH {
            deep = Condition::Not { of: Box::new(deep) };
        }
        assert!(!fits(&s, &s, &answer_of(&rule(deep, flag.clone()))));
        let mut more = answer_of(&rule(Condition::Bulk, flag));
        more["rule"]["account"] = json!("work");
        assert!(!fits(&s, &s, &more), "the account is MailCraft's to say");
        let none = rule_answer_schema(&[]);
        assert!(!fits(&none, &none, &answer_of(&rule(Condition::Bulk, vec![RuleAction::MoveTo("Projects".into())]))), "no folders, no move to one");
        assert!(fits(&none, &none, &answer_of(&rule(Condition::Bulk, vec![RuleAction::MoveToRole(FolderRole::Archive)]))));
    }

    /// The schema is one every server's structured output takes: no recursion (Anthropic's takes
    /// none), every object closed with each property required (OpenAI's strict mode), and only
    /// the keywords all of them know.
    #[test]
    fn the_schema_is_one_every_server_takes() {
        const KNOWN: [&str; 10] =
            ["type", "description", "additionalProperties", "required", "properties", "enum", "anyOf", "$ref", "$defs", "items"];
        fn check(v: &Value, refs: &mut Vec<String>) {
            match v {
                Value::Object(o) => {
                    if o.get("type") == Some(&json!("object")) {
                        assert_eq!(o.get("additionalProperties"), Some(&json!(false)), "{v}");
                        let mut keys: Vec<&String> = o["properties"].as_object().unwrap().keys().collect();
                        let mut required: Vec<&str> = o["required"].as_array().unwrap().iter().map(|k| k.as_str().unwrap()).collect();
                        keys.sort();
                        required.sort_unstable();
                        assert_eq!(keys, required, "{v}");
                    }
                    for (k, x) in o {
                        if k == "properties" || k == "$defs" {
                            x.as_object().unwrap().values().for_each(|p| check(p, refs));
                        } else {
                            assert!(KNOWN.contains(&k.as_str()), "{k}");
                            if k == "$ref" {
                                refs.push(x.as_str().unwrap().trim_start_matches("#/$defs/").to_string());
                            }
                            check(x, refs);
                        }
                    }
                }
                Value::Array(a) => a.iter().for_each(|x| check(x, refs)),
                _ => {}
            }
        }
        let s = schema();
        let defs = s["$defs"].as_object().unwrap();
        check(&s, &mut Vec::new());
        // Each definition reaches only others, never itself: no recursion.
        let refs_of = |name: &str| {
            let mut refs = Vec::new();
            check(&defs[name], &mut refs);
            refs
        };
        for name in defs.keys() {
            let mut seen: Vec<String> = refs_of(name);
            let mut i = 0;
            while let Some(next) = seen.get(i).cloned() {
                assert_ne!(&next, name, "{name} reaches itself");
                for r in refs_of(&next) {
                    if !seen.contains(&r) {
                        seen.push(r);
                    }
                }
                i += 1;
            }
        }
    }

    /// Every test and action the schema offers, filled in, is one MailCraft reads.
    #[test]
    fn everything_the_schema_offers_is_read() {
        let s = schema();
        let filled = |branch: &Value| -> Value {
            let o: Map<String, Value> = branch["properties"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, p)| {
                    let v = match (p.get("enum").and_then(|e| e.get(0)), p["type"].as_str()) {
                        (Some(first), _) => first.clone(),
                        (None, Some("integer")) => json!(1),
                        _ => json!("x"),
                    };
                    (k.clone(), v)
                })
                .collect();
            Value::Object(o)
        };
        let tests = s["$defs"]["test"]["anyOf"].as_array().unwrap();
        assert_eq!(tests.len(), 11);
        for t in tests {
            let c: Condition = serde_json::from_value(filled(t)).unwrap();
            assert!(c.simple().is_some());
        }
        for a in s["$defs"]["action"]["anyOf"].as_array().unwrap() {
            let _: RuleAction = serde_json::from_value(filled(a)).unwrap();
        }
    }
}

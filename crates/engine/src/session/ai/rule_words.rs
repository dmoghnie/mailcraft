//! Rules in plain words, as text2sql writes a query from a question: what the person says a rule
//! should do ("receipts from the print shop go to Finance and get the Receipt category") made by
//! the chat model into one of MailCraft's own rules. The model fills in the rule's own schema
//! (its conditions, the account's folders), so its answer is read straight into the rule
//! MailCraft runs. They read it, and add it or let it go, in Home › Rules: nothing changes until
//! they add it. Only their words go, with the account's folders and the categories' names:
//! never any mail.

use mailcraft_model::organize::{Rule, RuleAction, RuleAnswer, rule_answer_schema};
use mailcraft_model::{AiAnswer, AiChat};
use serde_json::{Value, json};

use super::AiTask;
use super::assistant::cut;
use crate::{CmdError, CommandSpec, Session, p};

const WHERE: &str = "Home › Rules";
/// What the record of what went where says its questions were for.
const PURPOSE: &str = "writing a rule";
/// The most of the person's words taken.
const WORDS_MOST: usize = 1_000;
/// The longest answer: a rule, with room for a model that thinks before it writes (a 12B local
/// one thought for up to 3,500 tokens, and wrote no rule at all within 4,000; without thinking,
/// it combined with "any" what was said with "and").
const ANSWER_TOKENS: u32 = 16_000;
/// The most folders a rule may move mail to, and the longest rule name kept.
const FOLDERS_MOST: usize = 300;
const NAME_MOST: usize = 80;

const SYSTEM: &str = "You turn what a person says an email rule should do into a rule for MailCraft, their email program, as a database query is written from a question. Fill in the JSON your answer is given: the rule's `where` is its where clause, tests of a message's fields combined with all, any and not; its actions are what it does, in order. What must all hold (said with \"and\", \"that\", \"with\", \"but not\") goes in `all`; `any` is only for alternatives (said with \"or\", or a list of things any one of which will do); \"not\" or \"but not\" wraps a test in `not`. Test only what they said, nothing more. A rule looks at a message as it arrives, so it can't know what the person will do with it later (read it, answer it), nor what's inside an attachment beyond there being one. Move mail only to a folder the schema lists. A category may be one there is or a new one: naming one is fine. When no rule can do what they ask (it needs to look at or do something a rule can't), give no rule and say why in `whyNot`.";

/// A rule asked for in words: on its way, written, or why it couldn't be.
#[derive(Clone, Debug, Default)]
pub struct RuleProposal {
    pub words: String,
    /// The account whose folders it may move to.
    account: String,
    /// The question on its way.
    pub asking: Option<u64>,
    /// The profile asked: the one that answered, once one did (its model is named with it).
    pub profile: String,
    pub rule: Option<Rule>,
    pub error: Option<String>,
}

impl RuleProposal {
    /// The model is writing it.
    pub fn writing(&self) -> bool {
        self.asking.is_some()
    }
}

impl Session {
    /// The rule asked for in words, while there is one: on its way, written, or why not.
    pub fn rule_proposal(&self) -> Option<&RuleProposal> {
        self.ai.rule_words.as_ref()
    }

    /// Ask the chat model for a rule doing what `words` say, for `account`'s folders (the first
    /// account whose mail may go to a model, when none is named).
    fn rule_from_words(&mut self, words: &str, account: Option<&str>) -> Result<(), CmdError> {
        let words: String = words.trim().chars().take(WORDS_MOST).collect();
        if words.is_empty() {
            return Err(CmdError::Params("`words` (what the rule should do) is required".into()));
        }
        let account = match account {
            Some(a) if self.account(a).is_some() => a.to_string(),
            Some(_) => return Err(CmdError::Params("`account`: no such account".into())),
            None => {
                let ids: Vec<String> = self.accounts.iter().map(|a| a.config.id.clone()).collect();
                let first = ids.iter().find(|a| self.ai_route(AiTask::Chat, Some(a.as_str())).is_ok()).or(ids.first());
                first.cloned().ok_or_else(|| CmdError::Disabled("add an account first".into()))?
            }
        };
        let folders: Vec<String> = self
            .account(&account)
            .map(|a| a.folders.iter().filter(|f| f.selectable).take(FOLDERS_MOST).map(|f| f.path.clone()).collect())
            .unwrap_or_default();
        let categories: Vec<&str> = self.prefs.categories.iter().map(|c| c.name.as_str()).collect();
        let question = format!(
            "What the rule should do: {words}\n\nThe folders it may move mail to: {}\n\nThe categories there are: {}",
            folders.join(", "),
            categories.join(", ")
        );
        // The schema in the instructions too: a server that keeps the answer to it while it's
        // written (Ollama's `format`, llama.cpp's grammar) doesn't show it to the model.
        let schema = rule_answer_schema(&folders);
        let system = format!("{SYSTEM}\n\nThe answer's JSON Schema:\n{schema}");
        let chat = AiChat { answer_schema: Some(schema), ..AiChat::ask(&system, &question, ANSWER_TOKENS) };
        let profile = self.ai_route(AiTask::Chat, Some(&account)).map(|p| p.id.clone()).unwrap_or_default();
        let ask = self.ai_ask(AiTask::Chat, Some(&account), PURPOSE, chat, false)?;
        self.ai.rule_words = Some(RuleProposal { words, account, asking: Some(ask), profile, rule: None, error: None });
        Ok(())
    }

    /// Question `id`'s answer, if it's the rule's: the rule it says, checked. Whether it was.
    pub(crate) fn rule_words_came(&mut self, id: u64, profile: &str, result: &Result<AiAnswer, String>) -> bool {
        let Some(account) = self.ai.rule_words.as_ref().filter(|r| r.asking == Some(id)).map(|r| r.account.clone()) else { return false };
        let made = match result {
            Ok(a) => self.rule_said(&a.text, &account),
            Err(e) => Err(e.clone()),
        };
        if let Some(r) = self.ai.rule_words.as_mut() {
            r.asking = None;
            r.profile = profile.to_string();
            (r.rule, r.error) = match made {
                Ok(rule) => (Some(rule), None),
                Err(e) => (None, Some(e)),
            };
        }
        true
    }

    /// The rule in the model's `answer`, checked for `account`: a name, something to look at
    /// and something to do, a move only to one of its folders (and then for its mail alone). A
    /// server that kept the answer to its schema gave the JSON alone; one that wrapped it (in a
    /// Markdown fence) is read from its braces.
    fn rule_said(&self, answer: &str, account: &str) -> Result<Rule, String> {
        let braces = answer.find('{').zip(answer.rfind('}')).and_then(|(a, b)| answer.get(a..=b));
        let read = |text: &str| serde_json::from_str::<RuleAnswer>(text);
        let said = read(answer.trim()).or_else(|e| braces.map_or(Err(e), read)).map_err(|e| format!("the model's rule couldn't be read: {e}"))?;
        let mut rule = match (said.rule, said.why_not) {
            (Some(rule), _) => rule,
            (None, Some(why)) => return Err(format!("the model says it can't be a rule: {}", cut(why.trim(), 300))),
            (None, None) => return Err("the model gave no rule".into()),
        };
        rule.enabled = true;
        rule.account = None;
        rule.name = cut(rule.name.trim(), NAME_MOST);
        if rule.name.is_empty() || rule.actions.is_empty() {
            return Err("the model's rule is missing its name or what it does".into());
        }
        if let Some(why) = rule.filter.refused() {
            return Err(format!("the model's rule can't run: {why}"));
        }
        let folders = self.account(account).map(|a| a.folders.clone()).unwrap_or_default();
        let mut moves = false;
        for a in &mut rule.actions {
            if let RuleAction::MoveTo(path) = a {
                let found =
                    folders.iter().find(|f| f.path.eq_ignore_ascii_case(path.trim())).ok_or_else(|| format!("there's no folder called {path}"))?;
                path.clone_from(&found.path);
                moves = true;
            }
            if let RuleAction::Categorize(name) = a {
                *name = self.category(name.trim()).map_or_else(|| cut(name.trim(), NAME_MOST), |c| c.name.clone());
            }
        }
        rule.account = moves.then(|| account.to_string());
        Ok(rule)
    }

    /// Add the rule written: the categories it names that aren't there yet made first.
    fn add_proposed_rule(&mut self) -> Result<Value, CmdError> {
        let rule =
            self.ai.rule_words.as_ref().and_then(|r| r.rule.clone()).ok_or_else(|| CmdError::Failed("there's no rule written to add".into()))?;
        for a in &rule.actions {
            if let RuleAction::Categorize(name) = a
                && self.category(name).is_none()
            {
                self.add_category(name, None, None)?;
            }
        }
        let mut rules = self.rules.clone();
        rules.push(rule);
        self.set_rules(rules)?;
        self.ai.rule_words = None;
        Ok(rule_shown(self))
    }
}

/// The rule asked for in words: what was said, whether it's being written, the rule, and why
/// it couldn't be.
fn rule_shown(s: &Session) -> Value {
    match &s.ai.rule_words {
        None => json!({"proposal": null}),
        Some(r) => json!({"proposal": {"words": r.words, "writing": r.writing(), "rule": r.rule, "error": r.error}}),
    }
}

pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("rules.fromWords", "Write a rule from my words", WHERE, |s, v| {
            s.rule_from_words(p::str(v, "words").unwrap_or_default(), p::str(v, "account"))?;
            Ok(rule_shown(s))
        })
        .params(r#"{"words": string (what the rule should do: "receipts go to Finance and get the Receipt category"), "account"?: string (whose folders it may move to)} (the words, the account's folder names and the categories' go to the model chat work goes to, as far as the account lets its mail; never any mail. The model fills in the rule's own schema; the rule is a proposal: rules.addProposed adds it)"#),
        CommandSpec::new("rules.proposed", "Show the rule written from my words", WHERE, |s, _| Ok(rule_shown(s))),
        CommandSpec::new("rules.addProposed", "Add the rule written from my words", WHERE, |s, _| s.add_proposed_rule())
            .params(r#"{} (with the categories it names that aren't there yet)"#),
        CommandSpec::new("rules.dropProposed", "Let the rule written from my words go", WHERE, |s, _| {
            s.ai.rule_words = None;
            Ok(rule_shown(s))
        }),
    ]
}

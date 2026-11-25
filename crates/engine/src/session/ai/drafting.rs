//! Replies drafted by MailCraft's AI: mail that Needs me marks, in an account where the person
//! turned drafting on, gets a reply written by the model chat work goes to, in their style, and
//! kept in the account's Drafts for them to check, change and send. Nothing is ever sent by it;
//! a message gets one draft at most, never one the person answered, sent or that came encrypted.

use std::collections::{HashMap, VecDeque};

use mailcraft_mime::ReplyMode;
use mailcraft_mime::rich::{self, Styles};
use mailcraft_model::{AiAnswer, AiChat, Job, MessageKey};
use serde_json::{Value, json};

use super::AiTask;
use super::rewriting::{ANSWER_IT, WRITE};
use crate::session::{AccountState, Reading};
use crate::{CmdError, CommandSpec, Session, p};

/// What the record of what went where says its questions were for.
const PURPOSE: &str = "drafting a reply";
/// The longest reply asked for, in tokens (the profile's own longest answer is the most).
const ANSWER_TOKENS: u32 = 2_000;
/// Replies being written at once: the rest wait their turn.
const ASKING_MOST: usize = 2;
/// Messages waiting their turn or their source: a burst past it isn't drafted.
const WAITING_MOST: usize = 50;
/// The messages drafted for, remembered so none gets two.
const DONE_KEPT: usize = 2_000;
/// Said when a draft is kept.
const DRAFTED: &str = "MailCraft's AI drafted a reply: it's in Drafts";

/// Messages to draft for, in turn, and those being.
#[derive(Default)]
pub(crate) struct Drafting {
    /// Each with whether its source was downloaded for it already.
    queue: VecDeque<(MessageKey, bool)>,
    /// Their sources on their way.
    downloading: Vec<MessageKey>,
    asking: HashMap<u64, MessageKey>,
    /// The ids of those drafted for, or being, oldest first.
    done: VecDeque<String>,
}

impl Session {
    /// Message `key` was marked Needs me: a reply drafted for it, in turn, when its account
    /// has drafting on.
    pub(crate) fn draft_for(&mut self, key: &MessageKey) {
        let d = &mut self.ai.drafting;
        let id = key.to_id();
        if !self.prefs.ai.auto_drafts.contains(&key.account) || d.done.contains(&id) || d.queue.len() >= WAITING_MOST {
            return;
        }
        d.done.push_back(id);
        while d.done.len() > DONE_KEPT {
            d.done.pop_front();
        }
        d.queue.push_back((key.clone(), false));
        self.drafting_next();
    }

    /// Ask for those waiting, as far as there's room.
    fn drafting_next(&mut self) {
        while self.ai.drafting.asking.len() < ASKING_MOST {
            let Some((key, fetched)) = self.ai.drafting.queue.pop_front() else { return };
            self.draft_ask(key, fetched);
        }
    }

    /// Ask the model for a reply to message `key`, its source downloaded first (once: `fetched`
    /// when it was). One answered, the person's own or encrypted is passed over, as is one whose
    /// account's mail may no longer go to a model.
    fn draft_ask(&mut self, key: MessageKey, fetched: bool) {
        let Some(m) = self.shared_of(&key) else { return };
        if m.flags.answered || self.is_own_address(&key.account, &m.from.email) || !self.prefs.ai.auto_drafts.contains(&key.account) {
            return;
        }
        let Some(raw) = self.body_raw(&key) else {
            let local = self.account(&key.account).is_none_or(AccountState::is_local);
            if !fetched && !local && self.ai.drafting.downloading.len() < WAITING_MOST {
                self.jobs.push(Job::FetchBody { key: key.clone() });
                self.ai.drafting.downloading.push(key);
            }
            return;
        };
        if self.as_written(&key, raw).2 {
            return;
        }
        let Some(given) = self.message_given(&key) else { return };
        let system = format!("{WRITE} Write in the language the message it answers is written in.{}", self.style_said());
        let question = format!("What it should say: {ANSWER_IT}\nTo: {}\nSubject: {}\n\nThe message it answers:\n{given}", m.from, m.subject);
        let chat = AiChat::ask(&system, &question, ANSWER_TOKENS);
        if let Ok(id) = self.ai_ask(AiTask::Chat, Some(&key.account), PURPOSE, chat, false) {
            self.ai.drafting.asking.insert(id, key);
        }
    }

    /// Message `key`'s source came (or `failed` to): the reply waiting for it is asked for.
    pub(crate) fn drafting_body(&mut self, key: &MessageKey, failed: bool) {
        let d = &mut self.ai.drafting;
        let Some(at) = d.downloading.iter().position(|k| k == key) else { return };
        d.downloading.remove(at);
        if !failed {
            d.queue.push_front((key.clone(), true));
            self.drafting_next();
        }
    }

    /// Question `id`'s answer, if it was a reply's: kept in Drafts, and the next asked. Whether
    /// it was one.
    pub(crate) fn drafting_came(&mut self, id: u64, result: &Result<AiAnswer, String>) -> bool {
        let Some(key) = self.ai.drafting.asking.remove(&id) else { return false };
        let kept = match result {
            Ok(a) if a.text.trim().is_empty() => Err(CmdError::Failed("the model gave no answer".into())),
            // Turned off since it was asked for: let go.
            Ok(_) if !self.prefs.ai.auto_drafts.contains(&key.account) => Ok(()),
            Ok(a) => self.keep_drafted(&key, a.text.trim()),
            Err(e) => Err(CmdError::Failed(e.clone())),
        };
        if let Err(e) = kept {
            log::warn!("a reply couldn't be drafted: {e}");
        }
        self.drafting_next();
        true
    }

    /// The reply to message `key`, `text` written above what it quotes, kept in Drafts.
    fn keep_drafted(&mut self, key: &MessageKey, text: &str) -> Result<(), CmdError> {
        let raw = self.readable_raw(key).ok_or_else(|| CmdError::Failed("its source isn't here any more".into()))?;
        let parsed = mailcraft_mime::parse(&raw).ok_or_else(|| CmdError::Failed("it couldn't be read".into()))?;
        let parsed = self.for_reading(key, parsed);
        let r = Reading {
            key: key.clone(),
            parsed: Some(parsed),
            error: None,
            thread: Vec::new(),
            aside: Vec::new(),
            warning: None,
            security: None,
            text_first: false,
        };
        let (mut c, signature) = self.reply_to(r, ReplyMode::Reply)?;
        self.style_signature_in(&mut c, signature.as_ref());
        self.smime_defaults(&mut c);
        rich::splice(&mut c.body, &mut c.styles, 0..0, text, &Styles::plain(text));
        self.file_draft(&c)?;
        self.notice(DRAFTED, false);
        Ok(())
    }

    /// Whether replies are drafted for account `account`'s mail that needs the person.
    fn drafts_shown(&self, account: &str) -> Value {
        json!({"account": account, "on": self.prefs.ai.auto_drafts.contains(account), "needsMe": self.prefs.ai.needs_me})
    }
}

pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![CommandSpec::new("ai.autoDrafts", "Draft replies to mail that needs me", "File › Options › AI", |s, v| {
        let account = p::req_str(v, "account")?.to_string();
        if s.account(&account).is_none() {
            return Err(CmdError::Params("`account`: no such account".into()));
        }
        let on = p::bool(v, "on").unwrap_or(!s.prefs.ai.auto_drafts.contains(&account));
        if on {
            s.prefs.ai.auto_drafts.insert(account.clone());
        } else {
            s.prefs.ai.auto_drafts.remove(&account);
            let d = &mut s.ai.drafting;
            d.queue.retain(|(k, _)| k.account != account);
            d.downloading.retain(|k| k.account != account);
        }
        Ok(s.drafts_shown(&account))
    })
    .params(r#"{"account": string, "on"?: bool (none: switch)} (mail of the account that Needs me marks gets a reply written by the model chat work goes to, where the account's mail may go to it, kept in Drafts and never sent; Needs me must be on)"#)]
}

//! MailCraft's AI as it runs: questions sent and waiting, a profile's test, and the record of
//! what went where, which outlives a restart (its sizes, never its content).

use std::collections::{BTreeMap, HashMap, VecDeque};

use mailcraft_model::{AiAnswer, AiChat, AiDone, AiWork, Job};
use serde::{Deserialize, Serialize};

use super::{AiProfile, AiTask};
use crate::{CmdError, Session};

/// The record's file in the cache.
pub(crate) const AI_LOG_DOC: &str = "ai-log.json";
/// The most of the record kept: the oldest go first.
pub const AI_LOG_KEPT: usize = 500;
/// The most of answers kept for their askers to take.
const ANSWERS_KEPT: usize = 64;
/// What a test asks.
const TEST_QUESTION: &str = "Reply with the single word OK.";
/// The longest answer a test asks for (room for a model that thinks first).
const TEST_TOKENS: u32 = 1024;
/// What a test is recorded as being for.
pub(crate) const TEST_PURPOSE: &str = "test";

/// One question as it went: where, for what, and how big; never what it said.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AiLogEntry {
    /// When it was sent (Unix seconds).
    pub at: i64,
    /// What it was for: `test`, `summary`…
    pub purpose: String,
    /// The profile asked, by its name then.
    pub profile: String,
    /// The server it went to.
    pub server: String,
    pub model: String,
    /// The account whose mail it held, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// Bytes of the question, and of the answer once it came.
    pub sent: u64,
    pub received: u64,
    /// `waiting`, `answered` or `failed`.
    pub outcome: String,
}

/// A profile's last test (`ai.test`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum AiTest {
    Asking,
    Answered { text: String },
    Failed { error: String },
}

/// A question waiting for its answer.
struct Waiting {
    account: Option<String>,
    purpose: String,
    chat: AiChat,
    profile: String,
    /// The profiles asked so far, the one waiting last.
    tried: Vec<String>,
    /// Its line in the record.
    logged: u64,
}

/// MailCraft's AI as it runs.
#[derive(Default)]
pub struct AiState {
    next: u64,
    waiting: HashMap<u64, Waiting>,
    /// Each profile's last test, by its id.
    pub tests: BTreeMap<String, AiTest>,
    /// What went where, oldest first.
    pub log: VecDeque<(u64, AiLogEntry)>,
    next_line: u64,
    /// The record couldn't be read: it isn't saved over.
    log_unread: bool,
    /// The record changed since it was last saved.
    log_changed: bool,
    /// Answers come back by question, for their askers to take.
    answers: HashMap<u64, Result<AiAnswer, String>>,
    /// Why a key couldn't be kept, by profile id.
    pub key_errors: BTreeMap<String, String>,
}

impl AiState {
    /// Questions waiting now.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// Whether question `id` waits for its answer.
    pub fn is_waiting(&self, id: u64) -> bool {
        self.waiting.contains_key(&id)
    }
}

impl Session {
    /// The record of what went where, as the last run left it.
    pub(crate) fn load_ai_log(&mut self) {
        let Some(store) = self.store.clone() else { return };
        match store.load_doc::<Vec<AiLogEntry>>(AI_LOG_DOC) {
            Ok(entries) => {
                let skip = entries.len().saturating_sub(AI_LOG_KEPT);
                for e in entries.into_iter().skip(skip) {
                    let line = self.ai.next_line;
                    self.ai.next_line += 1;
                    self.ai.log.push_back((line, e));
                }
            }
            Err(e) => {
                log::warn!("the record of what MailCraft's AI sent couldn't be read: {e}");
                self.ai.log_unread = true;
            }
        }
    }

    /// The record saved, once it changed: as the jobs are taken, so it's kept before a
    /// question leaves, once for all the questions a pass of the app sends.
    pub(crate) fn save_ai_log(&mut self) {
        if !std::mem::take(&mut self.ai.log_changed) || self.ai.log_unread {
            return;
        }
        let entries: Vec<&AiLogEntry> = self.ai.log.iter().map(|(_, e)| e).collect();
        if let Some(store) = &self.store
            && let Err(e) = store.save_doc(AI_LOG_DOC, &entries)
        {
            log::warn!("the record of what MailCraft's AI sent couldn't be saved: {e}");
        }
    }

    /// Ask the model `task` goes to about mail of `account` (`None`: no mail in it), for
    /// `purpose`; the answer is taken with [`Session::ai_answer`] by the id returned.
    pub(crate) fn ai_ask(&mut self, task: AiTask, account: Option<&str>, purpose: &str, chat: AiChat) -> Result<u64, CmdError> {
        let profile = self.prefs.ai.route(task, account).map_err(CmdError::Disabled)?.clone();
        self.ai.next += 1;
        let id = self.ai.next;
        let waiting = Waiting {
            account: account.map(str::to_string),
            purpose: purpose.to_string(),
            chat,
            profile: profile.id.clone(),
            tried: Vec::new(),
            logged: 0,
        };
        self.ai_send(id, waiting, &profile);
        Ok(id)
    }

    /// Ask `profile` question `id`, and record it.
    fn ai_send(&mut self, id: u64, mut w: Waiting, profile: &AiProfile) {
        let chat = AiChat { max_tokens: w.chat.max_tokens.min(profile.max_tokens.max(1)), ..w.chat.clone() };
        let size = chat.system.len().saturating_add(chat.turns.iter().map(|t| t.text.len()).sum());
        w.logged = self.ai_record(AiLogEntry {
            at: self.clock.now,
            purpose: w.purpose.clone(),
            profile: profile.name.clone(),
            server: server_of(&profile.endpoint.url),
            model: profile.endpoint.model.clone(),
            account: w.account.clone(),
            sent: u64::try_from(size).unwrap_or(u64::MAX),
            received: 0,
            outcome: "waiting".into(),
        });
        w.profile = profile.id.clone();
        w.tried.push(profile.id.clone());
        let key = profile.has_key.then(|| profile.id.clone());
        self.jobs.push(Job::Ai { work: AiWork::Ask { id, endpoint: profile.endpoint.clone(), key, chat } });
        self.ai.waiting.insert(id, w);
    }

    fn ai_record(&mut self, entry: AiLogEntry) -> u64 {
        let line = self.ai.next_line;
        self.ai.next_line += 1;
        self.ai.log.push_back((line, entry));
        while self.ai.log.len() > AI_LOG_KEPT {
            self.ai.log.pop_front();
        }
        self.ai.log_changed = true;
        line
    }

    fn ai_settle(&mut self, line: u64, outcome: &str, received: u64) {
        if let Some((_, e)) = self.ai.log.iter_mut().find(|(l, _)| *l == line) {
            e.outcome = outcome.into();
            e.received = received;
            self.ai.log_changed = true;
        }
    }

    /// What the network layer's models did.
    pub(crate) fn ai_done(&mut self, done: AiDone) {
        match done {
            AiDone::Answered { id, answer, error } => {
                let Some(w) = self.ai.waiting.remove(&id) else { return };
                let result = match (answer, error) {
                    (Some(a), _) => Ok(a),
                    (None, e) => Err(e.unwrap_or_else(|| "no answer came".into())),
                };
                let received = result.as_ref().map_or(0, |a| a.received);
                self.ai_settle(w.logged, if result.is_ok() { "answered" } else { "failed" }, received);
                if result.is_err()
                    && let Some(next) = self.ai_fallback(&w)
                {
                    return self.ai_send(id, w, &next);
                }
                if w.purpose == TEST_PURPOSE {
                    let test = match &result {
                        Ok(a) => AiTest::Answered { text: a.text.trim().chars().take(200).collect() },
                        Err(e) => AiTest::Failed { error: e.clone() },
                    };
                    self.ai.tests.insert(w.profile.clone(), test);
                    return;
                }
                if self.ai.answers.len() >= ANSWERS_KEPT
                    && let Some(oldest) = self.ai.answers.keys().min().copied()
                {
                    self.ai.answers.remove(&oldest);
                }
                self.ai.answers.insert(id, result);
            }
            AiDone::KeySaved { name, error } => match error {
                Some(e) => {
                    if let Some(p) = self.prefs.ai.profiles.iter_mut().find(|p| p.id == name) {
                        p.has_key = false;
                    }
                    self.ai.key_errors.insert(name, e);
                }
                None => {
                    self.ai.key_errors.remove(&name);
                }
            },
        }
    }

    /// The profile a failed question goes to next: along the fallbacks of the one asked, the
    /// first allowed and not asked yet (each looked at once, however they loop).
    fn ai_fallback(&self, w: &Waiting) -> Option<AiProfile> {
        let ai = &self.prefs.ai;
        let mut seen: Vec<&str> = vec![w.profile.as_str()];
        let mut at = ai.profile(&w.profile).and_then(|p| p.fallback.as_deref()).and_then(|id| ai.profile(id));
        while let Some(p) = at.filter(|p| !seen.contains(&p.id.as_str())) {
            if !w.tried.contains(&p.id) && ai.allows(p, w.account.as_deref()) {
                return Some(p.clone());
            }
            seen.push(&p.id);
            at = p.fallback.as_deref().and_then(|id| ai.profile(id));
        }
        None
    }

    /// The answer to question `id`, once it came (taken: asked again, it's gone).
    pub(crate) fn ai_answer(&mut self, id: u64) -> Option<Result<AiAnswer, String>> {
        self.ai.answers.remove(&id)
    }

    /// Ask profile `id` a question that holds no mail, to see that it answers.
    pub(crate) fn ai_test(&mut self, id: &str) -> Result<(), CmdError> {
        let ai = &self.prefs.ai;
        let profile = ai.profile(id).ok_or_else(|| CmdError::Params(format!("no model profile {id}")))?.clone();
        if !ai.allows(&profile, None) {
            return Err(CmdError::Disabled(format!(
                "{} isn't on this computer or its network, and MailCraft's AI is set to local models only",
                profile.name
            )));
        }
        self.ai.next += 1;
        let question = self.ai.next;
        let waiting = Waiting {
            account: None,
            purpose: TEST_PURPOSE.into(),
            chat: AiChat::ask("", TEST_QUESTION, TEST_TOKENS),
            profile: profile.id.clone(),
            // Its own answer alone: a fallback's would say nothing of this one.
            tried: ai.profiles.iter().map(|p| p.id.clone()).collect(),
            logged: 0,
        };
        self.ai_send(question, waiting, &profile);
        self.ai.tests.insert(profile.id, AiTest::Asking);
        Ok(())
    }
}

/// The host (and port) of a server's address, as the record and File › Options › AI name it.
pub fn server_of(url: &str) -> String {
    let rest = url.trim().split_once("://").map_or(url.trim(), |(_, r)| r);
    rest.split(['/', '?', '#']).next().unwrap_or_default().rsplit('@').next().unwrap_or_default().to_ascii_lowercase()
}

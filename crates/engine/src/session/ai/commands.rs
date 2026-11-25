//! The commands that set MailCraft's AI up (File › Options › AI) and show what it sent.

use mailcraft_model::{AiApi, AiEndpoint, AiWork, Job, Secret};
use serde_json::{Value, json};

use super::{AiProfile, AiReach, AiTask, settings::CHATS_DAYS_MOST, settings::local_url};
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

const WHERE: &str = "File › Options › AI";
/// The most profiles kept.
const PROFILES_MOST: usize = 32;
/// The longest answer a profile may ask for, in tokens (the most any model writes today).
const TOKENS_MOST: u32 = 128_000;
/// A profile's answers at first, in tokens.
const TOKENS_AT_FIRST: u32 = 4096;
/// The most a model takes in, as a profile may say, in tokens (more than any takes today).
const CONTEXT_MOST: u32 = 16_000_000;
/// The least: below it, no chat with tools fits.
const CONTEXT_LEAST: u32 = 2048;
/// The longest name, model and address, in characters.
const TEXT_MOST: usize = 200;
/// The longest key, in bytes.
const KEY_MOST: usize = 4096;
/// The longest question `ai.ask` sends, its instructions with it, in bytes.
const QUESTION_MOST: usize = 1 << 20;

pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("ai.settings", "Show MailCraft's AI settings", WHERE, |s, _| Ok(settings(s))),
        CommandSpec::new("ai.saveProfile", "Add or change a model MailCraft's AI may ask", WHERE, save_profile).params(
            r#"{"id"?: string (none: a new profile), "name": string, "api": "openAi"|"anthropic"|"ollama" (Ollama's own, told what the model takes in), "url": string (the API address, up to its version: https://api.openai.com/v1; Ollama's own: http://localhost:11434), "model": string, "maxTokens"?: number, "context"?: number (what the model takes in at once, in tokens; 0: 32768, Ollama's own), "fallback"?: string (a profile asked instead when this one fails; "": none), "scripts"?: boolean (the assistant may run scripts it writes: code mode), "key"?: string (kept in the keychain; "": forgotten)}"#,
        ),
        CommandSpec::new("ai.removeProfile", "Remove a model and forget its key", WHERE, |s, v| {
            let id = p::req_str(v, "id")?.to_string();
            remove_profile(s, &id)
        })
        .params(r#"{"id": string}"#),
        CommandSpec::new("ai.use", "Choose the model for a kind of work", WHERE, use_profile)
            .params(r#"{"task": "chat"|"decision", "profile": string ("": the first profile)}"#),
        CommandSpec::new("ai.allow", "Choose how far an account's mail may go to a model", WHERE, allow)
            .params(r#"{"account": string, "reach": "off"|"local"|"any" (local: models on this computer or its network alone)}"#),
        CommandSpec::new("ai.localOnly", "Use models on this computer or its network alone", WHERE, |s, v| {
            s.prefs.ai.local_only = p::bool(v, "on").unwrap_or(!s.prefs.ai.local_only);
            Ok(json!({"on": s.prefs.ai.local_only}))
        })
        .params(r#"{"on"?: bool (none: switch)}"#),
        CommandSpec::new("ai.keepChats", "Choose how long chats with MailCraft's AI are kept", WHERE, |s, v| {
            let days = p::u64(v, "days").ok_or_else(|| CmdError::Params("`days` (number) is required".into()))?;
            let days = u32::try_from(days)
                .ok()
                .filter(|d| *d <= CHATS_DAYS_MOST)
                .ok_or_else(|| CmdError::Params(format!("at most {CHATS_DAYS_MOST} days")))?;
            s.prefs.ai.chats_days = days;
            Ok(json!({"days": days, "deleted": s.assistant_expire(true)}))
        })
        .params(r#"{"days": number (0: kept until deleted; at most 36500)}"#),
        CommandSpec::new("ai.test", "Test a model", WHERE, |s, v| {
            let id = p::req_str(v, "profile")?.to_string();
            s.ai_test(&id)?;
            Ok(json!({"testing": id}))
        })
        .params(r#"{"profile": string}"#),
        CommandSpec::new("ai.ask", "Ask MailCraft's AI about an account's mail", WHERE, ask).params(
            r#"{"account": string (whose mail the question holds: its setting decides which models may be asked), "question": string, "system"?: string, "task"?: "chat"|"decision", "purpose"?: string (recorded with it), "stream"?: bool (the answer kept as it's written: ai.answer gives what came so far)}"#,
        ),
        CommandSpec::new("ai.answer", "Take the answer to a question asked", WHERE, |s, v| {
            let id = p::u64(v, "id").ok_or_else(|| CmdError::Params("`id` (number) is required".into()))?;
            match s.ai_answer(id) {
                Some(Ok(a)) => Ok(json!({"text": a.text, "cut": a.cut, "inputTokens": a.input_tokens, "outputTokens": a.output_tokens})),
                Some(Err(e)) => Err(CmdError::Failed(e)),
                None => match s.ai.part(id) {
                    Some((text, thinking)) => Ok(json!({"waiting": true, "text": text, "thinking": thinking})),
                    None => Ok(json!({"waiting": s.ai.is_waiting(id)})),
                },
            }
        })
        .params(r#"{"id": number (from ai.ask)}"#),
        CommandSpec::new("ai.log", "Show what MailCraft's AI sent where", WHERE, |s, _| {
            let entries: Vec<_> = s.ai.log.iter().map(|(_, e)| e).collect();
            Ok(json!({"entries": entries}))
        }),
    ]
}

/// The settings as the Options page shows them: each profile, whether it's local, its key
/// kept (never the key) and its last test.
fn settings(s: &Session) -> Value {
    let ai = &s.prefs.ai;
    let profiles: Vec<Value> = ai
        .profiles
        .iter()
        .map(|pr| {
            let mut v = json!(pr);
            v["local"] = json!(pr.local());
            v["test"] = json!(s.ai.tests.get(&pr.id));
            v["keyError"] = json!(s.ai.key_errors.get(&pr.id));
            v
        })
        .collect();
    json!({"profiles": profiles, "uses": ai.uses, "accounts": ai.accounts, "localOnly": ai.local_only, "waiting": s.ai.waiting()})
}

/// `k`'s text trimmed, at most [`TEXT_MOST`] characters, and not empty.
fn text(v: &Value, k: &str) -> Result<String, CmdError> {
    let t = p::req_str(v, k)?.trim();
    if t.is_empty() {
        return Err(CmdError::Params(format!("`{k}` is empty")));
    }
    if t.chars().count() > TEXT_MOST || t.chars().any(char::is_control) {
        return Err(CmdError::Params(format!("`{k}` is longer than {TEXT_MOST} characters, or holds control characters")));
    }
    Ok(t.to_string())
}

/// A server's API address: https, or plain http to this computer alone (as the network layer
/// takes it).
fn address(v: &Value) -> Result<String, CmdError> {
    let url = text(v, "url")?;
    let lower = url.to_ascii_lowercase();
    let host = lower.split_once("://").map(|(_, r)| r.split(['/', '?', '#']).next().unwrap_or_default()).unwrap_or_default();
    let host = host.strip_prefix('[').map_or_else(|| host.rsplit_once(':').map_or(host, |(h, _)| h), |v6| v6.split(']').next().unwrap_or_default());
    let loopback = matches!(host, "localhost" | "127.0.0.1" | "::1");
    if host.is_empty() || host.contains('@') || !(lower.starts_with("https://") || (lower.starts_with("http://") && loopback)) {
        return Err(CmdError::Params("`url` must start with https://, or http:// for a server on this computer (localhost)".into()));
    }
    Ok(url.trim_end_matches('/').to_string())
}

fn save_profile(s: &mut Session, v: &Value) -> CmdResult {
    let api = match p::req_str(v, "api")? {
        "openAi" => AiApi::OpenAi,
        "anthropic" => AiApi::Anthropic,
        "ollama" => AiApi::Ollama,
        other => return Err(CmdError::Params(format!("unknown api `{}`: openAi, anthropic or ollama", other.chars().take(40).collect::<String>()))),
    };
    let endpoint = AiEndpoint { api, url: address(v)?, model: text(v, "model")? };
    let name = text(v, "name")?;
    let max_tokens = match p::u64(v, "maxTokens") {
        Some(n) => u32::try_from(n)
            .ok()
            .filter(|n| (1..=TOKENS_MOST).contains(n))
            .ok_or_else(|| CmdError::Params(format!("`maxTokens` is from 1 to {TOKENS_MOST}")))?,
        None => TOKENS_AT_FIRST,
    };
    let context = match p::u64(v, "context") {
        Some(0) => 0,
        Some(n) => u32::try_from(n)
            .ok()
            .filter(|n| (CONTEXT_LEAST..=CONTEXT_MOST).contains(n))
            .ok_or_else(|| CmdError::Params(format!("`context` is 0, or from {CONTEXT_LEAST} to {CONTEXT_MOST}")))?,
        None => u32::MAX,
    };
    let key = p::str(v, "key").map(str::trim);
    if key.is_some_and(|k| k.len() > KEY_MOST || k.chars().any(char::is_control)) {
        return Err(CmdError::Params(format!("`key` is longer than {KEY_MOST} bytes, or holds control characters")));
    }
    let ai = &s.prefs.ai;
    let old = match p::str(v, "id") {
        Some(id) => {
            Some(ai.profile(id).ok_or_else(|| CmdError::Params(format!("no model profile {}", id.chars().take(40).collect::<String>())))?.clone())
        }
        None if ai.profiles.len() >= PROFILES_MOST => return Err(CmdError::Failed(format!("MailCraft keeps {PROFILES_MOST} models at most"))),
        None => None,
    };
    let id = old.as_ref().map_or_else(|| new_id(s), |o| o.id.clone());
    let fallback = match p::str(v, "fallback") {
        Some("") => None,
        Some(f) if f == id => return Err(CmdError::Params("a profile can't fall back to itself".into())),
        Some(f) => Some(s.prefs.ai.profile(f).map(|p| p.id.clone()).ok_or_else(|| CmdError::Params("no such fallback profile".into()))?),
        None => old.as_ref().and_then(|o| o.fallback.clone()),
    };
    // A key stays with the server it was given for: a new address forgets it unless one is given.
    let moved = old.as_ref().is_some_and(|o| o.endpoint.url != endpoint.url);
    let has_key = match key {
        Some(k) => !k.is_empty(),
        None => old.as_ref().is_some_and(|o| o.has_key) && !moved,
    };
    let context = if context == u32::MAX { old.as_ref().map_or(0, |o| o.context) } else { context };
    let scripts = p::bool(v, "scripts").unwrap_or_else(|| old.as_ref().is_some_and(|o| o.scripts));
    let profile = AiProfile { id: id.clone(), name, endpoint, has_key, max_tokens, context, fallback, scripts };
    match key {
        Some(k) => s.jobs.push(Job::Ai { work: AiWork::SaveKey { name: id.clone(), key: (!k.is_empty()).then(|| Secret(k.to_string())) } }),
        None if moved => s.jobs.push(Job::Ai { work: AiWork::SaveKey { name: id.clone(), key: None } }),
        None => {}
    }
    s.ai.tests.remove(&id);
    let local = local_url(&profile.endpoint.url);
    match s.prefs.ai.profiles.iter_mut().find(|p| p.id == id) {
        Some(p) => *p = profile,
        None => s.prefs.ai.profiles.push(profile),
    }
    Ok(json!({"id": id, "local": local}))
}

/// The first `p<n>` no profile has.
fn new_id(s: &Session) -> String {
    let taken = |id: &str| s.prefs.ai.profiles.iter().any(|p| p.id == id);
    (1..).map(|n: u32| format!("p{n}")).find(|id| !taken(id)).unwrap_or_default()
}

fn remove_profile(s: &mut Session, id: &str) -> CmdResult {
    let ai = &mut s.prefs.ai;
    let before = ai.profiles.len();
    ai.profiles.retain(|p| p.id != id);
    if ai.profiles.len() == before {
        return Err(CmdError::Params(format!("no model profile {}", id.chars().take(40).collect::<String>())));
    }
    ai.uses.retain(|_, p| p != id);
    for p in &mut ai.profiles {
        if p.fallback.as_deref() == Some(id) {
            p.fallback = None;
        }
    }
    s.ai.tests.remove(id);
    s.ai.key_errors.remove(id);
    s.jobs.push(Job::Ai { work: AiWork::SaveKey { name: id.to_string(), key: None } });
    Ok(json!({"removed": id}))
}

fn use_profile(s: &mut Session, v: &Value) -> CmdResult {
    let task = AiTask::from_code(p::req_str(v, "task")?).ok_or_else(|| CmdError::Params("`task` is chat or decision".into()))?;
    match p::req_str(v, "profile")? {
        "" => {
            s.prefs.ai.uses.remove(&task);
        }
        id => {
            let id = s.prefs.ai.profile(id).map(|p| p.id.clone()).ok_or_else(|| CmdError::Params("no such model profile".into()))?;
            s.prefs.ai.uses.insert(task, id);
        }
    }
    Ok(json!({"task": task, "profile": s.prefs.ai.uses.get(&task)}))
}

fn ask(s: &mut Session, v: &Value) -> CmdResult {
    let account = p::req_str(v, "account")?;
    let Some(account) = s.accounts.iter().map(|a| &a.config.id).find(|id| id.as_str() == account).cloned() else {
        return Err(CmdError::Params("no such account".into()));
    };
    let question = p::req_str(v, "question")?;
    let system = p::str(v, "system").unwrap_or_default();
    if question.trim().is_empty() || question.len().saturating_add(system.len()) > QUESTION_MOST {
        return Err(CmdError::Params(format!("`question` is empty, or with `system` longer than {QUESTION_MOST} bytes")));
    }
    let task = match p::str(v, "task") {
        Some(t) => AiTask::from_code(t).ok_or_else(|| CmdError::Params("`task` is chat or decision".into()))?,
        None => AiTask::Chat,
    };
    let purpose: String = p::str(v, "purpose").unwrap_or("question").chars().filter(|c| !c.is_control()).take(40).collect();
    let chat = mailcraft_model::AiChat::ask(system, question, TOKENS_MOST);
    let id = s.ai_ask(task, Some(&account), &purpose, chat, p::bool(v, "stream").unwrap_or(false))?;
    Ok(json!({"id": id}))
}

fn allow(s: &mut Session, v: &Value) -> CmdResult {
    let account = p::req_str(v, "account")?;
    let Some(account) = s.accounts.iter().map(|a| &a.config.id).find(|id| id.as_str() == account).cloned() else {
        return Err(CmdError::Params("no such account".into()));
    };
    let reach = match p::req_str(v, "reach")? {
        "off" => None,
        "local" => Some(AiReach::Local),
        "any" => Some(AiReach::Any),
        _ => return Err(CmdError::Params("`reach` is off, local or any".into())),
    };
    match reach {
        Some(r) => s.prefs.ai.accounts.insert(account.clone(), r),
        None => s.prefs.ai.accounts.remove(&account),
    };
    Ok(json!({"account": account, "reach": reach}))
}

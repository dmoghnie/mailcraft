//! A script's questions to the model: `ask(question)` or `ask(question, text)`, for what code
//! can't judge (does this message ask for a quote?). The chat's own model is asked, which may
//! read every account whose mail the chat holds, as a question of its own with nothing else of
//! the chat; the script waits for its answer as for a message to download.

use mailcraft_model::AiChat;

use super::*;

/// The function's name, as a script calls it.
pub(in super::super) const ASK: &str = "ask";
/// The most questions a script asks: each takes the model's time.
const ASKS_MOST: usize = 20;
/// The longest question, in characters.
const QUESTION_MOST: usize = 2_000;
/// The most of a text asked about, in characters, when the model takes in more.
const TEXT_MOST: usize = 60_000;
/// What the record of what went where says a script's question was for.
const PURPOSE: &str = "assistant script";
/// What the model asked is told.
const SYSTEM: &str = "A script MailCraft's assistant wrote asks you a question, about the text it gives when it gives one. Answer briefly and plainly: only the answer, with nothing before or after it.";

impl Session {
    /// `ask` from `script`, as call `made`: the chat's model asked, the script waiting for its
    /// answer; or why it can't be asked. Its run shows under the script's card.
    pub(super) fn script_ask(&mut self, chat: &mut Chat, profile: &AiProfile, script: &Script, made: AiToolCall) -> Result<Answer, Wait> {
        let parent = script.call.id.clone();
        let asked = chat.runs.iter().filter(|r| r.part_of.as_deref() == Some(parent.as_str()) && r.tool == ASK).count();
        let question = made.input.get("question").and_then(Value::as_str).map(str::trim).unwrap_or_default();
        let text = made.input.get("text").and_then(Value::as_str).unwrap_or_default();
        let what = format!("Ask {}: {}", profile.name, question.chars().take(80).collect::<String>());
        let refused = if asked >= ASKS_MOST {
            Some(format!("a script asks at most {ASKS_MOST} questions"))
        } else if question.is_empty() {
            Some("ask(question) takes the question to ask".to_string())
        } else {
            None
        };
        let (state, result, wait) = match refused {
            Some(why) => (RunState::Failed, why, None),
            None => {
                let q = self.script_question(chat, profile, question, text);
                self.ai.assistant.script_asks.insert(q, (chat.id.clone(), parent.clone()));
                (RunState::Waiting, String::new(), Some(q))
            }
        };
        let run = ToolRun {
            call: made.id,
            tool: ASK.into(),
            state,
            what,
            messages: Vec::new(),
            found: Vec::new(),
            result: result.clone(),
            part_of: Some(parent),
            input: Value::Null,
        };
        match chat.run_mut(&run.call) {
            Some(r) => *r = run,
            None => chat.runs.push(run),
        }
        match wait {
            Some(q) => Err(Wait::Answer(q)),
            None => Ok(Answer::Failed(result)),
        }
    }

    /// Ask `profile` `question` about `text`, cut to what it takes in beside its answer: the
    /// question's id.
    fn script_question(&mut self, chat: &Chat, profile: &AiProfile, question: &str, text: &str) -> u64 {
        let question = cut(question, QUESTION_MOST);
        let answer = profile.max_tokens.max(1);
        // A token for every three bytes, and a character takes up to four.
        let room = usize::try_from(profile.takes_in().saturating_sub(answer)).unwrap_or(0).saturating_mul(3) / 4;
        let room = room.saturating_sub(SYSTEM.len().saturating_add(question.len()));
        let text = cut(text, room.min(TEXT_MOST));
        let said = if text.trim().is_empty() { question } else { format!("{question}\n\nThe text:\n{text}") };
        self.ai_ask_profile(profile, chat.accounts.clone(), PURPOSE, AiChat::ask(SYSTEM, &said, answer), false)
    }
}

//! Asking a language model, for MailCraft's own AI: which server and model, what is asked, and
//! what came back. The engine words the question; `net` sends it.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Secret;

/// The wire format a model's server speaks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AiApi {
    /// OpenAI's Chat Completions, which most servers speak: OpenAI, xAI, Mistral, OpenRouter,
    /// Groq, DeepSeek, Google Gemini's compatible address, and the ones on a computer of the
    /// person's own (Ollama, LM Studio, llama.cpp's server).
    #[default]
    OpenAi,
    /// Anthropic's Messages.
    Anthropic,
}

/// A model on a server: what a profile names.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiEndpoint {
    pub api: AiApi,
    /// The server's API address, up to its version: `https://api.openai.com/v1`,
    /// `http://localhost:11434/v1`.
    pub url: String,
    pub model: String,
}

/// Who said a turn of a conversation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AiRole {
    User,
    Assistant,
}

/// One turn of a conversation with a model: what was said, the tools the model called in it (an
/// assistant's), and what they gave back (a user's, answering the turn before). Its `Debug`
/// form gives only its size: it holds mail.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiTurn {
    pub role: AiRole,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<AiToolCall>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub results: Vec<AiToolResult>,
}

impl AiTurn {
    pub fn user(text: &str) -> AiTurn {
        AiTurn { role: AiRole::User, text: text.into(), calls: Vec::new(), results: Vec::new() }
    }

    pub fn assistant(text: &str) -> AiTurn {
        AiTurn { role: AiRole::Assistant, text: text.into(), calls: Vec::new(), results: Vec::new() }
    }

    /// Its bytes, as the record counts them.
    pub fn size(&self) -> usize {
        let calls = self.calls.iter().map(AiToolCall::size).sum::<usize>();
        let results = self.results.iter().map(|r| r.content.len()).sum::<usize>();
        self.text.len().saturating_add(calls).saturating_add(results)
    }
}

impl fmt::Debug for AiTurn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AiTurn({:?}, <{} bytes>, {} calls, {} results)", self.role, self.size(), self.calls.len(), self.results.len())
    }
}

/// A tool a model may call: its name, what it does, and the JSON Schema of what it takes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiTool {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

/// A model's call of a tool: the id its result answers, the tool, and what it was given (a JSON
/// object; a model's arguments that weren't JSON come as the text it sent). Its `Debug` form
/// gives only its size: what a model passes holds mail.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

impl AiToolCall {
    pub fn size(&self) -> usize {
        self.name.len().saturating_add(self.input.to_string().len())
    }
}

impl fmt::Debug for AiToolCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AiToolCall({}, <{} bytes>)", self.name, self.size())
    }
}

/// What a tool gave back, for the call `id`: its text, and whether it failed. Its `Debug` form
/// gives only its size.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiToolResult {
    pub id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub error: bool,
}

impl fmt::Debug for AiToolResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AiToolResult(<{} bytes>, error {})", self.content.len(), self.error)
    }
}

/// What a model is asked: its instructions, the conversation so far, how long an answer may be,
/// and the tools it may call. Its `Debug` form gives only its size: it holds mail.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiChat {
    pub system: String,
    pub turns: Vec<AiTurn>,
    /// The longest answer, in tokens.
    pub max_tokens: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<AiTool>,
}

impl AiChat {
    /// One question, with its instructions.
    pub fn ask(system: &str, question: &str, max_tokens: u32) -> AiChat {
        AiChat { system: system.into(), turns: vec![AiTurn::user(question)], max_tokens, tools: Vec::new() }
    }

    /// Its bytes, as the record counts them: the instructions and every turn.
    pub fn size(&self) -> usize {
        self.system.len().saturating_add(self.turns.iter().map(AiTurn::size).sum::<usize>())
    }
}

impl fmt::Debug for AiChat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AiChat(<{} turns, {} bytes>, max_tokens {}, {} tools)", self.turns.len(), self.size(), self.max_tokens, self.tools.len())
    }
}

/// A model's answer: what it said, and the tools it called. Its `Debug` form gives only its size.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiAnswer {
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<AiToolCall>,
    /// What it thought before it answered, when the server sends it apart (a model that
    /// reasons: Qwen, DeepSeek, gpt-oss through Ollama).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub thinking: String,
    /// Cut off at the chat's `max_tokens`.
    pub cut: bool,
    /// Bytes sent and received, for the record of what went where.
    pub sent: u64,
    pub received: u64,
    /// Tokens read and written, as the server counted them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
}

impl fmt::Debug for AiAnswer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "AiAnswer(<{} bytes>, {} calls, <{} bytes thought>, cut {}, sent {}, received {})",
            self.text.len(),
            self.calls.len(),
            self.thinking.len(),
            self.cut,
            self.sent,
            self.received
        )
    }
}

/// Work for the network layer's models.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "work", rename_all = "camelCase")]
pub enum AiWork {
    /// Ask a model, with the key kept in the keychain under `key` (none: a server that takes
    /// none); answered by [`AiDone::Answered`] with the same `id`. Streamed, the answer comes as
    /// it's written too, in [`AiDone::Part`]s before it.
    Ask {
        id: u64,
        endpoint: AiEndpoint,
        key: Option<String>,
        chat: AiChat,
        #[serde(default)]
        stream: bool,
    },
    /// Keep a key in the keychain under `name`, or forget it (`None`); answered by
    /// [`AiDone::KeySaved`].
    SaveKey { name: String, key: Option<Secret> },
}

/// What the network layer's models did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "done", rename_all = "camelCase")]
pub enum AiDone {
    /// The answer to [`AiWork::Ask`] `id`, or why there's none.
    Answered { id: u64, answer: Option<AiAnswer>, error: Option<String> },
    /// What more of a streamed answer `id` came: its text and its thinking since the last part.
    Part { id: u64, text: String, thinking: String },
    /// A key kept or forgotten, or why it couldn't be.
    KeySaved { name: String, error: Option<String> },
}

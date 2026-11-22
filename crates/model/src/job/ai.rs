//! Asking a language model, for MailCraft's own AI: which server and model, what is asked, and
//! what came back. The engine words the question; `net` sends it.

use std::fmt;

use serde::{Deserialize, Serialize};

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

/// One turn of a conversation with a model. Its `Debug` form gives only its size: it holds mail.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiTurn {
    pub role: AiRole,
    pub text: String,
}

impl fmt::Debug for AiTurn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AiTurn({:?}, <{} bytes>)", self.role, self.text.len())
    }
}

/// What a model is asked: its instructions, the conversation so far, and how long an answer may
/// be. Its `Debug` form gives only its size: it holds mail.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiChat {
    pub system: String,
    pub turns: Vec<AiTurn>,
    /// The longest answer, in tokens.
    pub max_tokens: u32,
}

impl AiChat {
    /// One question, with its instructions.
    pub fn ask(system: &str, question: &str, max_tokens: u32) -> AiChat {
        AiChat { system: system.into(), turns: vec![AiTurn { role: AiRole::User, text: question.into() }], max_tokens }
    }
}

impl fmt::Debug for AiChat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.system.len().saturating_add(self.turns.iter().map(|t| t.text.len()).sum::<usize>());
        write!(f, "AiChat(<{} turns, {bytes} bytes>, max_tokens {})", self.turns.len(), self.max_tokens)
    }
}

/// A model's answer. Its `Debug` form gives only its size.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiAnswer {
    pub text: String,
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
        write!(f, "AiAnswer(<{} bytes>, cut {}, sent {}, received {})", self.text.len(), self.cut, self.sent, self.received)
    }
}

/// Work for the network layer's models.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "work", rename_all = "camelCase")]
pub enum AiWork {
    /// Ask a model, with the key kept in the keychain under `key` (none: a server that takes
    /// none); answered by [`AiDone::Answered`] with the same `id`.
    Ask { id: u64, endpoint: AiEndpoint, key: Option<String>, chat: AiChat },
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
    /// A key kept or forgotten, or why it couldn't be.
    KeySaved { name: String, error: Option<String> },
}

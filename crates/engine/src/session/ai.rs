//! MailCraft's own AI: the language models the person sets up, local or a cloud provider's, and
//! the questions the engine asks them. Off until the person adds a model and lets an account's
//! mail go to it; a test sends no mail; what went where is recorded (sizes, never content). The
//! engine words each question and queues it as a [`mailcraft_model::Job::Ai`]; `net` asks.

mod asking;
mod assistant;
mod commands;
mod settings;

pub use asking::{AI_LOG_KEPT, AiLogEntry, AiState, AiTest, server_of as ai_server};
pub(crate) use assistant::assistant_commands;
pub use assistant::{
    ASSISTANT_WIDTH, Assistant, AssistantPane, CONTEXT_AT_FIRST, CONTEXT_ITEMS_MOST, CONTEXT_QUOTE_MOST, Chat, ChatHead, ChatTurn, ContextItem,
    RunState, STEPS_MOST, ToolRun,
};
pub(crate) use commands::commands as ai_commands;
pub use settings::{AiPrefs, AiProfile, AiReach, AiTask, local_url as ai_url_is_local};

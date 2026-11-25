//! MailCraft's own AI: the language models the person sets up, local or a cloud provider's, and
//! the questions the engine asks them. Off until the person adds a model and lets an account's
//! mail go to it; a test sends no mail; what went where is recorded (sizes, never content). The
//! engine words each question and queues it as a [`mailcraft_model::Job::Ai`]; `net` asks.

mod about;
mod asking;
mod assistant;
mod budget;
mod commands;
mod digest;
mod drafting;
mod needs_me;
mod rewriting;
mod rule_words;
mod scheduling;
mod servers;
mod settings;
mod style;

pub(crate) use about::commands as about_commands;
pub use about::{AboutKind, AboutMessage, FactAmount, FactEvent, FactTask, FactTracking, Facts, ai_reads_file, facts_from};
pub use asking::{AI_LOG_KEPT, AiLogEntry, AiState, AiTest, server_of as ai_server};
pub(crate) use assistant::assistant_commands;
#[doc(hidden)]
pub use assistant::run_script_against;
pub use assistant::{
    ASSISTANT_LINK, ASSISTANT_WIDTH, Assistant, AssistantPane, CONTEXT_AT_FIRST, CONTEXT_ITEMS_MOST, CONTEXT_QUOTE_MOST, Chat, ChatHead, ChatTurn,
    ContextItem, KeptScript, RunState, STEPS_MOST, ScriptParam, ToolRun, assistant_message_link,
};
pub(crate) use commands::commands as ai_commands;
pub(crate) use digest::commands as digest_commands;
pub use digest::{Digest, DigestWhen};
pub(crate) use drafting::commands as drafting_commands;
pub(crate) use needs_me::commands as needs_me_commands;
pub(crate) use rewriting::commands as rewriting_commands;
pub use rewriting::{Rewrite, RewriteHow};
pub use rule_words::RuleProposal;
pub(crate) use rule_words::commands as rule_words_commands;
pub(crate) use servers::commands as mcp_server_commands;
pub use servers::{McpServer, ServerTools};
pub use settings::{AiPrefs, AiProfile, AiReach, AiTask, local_url as ai_url_is_local};
pub use style::STYLE_MOST;
pub(crate) use style::commands as style_commands;

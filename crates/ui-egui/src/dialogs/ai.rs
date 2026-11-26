//! File › Options › AI's dialogs: a model added or changed, a script MailCraft's AI keeps, an
//! MCP server, an automation, and the suggestions it made for arriving mail.

use super::*;

mod automation;
mod model;
mod script;

pub use automation::AutomationForm;
pub(super) use automation::draw as draw_automation;
pub(crate) use automation::{A_MONDAY, time_shown};

pub use model::AiModelForm;
pub(super) use model::draw as draw_model;
pub use script::ScriptForm;
pub(super) use script::draw as draw_script;
mod server;
pub use server::ServerForm;
pub(super) use server::draw as draw_server;
mod suggestions;
pub use suggestions::SuggestionsForm;
pub(super) use suggestions::draw as draw_suggestions;
pub(crate) use suggestions::suggestion_said;

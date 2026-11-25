//! File › Options › AI's dialogs: a model added or changed, and a script MailCraft's AI keeps.

use super::*;

mod model;
mod script;

pub use model::AiModelForm;
pub(super) use model::draw as draw_model;
pub use script::ScriptForm;
pub(super) use script::draw as draw_script;

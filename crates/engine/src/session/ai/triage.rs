//! Mail arriving in an Inbox sorted by MailCraft's AI as it comes: whether it needs the person
//! (`needs_me`), and what it suggests doing with it (`suggestions`).

mod needs_me;
mod suggestions;

pub(crate) use needs_me::{NeedsMe, commands as needs_me_commands};
pub use suggestions::TriageSuggestion;
pub(crate) use suggestions::{Triage, commands as triage_commands};

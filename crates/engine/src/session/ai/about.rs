//! What MailCraft's AI writes about a message for the reading pane: its conversation summed up
//! (`summary.rs`).

use super::{ai_server, assistant, settings};

mod summary;

pub(super) use summary::Summaries;
pub(crate) use summary::commands;
pub use summary::{SUMMARY_PURPOSE, ThreadSummary};

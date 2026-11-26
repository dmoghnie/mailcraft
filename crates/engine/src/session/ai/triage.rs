//! Mail arriving in an Inbox sorted by MailCraft's AI as it comes: whether it needs the person
//! (`needs_me`).

mod needs_me;

pub(crate) use needs_me::{NeedsMe, commands as needs_me_commands};

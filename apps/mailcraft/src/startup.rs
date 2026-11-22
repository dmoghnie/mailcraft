//! What a start works out before anything opens: what MailCraft was started with, whether it's
//! portable, whether another MailCraft already runs on the same data, and the log
//! (`mailcraft_net::logfile`, shared with the phones).

pub(super) mod args;
pub(super) use mailcraft_net::logfile;
pub(super) mod portable;
pub(super) mod single_instance;

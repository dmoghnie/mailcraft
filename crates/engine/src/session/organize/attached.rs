//! The Files view: every file mail brought or took, that this computer holds, newest first,
//! across accounts. It's built from the messages kept here (nothing is downloaded for it): a
//! folder's list is read a step at a time ([`Session::files_step`], a step a frame while the view
//! is open, as the To-Do Bar reads its flagged mail), then the messages with files, newest
//! first, each read for its files ([`mailcraft_mime::message_files`]: the pictures a message's
//! text shows aren't files). A message in two folders (Gmail's labels) is listed once.
//!
//! What the view shows is filtered by kind (pictures, documents, PDFs, archives), by person
//! (who sent it, or for sent mail who it went to) and by name ([`FilesFilter`]).

use std::collections::{BTreeSet, HashSet};

use mailcraft_mime::{AttachmentInfo, AttachmentKind};
use mailcraft_model::{FolderRole, MessageKey, SharedSummary};

use super::*;

/// The most messages with files read for the list: the newest this many.
pub const FILES_MESSAGES_MOST: usize = 5000;
/// The most files listed.
pub const FILES_MOST: usize = 20_000;
/// Messages read for their files in one step, and the most of their sources read in one (a
/// step is a frame: a few big files are a step of their own, the window not held up).
const READ_PER_STEP: usize = 20;
const STEP_BYTES: usize = 4 << 20;

/// What a file is, as the Files view sorts them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileKind {
    Picture,
    Document,
    Pdf,
    Archive,
    Other,
}

impl FileKind {
    /// The kinds the view offers to filter by, in its order.
    pub const ALL: [FileKind; 4] = [FileKind::Picture, FileKind::Document, FileKind::Pdf, FileKind::Archive];

    pub fn code(self) -> &'static str {
        match self {
            FileKind::Picture => "picture",
            FileKind::Document => "document",
            FileKind::Pdf => "pdf",
            FileKind::Archive => "archive",
            FileKind::Other => "other",
        }
    }

    pub fn from_code(code: &str) -> Option<FileKind> {
        [FileKind::Picture, FileKind::Document, FileKind::Pdf, FileKind::Archive, FileKind::Other].into_iter().find(|k| k.code() == code)
    }

    /// Its heading in the view (the interface text is looked up from it).
    pub fn label(self) -> &'static str {
        match self {
            FileKind::Picture => "Pictures",
            FileKind::Document => "Documents",
            FileKind::Pdf => "PDFs",
            FileKind::Archive => "Archives",
            FileKind::Other => "Other files",
        }
    }

    /// A file's kind, as the reading pane would preview it: text is a document.
    pub fn of(a: &AttachmentInfo) -> FileKind {
        match a.kind() {
            AttachmentKind::Image => FileKind::Picture,
            AttachmentKind::Pdf => FileKind::Pdf,
            AttachmentKind::Archive => FileKind::Archive,
            AttachmentKind::Document | AttachmentKind::Text => FileKind::Document,
            AttachmentKind::Video | AttachmentKind::Audio | AttachmentKind::Other => FileKind::Other,
        }
    }
}

/// A file of a message kept here.
#[derive(Clone, Debug, PartialEq)]
pub struct AttachedFile {
    /// The message it's in.
    pub key: MessageKey,
    /// Its place among the message's attachments (`attachment.save`'s `index`).
    pub index: usize,
    pub name: String,
    pub mime: String,
    pub size: usize,
    pub kind: FileKind,
    /// Who it came from; for mail sent, who it went to (the first).
    pub person: mailcraft_model::EmailAddress,
    /// Sent from this computer's accounts (in a Sent folder).
    pub sent: bool,
    pub date: i64,
    pub subject: String,
}

/// What the Files view shows of the files: all of them, or those of one kind, a person's, or
/// those whose name has some words.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilesFilter {
    pub kind: Option<FileKind>,
    /// Part of a name or an address, any case.
    pub person: String,
    /// Part of the file's name, any case.
    pub name: String,
}

impl FilesFilter {
    fn keeps(&self, f: &AttachedFile) -> bool {
        let has = |text: &str, part: &str| part.is_empty() || text.to_lowercase().contains(&part.trim().to_lowercase());
        self.kind.is_none_or(|k| k == f.kind) && (has(&f.person.name, &self.person) || has(&f.person.email, &self.person)) && has(&f.name, &self.name)
    }
}

/// The files found so far, and what's still to be read for them.
#[derive(Debug, Default)]
pub struct FilesBook {
    /// What's shown of them.
    pub filter: FilesFilter,
    begun: bool,
    /// Folders not read yet: whether it isn't an Inbox, the account and the path. An Inbox is
    /// read first, so a message in two folders is listed as its Inbox's (Gmail's All Mail and
    /// labels hold it too).
    folders: BTreeSet<(bool, String, String)>,
    /// Messages with files, not read yet, with whether each is in a Sent folder: sorted
    /// newest last (taken from the end) once every folder is read.
    waiting: Vec<(SharedSummary, bool)>,
    sorted: bool,
    /// Each message listed or waiting, by account and Message-ID: a copy in another folder is
    /// skipped.
    seen: HashSet<(String, String)>,
    read: usize,
    found: Vec<AttachedFile>,
}

impl Session {
    /// The Files view's list started again (it's read again a step at a time).
    pub fn files_restart(&mut self) {
        let filter = std::mem::take(&mut self.files.filter);
        self.files = FilesBook { filter, ..FilesBook::default() };
    }

    /// One step of reading the files: a folder's list, or a few messages; whether more waits.
    pub fn files_step(&mut self) -> bool {
        if !self.files.begun {
            self.files.begun = true;
            let folders = self.accounts.iter().flat_map(|a| {
                a.folders
                    .iter()
                    .filter(|f| f.selectable && !matches!(f.look(), FolderRole::Trash | FolderRole::Junk | FolderRole::Drafts))
                    .map(|f| (f.look() != FolderRole::Inbox, a.config.id.clone(), f.path.clone()))
            });
            self.files.folders = folders.collect();
        }
        if let Some((_, account, path)) = self.files.folders.pop_first() {
            self.files_folder(&account, &path);
            return true;
        }
        if !self.files.sorted {
            self.files.sorted = true;
            self.files.waiting.sort_by_key(|(m, _)| (m.date, m.key.uid));
        }
        let mut bytes = 0usize;
        for _ in 0..READ_PER_STEP {
            if self.files.read >= FILES_MESSAGES_MOST || self.files.found.len() >= FILES_MOST {
                self.files.waiting.clear();
            }
            let Some((m, sent)) = self.files.waiting.pop() else { return false };
            bytes = bytes.saturating_add(self.files_read(&m, sent));
            if bytes >= STEP_BYTES {
                break;
            }
        }
        !self.files.waiting.is_empty()
    }

    /// Whether every file there is to find has been read.
    pub fn files_done(&self) -> bool {
        self.files.begun && self.files.folders.is_empty() && self.files.waiting.is_empty()
    }

    /// The files as the filter shows them, newest first.
    pub fn files_shown(&self) -> Vec<&AttachedFile> {
        self.files_matching(&self.files.filter)
    }

    /// The files `filter` keeps, newest first.
    pub fn files_matching(&self, filter: &FilesFilter) -> Vec<&AttachedFile> {
        self.files.found.iter().filter(|f| filter.keeps(f)).collect()
    }

    /// The messages with files of a folder, put with the others waiting to be read.
    fn files_folder(&mut self, account: &str, path: &str) {
        let sent = self.account(account).and_then(|a| a.folders.iter().find(|f| f.path == path)).is_some_and(|f| f.look() == FolderRole::Sent);
        let Some(Ok(st)) = self.store.as_ref().map(|s| s.load_folder(account, path)) else { return };
        for m in st.messages.into_iter().filter(|m| m.has_attachments) {
            let id = if m.message_id.is_empty() { format!("{}:{}", m.key.folder, m.key.uid) } else { m.message_id.clone() };
            if self.files.seen.insert((account.to_string(), id)) {
                self.files.waiting.push((m, sent));
            }
        }
    }

    /// A message's files, when it's kept here, listed (it's the newest not read yet); how big
    /// its source was.
    fn files_read(&mut self, m: &SharedSummary, sent: bool) -> usize {
        let Some(raw) = self.readable_raw(&m.key) else { return 0 };
        self.files.read += 1;
        let person = if sent { m.to.first().cloned().unwrap_or_default() } else { m.from.clone() };
        for a in mailcraft_mime::message_files(&raw) {
            self.files.found.push(AttachedFile {
                key: m.key.clone(),
                index: a.index,
                kind: FileKind::of(&a),
                name: a.name,
                mime: a.mime,
                size: a.size,
                person: person.clone(),
                sent,
                date: m.date,
                subject: m.subject.clone(),
            });
        }
        raw.len()
    }

    /// The message a file is in, shown in the reading pane for the file's preview: not marked
    /// read (it isn't being read).
    pub fn show_file_message(&mut self, key: &MessageKey) {
        self.show_message(key, false);
    }

    /// A file of a message kept here: its bytes (`attachment.save` with a `key`).
    pub fn file_bytes(&self, key: &MessageKey, index: usize) -> Result<Vec<u8>, CmdError> {
        let raw = self.readable_raw(key).ok_or_else(|| CmdError::Failed("the message isn't downloaded".into()))?;
        mailcraft_mime::attachment_data(&raw, index).ok_or_else(|| CmdError::Params(format!("no attachment {index}")))
    }
}

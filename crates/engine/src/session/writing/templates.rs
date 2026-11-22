//! Message templates, as Outlook's and Airmail's: a message's subject and formatted text with its
//! pictures, and when asked its recipients and files, kept by name in the data folder
//! (`templates.json`, backed up with the rest) to start a message from or fill one with.

use super::signatures::quote_line;
use super::*;

/// Where they're kept, in the data folder.
pub(crate) const TEMPLATES_DOC: &str = "templates.json";
/// The most templates kept.
pub const TEMPLATES_MOST: usize = 200;
/// The longest name a template has.
pub const TEMPLATE_NAME_MOST: usize = 80;
/// The most a template's pictures and files take, all together.
pub const TEMPLATE_BYTES_MOST: usize = 20 << 20;

/// A message kept to start others from.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Template {
    pub name: String,
    pub subject: String,
    /// The text, without a signature or the quote of a reply, and its formatting.
    pub body: String,
    pub styles: mime::rich::Styles,
    /// The pictures in the text, by the `cid:` its formatting gives them.
    pub pictures: Vec<TemplateFile>,
    /// Who it's to, when kept (else empty).
    pub to: String,
    pub cc: String,
    pub bcc: String,
    /// Its files, when kept.
    pub files: Vec<TemplateFile>,
}

/// A picture or file of a template, kept as base64.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TemplateFile {
    /// A picture's `Content-ID` (empty for a file).
    pub cid: String,
    pub name: String,
    pub mime: String,
    #[serde(with = "base64_bytes")]
    pub data: Vec<u8>,
}

/// A file's bytes as base64 in the templates' file: read back leniently, one that isn't base64
/// (the file edited by hand) coming back empty, and left out, rather than losing every template.
mod base64_bytes {
    pub(super) fn serialize<S: serde::Serializer>(data: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&mailcraft_model::base64::encode(data))
    }

    pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(d)?;
        Ok(mailcraft_mime::decode_base64(&text).unwrap_or_default())
    }
}

/// A template's name as given: trimmed, not empty, not too long.
fn template_name(name: &str) -> Result<String, CmdError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CmdError::Params("a template needs a name".into()));
    }
    if name.chars().count() > TEMPLATE_NAME_MOST {
        return Err(CmdError::Params(format!("a template's name is {TEMPLATE_NAME_MOST} characters at most")));
    }
    Ok(name.to_string())
}

/// What of the message being written a template keeps: its text above a reply's quote, less the
/// signature (each message gets its own), the pictures that text shows, and (when asked) its
/// recipients and files.
fn template_of(c: &Compose, recipients: bool, files: bool) -> Template {
    let chars = |b: usize| c.body.get(..b).map_or(0, |s| s.chars().count());
    let quote = quote_line(c).unwrap_or_else(|| c.body.chars().count());
    let (mut body, mut styles) = mime::rich::slice(&c.body, &c.styles, 0..quote);
    if let Some(at) = c.signature_at() {
        let (start, end) = (chars(at), chars(at.saturating_add(c.signature.len())));
        if end <= quote {
            mime::rich::splice(&mut body, &mut styles, start..end, "", &mime::rich::Styles::plain(""));
        }
    }
    // Without the blank lines and the `-- ` line the signature leaves at its end.
    let kept = body.trim_end_matches(['\n', ' ']).trim_end_matches("--").trim_end_matches(['\n', ' ']).chars().count();
    let (body, styles) = mime::rich::slice(&body, &styles, 0..kept);
    let shown: Vec<&str> = styles.chars.iter().filter_map(|&i| styles.palette.get(usize::from(i))?.picture.as_deref()).collect();
    let pictures = c
        .inline
        .iter()
        .filter(|p| shown.contains(&p.cid.as_str()))
        .map(|p| TemplateFile { cid: p.cid.clone(), name: p.name.clone(), mime: p.mime.clone(), data: p.data.clone() })
        .collect();
    let (to, cc, bcc) = if recipients { (c.to.clone(), c.cc.clone(), c.bcc.clone()) } else { Default::default() };
    let files = if files {
        c.attachments.iter().map(|a| TemplateFile { cid: String::new(), name: a.name.clone(), mime: a.mime.clone(), data: a.data.clone() }).collect()
    } else {
        Vec::new()
    };
    Template { name: String::new(), subject: c.subject.clone(), body, styles, pictures, to, cc, bcc, files }
}

/// `add`'s addresses after those in `field`, those not there already; after its text as typed
/// when that isn't addresses yet (it's kept as it is).
fn with_recipients(field: &str, add: &str) -> String {
    let add = add.trim();
    if add.is_empty() {
        return field.to_string();
    }
    if field.trim().is_empty() {
        return add.to_string();
    }
    match (mailcraft_model::addr::parse_list(field), mailcraft_model::addr::parse_list(add)) {
        (Ok(have), Ok(new)) => {
            let new = new.into_iter().filter(|a| !have.iter().any(|h| h.email.eq_ignore_ascii_case(&a.email)));
            mailcraft_model::addr::format_field(&have.iter().cloned().chain(new).collect::<Vec<_>>())
        }
        _ => format!("{}; {add}", field.trim_end().trim_end_matches(';')),
    }
}

/// The templates, read from their file when the session opens.
#[derive(Default)]
pub(crate) struct Kept {
    all: Vec<Template>,
    /// Their file couldn't be read: nothing is written over it.
    unread: bool,
}

impl Kept {
    pub(crate) fn load(store: &Store) -> Self {
        match store.load_doc::<Vec<Template>>(TEMPLATES_DOC) {
            Ok(mut all) => {
                all.truncate(TEMPLATES_MOST);
                for t in &mut all {
                    t.pictures.retain(|f| !f.data.is_empty());
                    t.files.retain(|f| !f.data.is_empty());
                }
                Self { all, unread: false }
            }
            Err(e) => {
                log::warn!("the templates couldn't be read: {e}");
                Self { all: Vec::new(), unread: true }
            }
        }
    }
}

impl Session {
    /// The templates kept, in the order they were made.
    pub fn templates(&self) -> &[Template] {
        &self.templates.all
    }

    /// `all` kept as the templates, in their file.
    fn keep_templates(&mut self, all: Vec<Template>) -> Result<(), CmdError> {
        if self.templates.unread {
            return Err(CmdError::Failed("the templates couldn't be read: changes to them aren't saved".into()));
        }
        let store = self.store.as_ref().ok_or_else(|| CmdError::Disabled("templates are kept in a data folder, and there's none".into()))?;
        store.save_doc(TEMPLATES_DOC, &all).map_err(|e| CmdError::Failed(format!("the template couldn't be saved: {e}")))?;
        self.templates.all = all;
        Ok(())
    }

    /// The template called `name` (in any case).
    pub fn template(&self, name: &str) -> Option<&Template> {
        let name = name.trim();
        self.templates.all.iter().find(|t| t.name.eq_ignore_ascii_case(name))
    }

    /// Keep the message being written as the template `name` (one of that name is replaced): its
    /// recipients and files too when asked.
    pub fn save_template(&mut self, name: &str, recipients: bool, files: bool) -> Result<Template, CmdError> {
        let name = template_name(name)?;
        let mut t = template_of(self.composing()?, recipients, files);
        t.name = name;
        let bytes: usize = t.pictures.iter().chain(&t.files).map(|f| f.data.len()).sum();
        if bytes > TEMPLATE_BYTES_MOST {
            return Err(CmdError::Params(format!("a template's pictures and files take {} MB at most", TEMPLATE_BYTES_MOST >> 20)));
        }
        if t.body.trim().is_empty() && t.subject.trim().is_empty() && t.files.is_empty() {
            return Err(CmdError::Params("there's nothing in the message to keep".into()));
        }
        let mut all = self.templates.all.clone();
        match all.iter().position(|o| o.name.eq_ignore_ascii_case(&t.name)) {
            Some(i) => {
                if let Some(same) = all.get_mut(i) {
                    *same = t.clone();
                }
            }
            None if all.len() >= TEMPLATES_MOST => return Err(CmdError::Params(format!("{TEMPLATES_MOST} templates at most: delete one first"))),
            None => all.push(t.clone()),
        }
        self.keep_templates(all)?;
        Ok(t)
    }

    /// The template `name` called `to` instead.
    pub fn rename_template(&mut self, name: &str, to: &str) -> Result<(), CmdError> {
        let to = template_name(to)?;
        let mut all = self.templates.all.clone();
        let i = all
            .iter()
            .position(|t| t.name.eq_ignore_ascii_case(name.trim()))
            .ok_or_else(|| CmdError::Params(format!("no template called {name}")))?;
        if all.iter().enumerate().any(|(j, t)| j != i && t.name.eq_ignore_ascii_case(&to)) {
            return Err(CmdError::Params(format!("there's a template called {to} already")));
        }
        if let Some(t) = all.get_mut(i) {
            t.name = to;
        }
        self.keep_templates(all)
    }

    /// The template `name`, gone.
    pub fn delete_template(&mut self, name: &str) -> Result<(), CmdError> {
        let mut all = self.templates.all.clone();
        let before = all.len();
        all.retain(|t| !t.name.eq_ignore_ascii_case(name.trim()));
        if all.len() == before {
            return Err(CmdError::Params(format!("no template called {name}")));
        }
        self.keep_templates(all)
    }

    /// The template `name` into the message being written: its text at character `at` (by
    /// default the start, above a reply's quote and the signature), on lines of its own; its
    /// subject when the message has none; its recipients after those there, its pictures and
    /// files with the message's. Where its text went.
    pub fn insert_template(&mut self, name: &str, at: Option<usize>) -> Result<std::ops::Range<usize>, CmdError> {
        let t = self.template(name).cloned().ok_or_else(|| CmdError::Params(format!("no template called {name}")))?;
        let c = self.composing_mut()?;
        if c.sending {
            return Err(CmdError::Disabled("the message is being sent".into()));
        }
        c.styles.fit(&c.body);
        let n = c.body.chars().count();
        let at = at.unwrap_or(0).min(n);
        let mut text = t.body.clone();
        let mut styles = t.styles.clone();
        styles.fit(&text);
        // On lines of its own: after a line break, and with one after it unless one follows.
        if at > 0 && c.body.chars().nth(at - 1) != Some('\n') {
            mime::rich::splice(&mut text, &mut styles, 0..0, "\n", &mime::rich::Styles::plain("\n"));
        }
        if !text.is_empty() && c.body.chars().nth(at).is_some_and(|ch| ch != '\n') {
            mime::rich::append(&mut text, &mut styles, "\n", &mime::rich::Styles::plain("\n"));
        }
        let added = text.chars().count();
        // A reply's quote at the very start keeps its rule on its own first line.
        let quote_first = at == 0 && added > 0 && quote_line(c) == Some(0);
        mime::rich::splice(&mut c.body, &mut c.styles, at..at, &text, &styles);
        if quote_first {
            let line = text.matches('\n').count();
            c.styles.paras.iter_mut().take(line).for_each(|p| p.rule = false);
            if let Some(p) = c.styles.paras.get_mut(line) {
                p.rule = true;
            }
        }
        if c.subject.trim().is_empty() {
            c.subject = t.subject.clone();
        }
        c.to = with_recipients(&c.to, &t.to);
        c.cc = with_recipients(&c.cc, &t.cc);
        c.bcc = with_recipients(&c.bcc, &t.bcc);
        if !c.bcc.is_empty() {
            c.show_bcc = true;
        }
        for p in t.pictures {
            if !c.inline.iter().any(|q| q.cid == p.cid) {
                c.inline.push(mime::InlinePicture { cid: p.cid, name: p.name, mime: p.mime, data: p.data });
            }
        }
        for f in t.files {
            if !c.attachments.iter().any(|a| a.name == f.name && a.data.len() == f.data.len()) {
                c.attachments.push(OutAttachment { name: f.name, mime: f.mime, data: f.data, ..Default::default() });
            }
        }
        Ok(at..at.saturating_add(added))
    }
}

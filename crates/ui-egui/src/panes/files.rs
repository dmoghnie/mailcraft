//! The Files view: every file mail brought or took that this computer holds, newest first,
//! across accounts (the engine's `organize/attached.rs`). The side pane picks a kind and says
//! how many there are; the main pane has a box for a person over the files, one a row (its
//! kind, name, size, who and when), and the top bar's box searches their names. A click
//! previews a file beside the list (the reading pane, its message shown); double-click opens it;
//! right-click gives Preview, Open, Save As… and Go to the message.

use egui::{Align, Layout, RichText, Sense, Ui, vec2};
use mailcraft_engine::model::dates;
use mailcraft_engine::{AttachedFile, FileKind, Session};
use serde_json::json;

use crate::icons::{self, Icon};
use crate::theme;
use crate::{Action, UiState, widgets};

/// A row's height.
const ROW_H: f32 = 34.0;

/// A kind's icon, as the reading pane's attachments have them.
fn kind_icon(kind: FileKind) -> Icon {
    match kind {
        FileKind::Picture => Icon::Image,
        FileKind::Archive => Icon::Archive,
        FileKind::Document | FileKind::Pdf | FileKind::Other => Icon::File,
    }
}

/// The side pane: all files or one kind, and how many are shown (and that more are being
/// looked for).
pub fn nav(s: &Session, st: &mut UiState, ui: &mut Ui, acts: &mut Vec<Action>) {
    let t = st.tokens;
    ui.add_space(6.0);
    ui.with_layout(crate::bidi::column(), |ui| {
        let kinds = std::iter::once((None, "All files")).chain(FileKind::ALL.iter().map(|k| (Some(*k), k.label())));
        for (kind, label) in kinds {
            let on = s.files.filter.kind == kind;
            if ui.selectable_label(on, tl!(label)).clicked() && !on {
                acts.push(Action::Run("files.filter", json!({"kind": kind.map_or("all", FileKind::code)})));
            }
        }
        ui.add_space(8.0);
        ui.label(RichText::new(tf!("{n} files", n = s.files_shown().len())).color(t.faint).small());
        if !s.files_done() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(tl!("Looking through your mail…")).color(t.faint).small());
            });
        }
    });
}

/// The main pane: the box for a person, then the files.
pub fn show(s: &Session, st: &mut UiState, ui: &mut Ui, acts: &mut Vec<Action>) {
    let t = st.tokens;
    egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
        let mut person = s.files.filter.person.clone();
        let resp = ui.add(egui::TextEdit::singleline(&mut person).hint_text(tl!("From or to")).desired_width(240.0));
        if resp.changed() {
            acts.push(Action::Run("files.filter", json!({"person": person})));
        }
    });
    let files = s.files_shown();
    if files.is_empty() {
        if s.files_done() {
            let filtered = s.files.filter != mailcraft_engine::FilesFilter::default();
            let said = if filtered { "No file matches." } else { "No files: the files of the mail kept on this computer are listed here." };
            widgets::empty_state(ui, &t, Icon::Attach, tl!(said));
        } else {
            ui.vertical_centered(|ui| ui.spinner());
        }
        return;
    }
    egui::ScrollArea::vertical().auto_shrink(false).id_salt("files").show_rows(ui, ROW_H, files.len(), |ui, range| {
        for f in files.get(range).unwrap_or_default() {
            row(s, st, ui, f, acts);
        }
    });
}

/// A file's row: lit while it's previewed.
fn row(s: &Session, st: &UiState, ui: &mut Ui, f: &AttachedFile, acts: &mut Vec<Action>) {
    let t = st.tokens;
    let on = st.attachment_view.as_ref().is_some_and(|(k, i)| *k == f.key && *i == f.index);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    if on || resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, if on { t.accent_soft } else { t.hover_row });
    }
    let who = if f.person.name.is_empty() { f.person.email.as_str() } else { f.person.name.as_str() };
    let who = if f.sent { tf!("To {name}", name = who) } else { who.to_string() };
    let when = crate::i18n::date(&dates::list_time(f.date, &s.clock));
    let size = widgets::human_size(f.size);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{}, {size}, {who}, {when}", f.name)));
    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(8.0, 0.0))).layout(crate::bidi::row_from_start(Align::Center)));
    let (icon, _) = inner.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
    icons::paint(inner.painter(), icon, kind_icon(f.kind), t.dim);
    inner.add_space(8.0);
    let end_w = 220.0_f32.min(inner.available_width() * 0.5);
    let name_w = (inner.available_width() - end_w - 8.0).max(40.0);
    let name = widgets::one_line(&inner, &f.name, theme::regular(13.0), t.text, name_w);
    let (name_rect, _) = inner.allocate_exact_size(vec2(name_w, ROW_H), Sense::hover());
    inner.painter().galley(egui::pos2(name_rect.left(), name_rect.center().y - name.size().y / 2.0), name, t.text);
    inner.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.label(RichText::new(when).color(t.dim).small());
        ui.label(RichText::new(size).color(t.faint).small());
        ui.add(egui::Label::new(RichText::new(who).color(t.dim).small()).truncate());
    });
    let (key, index) = (f.key.clone(), f.index);
    if resp.double_clicked() {
        acts.push(Action::MailFile { key: key.clone(), index, name: f.name.clone(), open: true });
    } else if resp.clicked() {
        acts.push(Action::PreviewFile(key.clone(), index));
    }
    resp.context_menu(|ui| {
        if ui.button(tl!("Preview")).clicked() {
            acts.push(Action::PreviewFile(key.clone(), index));
            ui.close();
        }
        if ui.button(tl!("Open")).clicked() {
            acts.push(Action::MailFile { key: key.clone(), index, name: f.name.clone(), open: true });
            ui.close();
        }
        if ui.button(tl!("Save As…")).clicked() {
            acts.push(Action::MailFile { key: key.clone(), index, name: f.name.clone(), open: false });
            ui.close();
        }
        ui.separator();
        if ui.button(tl!("Go to the message")).clicked() {
            acts.push(Action::Run("files.goTo", json!({"key": key.to_id(), "index": index})));
            ui.close();
        }
    });
}

//! The ribbon's Home in Mail, each button known by the command it runs: in its group on the full
//! ribbon, on the simplified ribbon's row, or as a line of that ribbon's More commands. A command
//! added to the simplified ribbon (Customize the ribbon) is a button of its name, run as it is.

use mailcraft_engine::model::FolderRole;

use super::mail_tabs::enabled;
use super::{junk_menu, quick_steps_menu};
use crate::*;

/// Home's groups on the full ribbon: their names, and their buttons in order.
pub(super) const GROUPS: [(&str, &[&str]); 7] = [
    ("New", &["mail.new"]),
    ("Delete", &["mail.delete", "mail.archive", "mail.junk"]),
    ("Move", &["mail.move", "quickStep.run", "rules.list"]),
    ("Respond", &["mail.reply", "mail.replyAll", "mail.forward"]),
    ("Tags", &["mail.toggleRead", "mail.flag", "mail.categorize", "mail.pin", "mail.snooze"]),
    ("Find", &["list.filter"]),
    ("AI", &["assistant.pane", "mail.digest"]),
];

/// Where a command added to the ribbon lives, by the start of its id, and the icon its button
/// takes (any other: Quick steps').
const AREA_ICONS: [(&str, Icon); 14] = [
    ("mail.", Icon::Inbox),
    ("list.", Icon::Funnel),
    ("folder.", Icon::Folder),
    ("search.", Icon::Search),
    ("view.", Icon::Layout),
    ("calendar.", Icon::Calendar),
    ("event.", Icon::Calendar),
    ("contact.", Icon::People),
    ("people.", Icon::People),
    ("task.", Icon::Task),
    ("note.", Icon::Note),
    ("outbox.", Icon::Outbox),
    ("print.", Icon::Print),
    ("account.", Icon::Person),
];

/// Home on the full ribbon: every button, in its group.
pub(super) fn ribbon_home(ui: &mut Ui, s: &Session, t: Tokens, acts: &mut Vec<Action>) {
    for (i, (name, ids)) in GROUPS.iter().enumerate() {
        for id in *ids {
            button(ui, s, t, id, None, acts);
        }
        widgets::group(ui, &t, name, i + 1 == GROUPS.len());
    }
}

fn in_junk(s: &Session) -> bool {
    s.view_role() == Some(FolderRole::Junk)
}

fn first_unread(s: &Session) -> bool {
    s.first_selected().is_some_and(|m| !m.flags.seen)
}

fn pinned(s: &Session) -> bool {
    s.first_selected().is_some_and(|m| m.flags.pinned())
}

/// What Home's button running `id` is called now, in English (its key tip is found by it):
/// Report junk is Not junk in Junk Email, Mark unread is Mark read on unread mail, Pin is Unpin
/// on pinned mail. Another command: its own name.
pub(super) fn name(s: &Session, id: &'static str) -> &'static str {
    match id {
        "mail.new" => "New mail",
        "mail.delete" => "Delete",
        "mail.archive" => "Archive",
        "mail.junk" if in_junk(s) => "Not junk",
        "mail.junk" => "Report junk",
        "mail.move" => "Move to",
        "quickStep.run" => "Quick steps",
        "rules.list" => "Rules",
        "mail.reply" => "Reply",
        "mail.replyAll" => "Reply all",
        "mail.forward" => "Forward",
        "mail.toggleRead" if first_unread(s) => "Mark read",
        "mail.toggleRead" => "Mark unread",
        "mail.flag" => "Flag",
        "mail.categorize" => "Categorize",
        "mail.pin" if pinned(s) => "Unpin",
        "mail.pin" => "Pin",
        "mail.snooze" => "Snooze",
        "list.filter" => "Filter",
        "assistant.pane" => "Ask AI",
        "mail.digest" => "Digest",
        _ => s.registry().get(id).map_or(id, |c| c.label),
    }
}

/// Whether `id` is one of Home's own buttons.
fn home_button(id: &str) -> bool {
    mailcraft_engine::HOME_BUTTONS.contains(&id)
}

/// What pressing the button running `id` does; none: it opens a menu (Quick steps, Rules,
/// Categorize, Snooze, Filter).
pub(super) fn act(s: &Session, id: &'static str) -> Option<Action> {
    Some(match id {
        "mail.junk" => Action::Run(if in_junk(s) { "mail.notJunk" } else { "mail.junk" }, json!({})),
        "mail.move" => Action::Dialog(Dialog::move_to()),
        "mail.pin" => Action::Run("mail.pin", json!({"on": !pinned(s)})),
        "quickStep.run" | "rules.list" | "mail.categorize" | "mail.snooze" | "list.filter" => return None,
        // Written in the language the window is in.
        "mail.digest" => Action::Run(id, json!({"language": crate::i18n::current().code()})),
        // Delete is asked first when it's for good (`act_run`).
        _ => Action::Run(id, json!({})),
    })
}

/// Whether the button running `id` can be used now.
pub(super) fn can(s: &Session, id: &'static str) -> bool {
    match id {
        "mail.junk" => enabled(s, if in_junk(s) { "mail.notJunk" } else { "mail.junk" }),
        "quickStep.run" | "rules.list" | "list.filter" => true,
        "mail.categorize" | "mail.pin" | "mail.snooze" => !s.selection.is_empty(),
        "mail.digest" => !s.prefs.ai.profiles.is_empty(),
        _ => enabled(s, id),
    }
}

/// Its hover text: its shortcut.
fn tip(id: &str) -> Option<&'static str> {
    Some(match id {
        "mail.new" => "New mail (Ctrl+N)",
        "mail.delete" => "Delete (Del)",
        "mail.archive" => "Archive (Backspace)",
        "mail.move" => "Move to a folder (Ctrl+Shift+V)",
        "mail.reply" => "Reply (Ctrl+R)",
        "mail.replyAll" => "Reply all (Ctrl+Shift+R)",
        "mail.forward" => "Forward (Ctrl+F)",
        "mail.toggleRead" => "Read / Unread (Ctrl+Q / Ctrl+U)",
        "mail.flag" => "Flag / Unflag (Insert)",
        _ => return None,
    })
}

fn icon(s: &Session, id: &str) -> Icon {
    match id {
        "mail.new" => Icon::NewMail,
        "mail.delete" => Icon::Delete,
        "mail.archive" => Icon::Archive,
        "mail.junk" => Icon::Junk,
        "mail.move" => Icon::Move,
        "quickStep.run" => Icon::Steps,
        "rules.list" => Icon::Rules,
        "mail.reply" => Icon::Reply,
        "mail.replyAll" => Icon::ReplyAll,
        "mail.forward" => Icon::Forward,
        "mail.toggleRead" if first_unread(s) => Icon::Read,
        "mail.toggleRead" => Icon::Unread,
        "mail.flag" => Icon::Flag,
        "mail.categorize" => Icon::Tag,
        "mail.pin" => Icon::Pin,
        "mail.snooze" => Icon::Clock,
        "list.filter" => Icon::Funnel,
        "assistant.pane" | "mail.digest" => Icon::Sparkle,
        _ => AREA_ICONS.iter().find(|(area, _)| id.starts_with(area)).map_or(Icon::Steps, |(_, icon)| *icon),
    }
}

/// Whether the button has a drop-down arrow beside it.
pub(super) fn has_arrow(id: &str) -> bool {
    matches!(id, "mail.new" | "mail.delete" | "mail.junk" | "mail.toggleRead" | "mail.flag")
}

/// The button running `id`, with its drop-down. One added to the simplified ribbon (`added`: its
/// place among those, from 0) is reached by a digit after Alt, H.
pub(super) fn button(ui: &mut Ui, s: &Session, t: Tokens, id: &'static str, added: Option<usize>, acts: &mut Vec<Action>) {
    let on = can(s, id);
    let label = name(s, id);
    let r = if id == "mail.new" {
        widgets::accent_button(ui, icon(s, id), label, &t, on)
    } else if home_button(id) {
        widgets::tool_button(ui, icon(s, id), label, &t, on)
    } else {
        // Its key tip is its digit, never a Home button's of the same name.
        let mut r = widgets::tool_button(ui, icon(s, id), widgets::Label::shown(id, tl!(label)), &t, on);
        if let Some(n) = added {
            crate::keytips::apply_added(ui, n, &mut r, on, &t);
        }
        r
    };
    let r = match tip(id) {
        Some(tip) => r.on_hover_text(tl!(tip)),
        None => r,
    };
    match act(s, id) {
        Some(a) => {
            if r.clicked() {
                acts.push(a);
            }
        }
        None => {
            egui::Popup::menu(&r).show(|ui| menu(ui, s, t, id, acts));
        }
    }
    drop_down(ui, s, t, id, acts);
}

/// The menu of a button that opens one.
fn menu(ui: &mut Ui, s: &Session, t: Tokens, id: &str, acts: &mut Vec<Action>) {
    match id {
        "quickStep.run" => quick_steps_menu(ui, s, acts),
        "rules.list" => rules_menu(ui, s, acts),
        "mail.categorize" => organize::categorize_menu(ui, s, &t, acts),
        "mail.snooze" => organize::snooze_menu(ui, s, &t, acts),
        // Show only some mail, as Outlook's Filter Email.
        "list.filter" => {
            for f in mailcraft_engine::ListFilter::EVERY {
                if ui.selectable_label(s.filter == f, tl!(f.label())).clicked() {
                    acts.push(Action::Run("list.filter", json!({"value": f.code()})));
                    ui.close();
                }
            }
        }
        _ => {}
    }
}

/// Home's button running `id` as a line of the simplified ribbon's More commands, or another
/// command's: a menu's opens beside it.
pub(super) fn line(ui: &mut Ui, s: &Session, t: Tokens, id: &'static str, acts: &mut Vec<Action>) {
    let on = can(s, id);
    let text = tl!(name(s, id));
    match act(s, id) {
        Some(a) => {
            if ui.add_enabled(on, egui::Button::new(text)).clicked() {
                acts.push(a);
                ui.close();
            }
        }
        None => {
            ui.add_enabled_ui(on, |ui| ui.menu_button(text, |ui| menu(ui, s, t, id, acts)));
        }
    }
}

/// The drop-down arrow beside a button that has one, and its menu.
fn drop_down(ui: &mut Ui, s: &Session, t: Tokens, id: &str, acts: &mut Vec<Action>) {
    match id {
        // New items of other kinds, as Outlook's New Email drop-down.
        "mail.new" => {
            let r = widgets::tiny_icon_button(ui, Icon::ChevronDown, "New items", &t);
            egui::Popup::menu(&r).show(|ui| new_items(ui, s, acts));
        }
        // Ignore the conversation, as Outlook's Delete drop-down.
        "mail.delete" => {
            let r = widgets::tiny_icon_button(ui, Icon::ChevronDown, "More delete options", &t);
            egui::Popup::menu(&r).show(|ui| delete_options(ui, s, acts));
        }
        // The Junk drop-down: the sender's lists, as Outlook's.
        "mail.junk" => {
            let r = widgets::tiny_icon_button(ui, Icon::ChevronDown, "Junk options", &t);
            egui::Popup::menu(&r).show(|ui| junk_menu(ui, s, acts));
        }
        // Mark All as Read for the folder on view (issue #197), as Outlook's Folder tab has it.
        "mail.toggleRead" => {
            let r = widgets::tiny_icon_button(ui, Icon::ChevronDown, "More ways to mark mail read", &t);
            egui::Popup::menu(&r).show(|ui| mark_all_read(ui, s, acts));
        }
        // Follow Up: when it's due, as Outlook's flag drop-down.
        "mail.flag" => {
            let r = widgets::tiny_icon_button(ui, Icon::ChevronDown, "Follow up", &t);
            egui::Popup::menu(&r).show(|ui| organize::follow_up_menu(ui, s, &t, acts));
        }
        _ => {}
    }
}

/// Rules (issue #243: Home had no way to them): one made from the message, or all of them.
fn rules_menu(ui: &mut Ui, s: &Session, acts: &mut Vec<Action>) {
    let sender = s.first_selected().map(|m| m.from.email.clone()).filter(|e| !e.is_empty());
    if ui.add_enabled(sender.is_some(), egui::Button::new(tl!("Create rule…"))).clicked() {
        acts.push(Action::Dialog(Dialog::rule_from(sender.as_deref().unwrap_or_default())));
        ui.close();
    }
    if ui.button(tl!("Manage rules")).clicked() {
        acts.push(Action::Dialog(Dialog::Rules));
        ui.close();
    }
}

fn new_items(ui: &mut Ui, s: &Session, acts: &mut Vec<Action>) {
    if ui.button(tl!("Email message")).clicked() {
        acts.push(Action::Run("mail.new", json!({})));
        ui.close();
    }
    if ui.button(tl!("Event")).clicked() {
        acts.push(Action::Dialog(Dialog::new_event(s, None)));
        ui.close();
    }
    if ui.button(tl!("Contact")).clicked() {
        acts.push(Action::Dialog(Dialog::new_contact()));
        ui.close();
    }
    // A new message from a template kept (from a message's More options).
    ui.separator();
    ui.menu_button(tl!("From a template"), |ui| {
        for t in s.templates() {
            if ui.button(t.name.as_str()).clicked() {
                acts.push(Action::Run("template.insert", json!({"name": t.name, "new": true})));
                ui.close();
            }
        }
        if s.templates().is_empty() {
            ui.add_enabled(false, egui::Button::new(tl!("No templates yet")));
        }
    });
}

fn delete_options(ui: &mut Ui, s: &Session, acts: &mut Vec<Action>) {
    let ignoring = !s.selection.is_empty() && s.selected_iter().all(|m| s.is_ignored(m));
    let label = if ignoring { "Stop ignoring the conversation" } else { "Ignore the conversation" };
    if ui
        .add_enabled(enabled(s, "mail.ignore"), egui::Button::new(tl!(label)))
        .on_hover_text(tl!("Its messages, and replies that arrive later, go to Deleted Items"))
        .clicked()
    {
        acts.push(Action::Run("mail.ignore", json!({})));
        ui.close();
    }
    // Clean Up: what a later reply quotes in full goes to Deleted Items.
    ui.separator();
    let tip = tl!("Messages quoted in full by a later reply go to Deleted Items");
    if ui.add_enabled(enabled(s, "mail.cleanUp"), egui::Button::new(tl!("Clean up conversation"))).on_hover_text(tip).clicked() {
        acts.push(Action::Run("mail.cleanUp", json!({})));
        ui.close();
    }
    if ui.add_enabled(enabled(s, "folder.cleanUp"), egui::Button::new(tl!("Clean up folder"))).on_hover_text(tip).clicked() {
        acts.push(Action::Run("folder.cleanUp", json!({})));
        ui.close();
    }
    // Sweep: the sender's mail in this folder, cleared out in one step.
    ui.separator();
    let sweep = Dialog::sweep(s);
    let tip = tl!("Clear out the sender's mail in this folder in one step");
    if ui.add_enabled(sweep.is_some(), egui::Button::new(tl!("Sweep…"))).on_hover_text(tip).clicked()
        && let Some(d) = sweep
    {
        acts.push(Action::Dialog(d));
        ui.close();
    }
    let tip = tl!("Lists you never read, bulk mail that piles up, and read mail left for a month, from your Inbox");
    if ui.button(tl!("Clean-up suggestions…")).on_hover_text(tip).clicked() {
        // Every Inbox read first, so the dialog finds what's in those not yet looked at.
        acts.push(Action::Run("mail.cleanupSuggestions", json!({})));
        acts.push(Action::Dialog(Dialog::Cleanup(Box::default())));
        ui.close();
    }
}

fn mark_all_read(ui: &mut Ui, s: &Session, acts: &mut Vec<Action>) {
    let all = match &s.view {
        mailcraft_engine::View::Folder { account, folder } => {
            let name = s.folder(account, folder).map_or_else(|| folder.clone(), crate::i18n::folder);
            tf!("Mark all in {folder} as read", folder = name)
        }
        _ => tl!("Mark all shown as read").to_string(),
    };
    if ui.add_enabled(enabled(s, "mail.markAllRead"), egui::Button::new(all)).clicked() {
        acts.push(Action::Run("mail.markAllRead", json!({})));
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The full ribbon's groups hold Home's buttons, each once, in their order.
    #[test]
    fn the_groups_hold_every_home_button_in_order() {
        let grouped: Vec<&str> = GROUPS.iter().flat_map(|(_, ids)| ids.iter().copied()).collect();
        assert_eq!(grouped, mailcraft_engine::HOME_BUTTONS);
    }
}

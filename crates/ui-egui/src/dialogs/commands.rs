//! Find a command by its name and run it: a box over the window listing the commands that match
//! what's typed (a command palette, as Outlook's search box finds commands and a Mac app's Help
//! menu does). Each row is the command's name in the interface's language, where it lives in the
//! window and its shortcut, so the list teaches them; one that can't run now is dimmed, saying why.

use mailcraft_engine::CommandSpec;
use serde_json::Value;

use super::*;
use crate::i18n;

pub(super) mod ribbon;

/// How many rows the list shows.
const ROWS: usize = 14;

/// How many commands run from here are kept, to lead the list while nothing is typed.
const RECENT: usize = 5;

/// Commands that hand data to an agent (a list of accounts, of rules…) or are the window's own
/// (a sign-in checked, the idle clock, a message changed field by field): nothing to pick.
const HIDDEN: &[&str] = &[
    "account.checked",
    "account.list",
    "account.mailbox",
    "account.vacation",
    "account.keepMail",
    "compose.update",
    "contact.addressBook",
    "contact.list",
    "dataFile.idle",
    "list.rowAction",
    "mail.noAnswer",
    "meetings.list",
    "note.list",
    "options.composeFont",
    "options.startIn",
    "people.vips",
    "search.run",
    "searchFolder.list",
    "security.certificates",
    "task.list",
    "template.list",
];

/// Commands that need something given, which the window asks for first, in the dialog their
/// button opens ([`dialog_for`]).
const OPENS: &[&str] =
    &["mail.move", "mail.copy", "mail.sweep", "category.list", "rules.list", "event.create", "contact.create", "task.create", "view.arrangeRibbon"];

/// Commands that take one of a few values, a row for each: (id, parameter, value, name).
const CHOICES: &[(&str, &str, &str, &str)] = &[
    ("calendar.view", "mode", "day", "Day"),
    ("calendar.view", "mode", "workWeek", "Work week"),
    ("calendar.view", "mode", "week", "Week"),
    ("calendar.view", "mode", "month", "Month"),
    ("calendar.view", "mode", "schedule", "Schedule"),
];

/// The dialog a command in [`OPENS`] is asked for in.
fn dialog_for(id: &str, s: &Session) -> Option<Dialog> {
    Some(match id {
        "mail.move" => Dialog::move_to(),
        "mail.copy" => Dialog::copy_to(),
        "mail.sweep" => Dialog::sweep(s)?,
        "category.list" => Dialog::Categories,
        "rules.list" => Dialog::Rules,
        "event.create" => Dialog::new_event(s, None),
        "contact.create" => Dialog::new_contact(),
        "task.create" => Dialog::new_task(s, None),
        "view.arrangeRibbon" => Dialog::customize_ribbon(s),
        // Asked first, as from the reading pane (a message goes out, or a page opens); from Junk
        // Email, or with no way to leave, it runs and says why it can't.
        "mail.unsubscribe" => {
            let key = s.reading.as_ref()?.key.clone();
            let way = s.leaving(&key).filter(|_| !s.in_junk(&key))?;
            Dialog::Unsubscribe { key, way }
        }
        _ => return None,
    })
}

/// A command to offer: what runs, its name and where it lives, its shortcut, and why it can't
/// run now (`None`: it can).
#[derive(Clone, Debug)]
pub(crate) struct Offer {
    pub(crate) id: &'static str,
    pub(crate) params: Value,
    pub(crate) name: String,
    /// Its name in English too, so an English word finds it in any language.
    english: String,
    pub(crate) place: String,
    pub(crate) shortcut: String,
    pub(crate) off: Option<String>,
}

impl Offer {
    /// What tells this row from the others (a command, with the value it's given).
    fn key(&self) -> String {
        format!("{} {}", self.id, self.params)
    }
}

/// Whether a command needs nothing given: no parameter its description names is required
/// (`"to": string`; an optional one is `"key"?: string`).
pub(crate) fn runs_as_is(spec: &CommandSpec) -> bool {
    !spec.params.contains("\":")
}

/// A shortcut as the keyboard names it: `Mod` is ⌘ on a Mac and Ctrl elsewhere.
fn shown_shortcut(portable: &str) -> String {
    portable.replace("Mod", if cfg!(target_os = "macos") { "⌘" } else { "Ctrl" })
}

/// Where a command lives, in the interface's language: as far as each part is translated (a
/// part only agents read, `(a heading)`, left off).
fn shown_place(location: &str) -> String {
    location
        .split(" › ")
        .map(|p| p.split(" (").next().unwrap_or(p))
        .take_while(|p| i18n::in_every_catalog(p))
        .map(|p| tl!(p).to_string())
        .collect::<Vec<_>>()
        .join(" › ")
}

/// Why a command can't run now, in the interface's language (in general words when its own
/// aren't translated).
fn shown_reason(why: &str) -> String {
    if i18n::in_every_catalog(why) { tl!(why).to_string() } else { tl!("Can't be used now").to_string() }
}

/// Every command the window can offer, in the interface's language: those the interface names
/// (each row's name in every catalog), that need nothing given or are asked for in a dialog.
pub(crate) fn offers(s: &Session) -> Vec<Offer> {
    let reg = s.registry();
    let mut out = Vec::new();
    for spec in reg.all().iter().filter(|c| !HIDDEN.contains(&c.id)) {
        // Each name looked up only once it's known to be offered: a lookup is what's drawn.
        let whole = (runs_as_is(spec) || OPENS.contains(&spec.id)) && i18n::in_every_catalog(spec.label);
        let choices: Vec<&(&str, &str, &str, &str)> = CHOICES.iter().filter(|c| c.0 == spec.id && i18n::in_every_catalog(c.3)).collect();
        if !whole && choices.is_empty() {
            continue;
        }
        let offer = |params: Value, english: &str| Offer {
            id: spec.id,
            params,
            name: tl!(english).to_string(),
            english: english.to_lowercase(),
            place: shown_place(spec.location),
            shortcut: shown_shortcut(spec.shortcut),
            off: (spec.enabled)(s).map(shown_reason),
        };
        if whole {
            out.push(offer(json!({}), spec.label));
        }
        for (_, param, value, name) in choices {
            out.push(offer(json!({ *param: value }), name));
        }
    }
    out
}

/// What can go on the simplified ribbon (Customize the ribbon): Home's own buttons, and every
/// command that runs as it is, named in the interface's language.
pub(crate) fn ribbon_offers(s: &Session) -> Vec<Offer> {
    let reg = s.registry();
    let fits = |spec: &&CommandSpec| {
        (mailcraft_engine::HOME_BUTTONS.contains(&spec.id) || (runs_as_is(spec) && !HIDDEN.contains(&spec.id) && !OPENS.contains(&spec.id)))
            && i18n::in_every_catalog(spec.label)
    };
    reg.all()
        .iter()
        .filter(fits)
        .map(|spec| Offer {
            id: spec.id,
            params: json!({}),
            name: tl!(spec.label).to_string(),
            english: spec.label.to_lowercase(),
            place: shown_place(spec.location),
            shortcut: shown_shortcut(spec.shortcut),
            off: None,
        })
        .collect()
}

/// How well `offer` matches `typed` (lower case): 0 its name starts with it, 1 a word of its
/// name does, 2 its name holds it, 3 where it lives or its English name holds it, 4 its name has
/// its letters in order (a slip of the keyboard: "archv"); `None`: it doesn't match.
fn score(offer: &Offer, typed: &str) -> Option<u8> {
    let name = offer.name.to_lowercase();
    if typed.is_empty() || name.starts_with(typed) {
        return Some(0);
    }
    if name.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(typed)) {
        return Some(1);
    }
    if name.contains(typed) {
        return Some(2);
    }
    if offer.place.to_lowercase().contains(typed) || offer.english.contains(typed) {
        return Some(3);
    }
    let mut letters = name.chars();
    typed.chars().filter(|c| !c.is_whitespace()).all(|c| letters.any(|n| n == c)).then_some(4)
}

/// The offers matching `typed`, best first and those that can run before those that can't; while
/// nothing is typed, those run from here last (`recent`, newest first) lead.
pub(crate) fn matching(all: Vec<Offer>, typed: &str, recent: &[String]) -> Vec<Offer> {
    let typed = typed.trim().to_lowercase();
    let lead = |o: &Offer| if typed.is_empty() { recent.iter().position(|r| *r == o.key()).unwrap_or(RECENT) } else { RECENT };
    let mut found: Vec<(usize, u8, Offer)> = all.into_iter().filter_map(|o| score(&o, &typed).map(|s| (lead(&o), s, o))).collect();
    found.sort_by(|a, b| (a.0, a.1, a.2.off.is_some(), &a.2.name).cmp(&(b.0, b.1, b.2.off.is_some(), &b.2.name)));
    found.into_iter().map(|(_, _, o)| o).collect()
}

/// Where the commands run from here last are kept while MailCraft runs.
fn recent_id() -> egui::Id {
    egui::Id::unique("commands-run-last")
}

/// `offer` run from here: put first of those run last.
fn remember(ctx: &egui::Context, offer: &Offer) {
    ctx.data_mut(|m| {
        let list: &mut Vec<String> = m.get_temp_mut_or_default(recent_id());
        list.retain(|k| *k != offer.key());
        list.insert(0, offer.key());
        list.truncate(RECENT);
    });
}

pub(super) fn draw(ui: &mut Ui, d: &mut Dialog, s: &Session, t: &Tokens, acts: &mut Vec<Action>) -> bool {
    let Dialog::Commands { filter, picked } = d else { return false };
    ui.set_width(520.0);
    title(ui, t, "Find a command");
    let r = ui.add(egui::TextEdit::singleline(filter).hint_text(tl!("Type a command's name")).desired_width(f32::INFINITY));
    // Named as the dialog is, to a screen reader (its hint is only drawn while it's empty).
    ui.ctx().accesskit_node_builder(r.id, |node| node.set_label(tl!("Find a command")));
    // Once, as it opens (as Move to does): asked every frame, Tab never left the box.
    let (shown, now) = (egui::Id::unique("commands-shown"), ui.ctx().cumulative_pass_nr());
    let before: Option<u64> = ui.data(|m| m.get_temp(shown));
    ui.data_mut(|m| m.insert_temp(shown, now));
    if before.is_none_or(|last| last.saturating_add(2) < now) {
        r.request_focus();
    }
    if r.changed() {
        *picked = 0;
    }
    let recent: Vec<String> = ui.data(|m| m.get_temp(recent_id())).unwrap_or_default();
    let found = matching(offers(s), filter, &recent);
    let (down, up, enter) = ui.input(|i| (i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::Enter)));
    let last = found.len().min(ROWS).saturating_sub(1);
    let step = if down {
        picked.saturating_add(1)
    } else if up {
        picked.saturating_sub(1)
    } else {
        *picked
    };
    *picked = step.min(last);
    let mut chosen = if enter { found.get(*picked).cloned() } else { None };
    ui.add_space(6.0);
    if found.is_empty() {
        ui.label(RichText::new(tl!("No command has that name.")).color(t.dim));
    }
    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
        for (i, o) in found.iter().take(ROWS).enumerate() {
            if offer_row(ui, t, o, i == *picked).clicked() {
                chosen = Some(o.clone());
            }
        }
    });
    // One that can't run now stays in the list, saying why.
    if let Some(o) = chosen.filter(|o| o.off.is_none()) {
        remember(ui.ctx(), &o);
        match dialog_for(o.id, s) {
            Some(dialog) => acts.push(Action::Dialog(dialog)),
            None => acts.push(Action::Run(o.id, o.params)),
        }
        return false;
    }
    close_row(ui)
}

/// A row, on one line: the command's name (dimmed when it can't run), then in small grey type
/// where it lives, its shortcut and why it can't run. A screen reader hears its name.
fn offer_row(ui: &mut Ui, t: &Tokens, o: &Offer, picked: bool) -> egui::Response {
    let colour = if o.off.is_some() { t.faint } else { t.text };
    let after: Vec<&str> =
        [o.place.as_str(), o.shortcut.as_str(), o.off.as_deref().unwrap_or_default()].into_iter().filter(|x| !x.is_empty()).collect();
    let mut job = egui::text::LayoutJob::default();
    job.append(&o.name, 0.0, egui::TextFormat::simple(theme::regular(14.0), colour));
    if !after.is_empty() {
        job.append(&after.join("  ·  "), 14.0, egui::TextFormat::simple(theme::regular(12.0), t.dim));
    }
    job.wrap = egui::text::TextWrapping { max_width: ui.available_width(), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let row = ui.selectable_label(picked, job);
    row.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, o.off.is_none(), picked, &o.name));
    if picked {
        row.scroll_to_me(None);
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(id: &'static str, name: &str, place: &str) -> Offer {
        Offer { id, params: json!({}), name: name.into(), english: name.to_lowercase(), place: place.into(), shortcut: String::new(), off: None }
    }

    #[test]
    fn the_best_match_comes_first() {
        let all = vec![
            offer("mail.markAllRead", "Mark all as read", "Folder"),
            offer("mail.markRead", "Mark as read", "Home › Tags"),
            offer("mail.archive", "Archive", "Home › Delete"),
            offer("view.focused", "Focused Inbox", "View › Messages"),
        ];
        let names = |typed: &str| matching(all.clone(), typed, &[]).into_iter().map(|o| o.name).collect::<Vec<_>>();
        // Its name first; one with its letters in order after (a word slipped in between).
        assert_eq!(names("mark as"), ["Mark as read", "Mark all as read"]);
        // A word of the name, before the name holding it.
        assert_eq!(names("read"), ["Mark all as read", "Mark as read"]);
        assert_eq!(names("inbox"), ["Focused Inbox"]);
        // A letter left out.
        assert_eq!(names("archv"), ["Archive"]);
        // Where it lives finds it too.
        assert_eq!(names("delete"), ["Archive"]);
        assert!(names("zzz").is_empty());
    }

    #[test]
    fn one_that_cant_run_comes_after_those_that_can() {
        let mut off = offer("mail.reply", "Reply", "Home › Respond");
        off.off = Some("open a message first".into());
        let all = vec![off, offer("mail.replyAll", "Reply all", "Home › Respond")];
        let names: Vec<String> = matching(all, "reply", &[]).into_iter().map(|o| o.name).collect();
        assert_eq!(names, ["Reply all", "Reply"]);
    }

    #[test]
    fn those_run_last_lead_while_nothing_is_typed() {
        let all = vec![offer("a.one", "Alpha", "X"), offer("b.two", "Beta", "X"), offer("c.three", "Gamma", "X")];
        let recent = [offer("c.three", "Gamma", "X").key()];
        let names: Vec<String> = matching(all.clone(), "", &recent).into_iter().map(|o| o.name).collect();
        assert_eq!(names, ["Gamma", "Alpha", "Beta"]);
        // Typing, the best match leads again.
        let names: Vec<String> = matching(all, "a", &recent).into_iter().map(|o| o.name).collect();
        assert_eq!(names.first().map(String::as_str), Some("Alpha"));
    }

    #[test]
    fn what_is_offered_can_be_picked() {
        let (mut a, _) = crate::tests::app("palette-offers");
        // A message selected: Sweep asks about its sender.
        a.run("list.open", json!({"key": "sample:1:1:INBOX"}));
        let all = offers(&a.session);
        let ids: Vec<&str> = all.iter().map(|o| o.id).collect();
        assert!(ids.contains(&"mail.markAllRead") && ids.contains(&"view.calendar") && ids.contains(&"mail.move"), "{ids:?}");
        // Not one that needs something no dialog asks for, nor one that hands data to agents.
        for hidden in ["folder.open", "dataFile.idle", "compose.update", "account.list", "list.messages"] {
            assert!(!ids.contains(&hidden), "{hidden} offered");
        }
        assert_eq!(ids.iter().filter(|i| **i == "calendar.view").count(), CHOICES.len(), "a row for each view");
        // Each in a dialog of its own.
        for id in OPENS {
            assert!(dialog_for(id, &a.session).is_some() && a.session.registry().get(id).is_some(), "{id}");
        }
        // Two rows never share a name (Notes the module, not the list of notes).
        let mut names: Vec<&str> = all.iter().map(|o| o.name.as_str()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "two rows of the same name");
    }
}

//! The whole conversation in the reading pane (File › Options › Mail): each of its messages a
//! card, in the order they were sent (newest first or last, as chosen). The open one is unfolded,
//! with its header, actions and text; the others are folded to who, when and their first line,
//! with Reply, Reply all and Forward. A click (or Enter) on a folded card opens its message here;
//! one on the open card's top folds it. Up and Down move from card to card. What a reply quotes
//! of an earlier message is folded under Show quoted text.

use super::*;

/// A folded card's height: who and when, then the first line.
const FOLDED_H: f32 = 48.0;
/// The open card's top, which folds it.
const TOP_H: f32 = 24.0;

/// One of the conversation's messages, and whether it's the user's own from Sent Items (not in
/// this list: opened in a window of its own, as from the conversation's list).
pub(super) type Card = (Summary, bool);

/// The conversation's messages as cards, in the order chosen; none when it isn't shown as cards
/// (the option off, a window of its own, a message alone).
pub(super) fn cards_of(s: &Session, r: &mailcraft_engine::Reading, in_window: bool) -> Option<Vec<Card>> {
    if !s.prefs.conversation_pane || in_window || r.thread.len().max(1).saturating_add(r.aside.len()) < 2 {
        return None;
    }
    let here: Vec<&Summary> =
        if r.thread.is_empty() { s.summary(&r.key).into_iter().collect() } else { r.thread.iter().filter_map(|k| s.summary(k)).collect() };
    let mut cards: Vec<Card> = here.into_iter().map(|m| (m.clone(), false)).chain(r.aside.iter().map(|m| ((**m).clone(), true))).collect();
    cards.sort_by_key(|(m, _)| m.when());
    if s.prefs.conversation_newest_first {
        cards.reverse();
    }
    (cards.len() > 1).then_some(cards)
}

/// What the reading pane's scroll area is kept by while the cards are shown: the conversation's
/// (its first card), so opening another of its cards leaves the view where it was.
pub(super) fn scroll_key(cards: &[Card]) -> Option<String> {
    cards.first().map(|(m, _)| m.key.to_id())
}

/// What the keyboard reaches a card by: a folded card, or the open one's top.
fn card_id(key: &MessageKey) -> egui::Id {
    egui::Id::unique(("conversation-card", key.to_id()))
}

/// Where whether the open message is folded (by its top) is kept.
fn folded_id(key: &MessageKey) -> egui::Id {
    egui::Id::unique(("conversation-folded", key.to_id()))
}

/// Where whether the open message's quoted text is shown is kept.
fn quote_shown_id(key: &MessageKey) -> egui::Id {
    egui::Id::unique(("conversation-quote-shown", key.to_id()))
}

/// Where the quote of an earlier message starts in `p`'s text as shown: a byte offset into its
/// plain text, or a line of its formatted text (none in mail laid out in blocks, or thousands of
/// lines long). Found once for each message.
fn quote_of(ui: &Ui, key: &MessageKey, p: &mime::Parsed, formatted: bool) -> Option<usize> {
    let id = egui::Id::unique(("conversation-quote", key.to_id(), formatted));
    if let Some(found) = ui.ctx().data(|d| d.get_temp::<Option<usize>>(id)) {
        return found;
    }
    let found = if !formatted {
        mime::quoted_start(&p.text)
    } else if p.layout.is_empty() && !long_text::many_lines(p) {
        let lines: Vec<String> = p.rich.iter().take(mime::QUOTE_LINES).map(|l| l.iter().map(|s| s.text.as_str()).collect()).collect();
        mime::quoted_line(&lines)
    } else {
        None
    };
    ui.ctx().data_mut(|d| d.insert_temp(id, found));
    found
}

fn kept(ui: &Ui, id: egui::Id) -> bool {
    ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(false)
}

fn keep(ui: &Ui, id: egui::Id, on: bool) {
    ui.ctx().data_mut(|d| d.insert_temp(id, on));
}

/// The conversation in the reading pane's scroll area: its subject, how many messages, then
/// each card. `waiting`: the open message's body is still being got ready.
pub(super) fn cards(
    s: &Session,
    st: &mut UiState,
    ui: &mut Ui,
    r: &mailcraft_engine::Reading,
    summary: Option<&Summary>,
    cards: &[Card],
    waiting: bool,
    acts: &mut Vec<Action>,
) {
    let t = st.tokens;
    let subject = r.parsed.as_ref().map(|p| p.subject.as_str()).or_else(|| summary.map(|m| m.subject.as_str())).unwrap_or_default();
    let subject = if subject.is_empty() { tl!("(no subject)") } else { subject };
    ui.with_layout(crate::bidi::column(), |ui| {
        subject_heading(ui, subject, &t, &subject_marks(s, subject));
        ui.label(RichText::new(tf!("{n} messages in this conversation", n = cards.len())).color(t.dim).small());
    });
    ui.add_space(10.0);
    let folded_open = kept(ui, folded_id(&r.key));
    let oldest = cards.iter().map(|(m, _)| m.when()).min();
    for (m, aside) in cards {
        let open = m.key == r.key;
        if open && !folded_open {
            open_card(s, st, ui, r, summary, m, oldest.is_some_and(|w| w < m.when()), waiting, acts);
        } else if folded(s, &t, ui, m, *aside, acts).clicked() {
            if open {
                keep(ui, folded_id(&r.key), false);
            } else if *aside {
                acts.push(Action::Run("message.popOut", json!({"key": m.key.to_id()})));
            } else {
                // The one open now unfolded when it's back.
                keep(ui, folded_id(&r.key), false);
                acts.push(Action::Open(m.key.clone()));
            }
        }
        ui.add_space(8.0);
    }
    arrows(ui, cards);
}

/// Up and Down on a card: the keyboard to the card before or after it (egui's own arrows go to
/// the nearest control, inside the open card).
fn arrows(ui: &Ui, cards: &[Card]) {
    let ids: Vec<egui::Id> = cards.iter().map(|(m, _)| card_id(&m.key)).collect();
    let Some(at) = ui.memory(|m| m.focused()).and_then(|f| ids.iter().position(|id| *id == f)) else { return };
    if let Some(id) = ids.get(at) {
        let filter = egui::EventFilter { vertical_arrows: true, ..Default::default() };
        ui.memory_mut(|m| m.set_focus_lock_filter(*id, filter));
    }
    let none = egui::Modifiers::NONE;
    let (down, up) = ui.input_mut(|i| (i.consume_key(none, egui::Key::ArrowDown), i.consume_key(none, egui::Key::ArrowUp)));
    let to = if down {
        at.checked_add(1)
    } else if up {
        at.checked_sub(1)
    } else {
        None
    };
    if let Some(id) = to.and_then(|i| ids.get(i)) {
        ui.memory_mut(|m| m.request_focus(*id));
    }
}

/// Who and when on one line: the sender, then (for the user's own from Sent Items) the folder
/// and the time.
fn who_and_when(s: &Session, m: &Summary, aside: bool) -> (String, String) {
    let folder =
        aside.then(|| s.account(&m.key.account).and_then(|a| a.folder(mailcraft_engine::model::FolderRole::Sent)).map(crate::i18n::folder)).flatten();
    let date = crate::i18n::date(&dates::list_time(m.when(), &s.clock));
    (m.from.display().to_string(), folder.map_or(date.clone(), |f| format!("{f} · {date}")))
}

/// A folded card: who and when, its first line under them, and (one in this list) Reply, Reply
/// all and Forward at its end. The response is the card's own.
fn folded(s: &Session, t: &Tokens, ui: &mut Ui, m: &Summary, aside: bool, acts: &mut Vec<Action>) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), FOLDED_H), Sense::hover());
    let resp = ui.interact(rect, card_id(&m.key), Sense::click());
    let fill = if resp.hovered() { t.hover_row } else { t.surface };
    ui.painter().rect(rect, 8.0, fill, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    if resp.has_focus() {
        ui.painter().rect_stroke(rect.shrink(1.0), 8.0, egui::Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
    }
    let (who, when) = who_and_when(s, m, aside);
    let unread = !m.flags.seen && !aside;
    let mirror = crate::bidi::Mirror::new(rect);
    let actions_w = if aside { 0.0 } else { 3.0 * widgets::ACTION_ICON + 8.0 };
    let when_g = ui.painter().layout_no_wrap(when.clone(), theme::regular(12.0), t.dim);
    let who_w = (rect.width() - when_g.size().x - actions_w - 40.0).max(40.0);
    let who_g = widgets::one_line(ui, &who, if unread { theme::bold(13.5) } else { theme::regular(13.5) }, t.text, who_w);
    let line_g = widgets::one_line(ui, &m.preview, theme::regular(12.5), t.dim, (rect.width() - 24.0).max(40.0));
    let p = ui.painter();
    p.galley(egui::pos2(mirror.start(rect.left() + 12.0, who_g.size().x), rect.top() + 7.0), who_g, t.text);
    let when_x = rect.right() - 12.0 - actions_w - when_g.size().x;
    p.galley(egui::pos2(mirror.start(when_x, when_g.size().x), rect.top() + 8.0), when_g, t.dim);
    p.galley(egui::pos2(mirror.start(rect.left() + 12.0, line_g.size().x), rect.top() + 27.0), line_g, t.dim);
    let label = format!("{who}, {when}, {}", m.preview);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    ui.ctx().accesskit_node_builder(resp.id, |node| node.set_expanded(false));
    if !aside {
        let row = egui::Rect::from_min_size(
            egui::pos2(rect.right() - 8.0 - actions_w + 8.0, rect.top() + 4.0),
            vec2(actions_w - 8.0, widgets::ACTION_ICON),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(mirror.rect(row)).layout(crate::bidi::row_from_start(egui::Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (icon, tip, id) in
                [(Icon::Reply, "Reply", "mail.reply"), (Icon::ReplyAll, "Reply all", "mail.replyAll"), (Icon::Forward, "Forward", "mail.forward")]
            {
                if widgets::action_icon(ui, icon, tip, t).clicked() {
                    // Opened first: what's done is done to it.
                    acts.push(Action::Open(m.key.clone()));
                    acts.push(Action::Run(id, json!({})));
                }
            }
        });
    }
    resp
}

/// The open message's card: its top (a click folds it), its header with what can be done with
/// it, its files and its text, with what it quotes of an earlier message folded (`reply`: an
/// earlier card is shown).
#[allow(clippy::too_many_arguments)]
fn open_card(
    s: &Session,
    st: &mut UiState,
    ui: &mut Ui,
    r: &mailcraft_engine::Reading,
    summary: Option<&Summary>,
    m: &Summary,
    reply: bool,
    waiting: bool,
    acts: &mut Vec<Action>,
) {
    let t = st.tokens;
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, t.border))
        .corner_radius(8)
        .inner_margin(Margin { left: 12, right: 12, top: 4, bottom: 12 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if top(s, &t, ui, m).clicked() {
                keep(ui, folded_id(&r.key), true);
            }
            let Some(p) = &r.parsed else {
                not_ready(ui, &t, r, summary, waiting, acts);
                return;
            };
            let formatted = shown_formatted(s, st, &r.key, p);
            let shown = kept(ui, quote_shown_id(&r.key));
            let quote = if reply && s.search.is_none() { quote_of(ui, &r.key, p, formatted) } else { None };
            let o = Open { r, p, summary, in_window: false, fold_from: quote.filter(|_| !shown) };
            let _ = header::header(s, st, ui, &o, header::Part::Card, acts);
            if r.security.as_ref().is_some_and(security::still_locked) {
                return;
            }
            attachments_strip(s, st, ui, &o, formatted, acts);
            message_text(s, st, ui, &o, formatted, true, acts);
            if quote.is_some() {
                ui.add_space(6.0);
                let label = if shown { tl!("Hide quoted text") } else { tl!("Show quoted text") };
                if ui.add(egui::Button::new(RichText::new(label).color(t.accent).small()).frame(false)).clicked() {
                    keep(ui, quote_shown_id(&r.key), !shown);
                }
            }
        });
}

/// The open card's top: who and when, small, and an arrow; a click folds the card.
fn top(s: &Session, t: &Tokens, ui: &mut Ui, m: &Summary) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), TOP_H), Sense::hover());
    let resp = ui.interact(rect, card_id(&m.key), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, t.hover_row);
    }
    if resp.has_focus() {
        ui.painter().rect_stroke(rect, 4.0, egui::Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
    }
    let (who, when) = who_and_when(s, m, false);
    let mirror = crate::bidi::Mirror::new(rect);
    let text = format!("{who} · {when}");
    let g = widgets::one_line(ui, &text, theme::regular(12.0), t.dim, (rect.width() - 40.0).max(40.0));
    let arrow = mirror.rect(egui::Rect::from_min_size(egui::pos2(rect.right() - 20.0, rect.center().y - 6.0), vec2(12.0, 12.0)));
    icons::paint(ui.painter(), arrow, Icon::ChevronDown, if resp.hovered() { t.text } else { t.dim });
    ui.painter().galley(egui::pos2(mirror.start(rect.left() + 2.0, g.size().x), rect.center().y - g.size().y / 2.0), g, t.dim);
    let label = format!("{who}, {when}");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    ui.ctx().accesskit_node_builder(resp.id, |node| node.set_expanded(true));
    resp.on_hover_text(tl!("Fold this message"))
}

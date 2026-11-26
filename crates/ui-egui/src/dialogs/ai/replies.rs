//! How the replies of MailCraft's AI went (the assistant's chats menu): each reply of the chat
//! shown, or of every chat, newest first: when, the model, its questions, steps, tokens and
//! time, the tools it called and how each went, and how it ended. Never what was said: the
//! engine keeps none of it ([`mailcraft_engine::ReplyTrace`]).

use mailcraft_engine::model::dates;
use mailcraft_engine::{ReplyLimit, ReplyTrace, RunState, TRACE_DAYS, TraceEnd};

use super::*;

/// The list's height at most: past it, it scrolls.
const LIST_HEIGHT: f32 = 420.0;
/// The most replies shown, the newest.
const SHOWN_MOST: usize = 100;

/// How reply `r` ended, in a few words.
fn ended(r: &ReplyTrace) -> &'static str {
    match (r.end, r.limit) {
        (TraceEnd::Going, _) => tl!("Still going"),
        (TraceEnd::Answered, _) => tl!("Answered"),
        (TraceEnd::Failed, _) => tl!("Failed"),
        (TraceEnd::Stopped, _) => tl!("Stopped"),
        (TraceEnd::Limit, Some(ReplyLimit::Tokens)) => tl!("Stopped: too many tokens"),
        (TraceEnd::Limit, Some(ReplyLimit::Time)) => tl!("Stopped: it took too long"),
        (TraceEnd::Limit, _) => tl!("Stopped: too many steps"),
        (TraceEnd::Left, _) => tl!("Not finished"),
    }
}

/// How a tool call went, as the chat's cards say it.
fn state_said(state: RunState) -> &'static str {
    match state {
        RunState::Ran => tl!("Ran"),
        RunState::Failed => tl!("Couldn't be done"),
        RunState::Waiting => tl!("Waiting"),
        RunState::Proposed => tl!("Needs your approval"),
        RunState::Done => tl!("Done"),
        RunState::Declined => tl!("Declined"),
        RunState::PutOff => tl!("Put off"),
    }
}

/// The replies of chat `chat` (none: of every chat); whether the dialog stays open.
pub(crate) fn draw(ui: &mut Ui, chat: Option<&str>, s: &Session, t: &Tokens) -> bool {
    ui.set_width(560.0);
    title(ui, t, tl!("How the replies went"));
    let note = tf!("How each reply of MailCraft's AI went, never what was said: kept on this computer for {days} days", days = TRACE_DAYS);
    ui.add(egui::Label::new(RichText::new(note).color(t.faint).small()).wrap());
    ui.add_space(8.0);
    let replies = s.ai_traces(chat);
    if replies.is_empty() {
        ui.label(RichText::new(tl!("No replies yet")).color(t.faint));
    }
    egui::ScrollArea::vertical().max_height(LIST_HEIGHT).show(ui, |ui| {
        for r in replies.iter().take(SHOWN_MOST) {
            reply(ui, r, s, t);
            ui.add_space(8.0);
        }
    });
    let mut open = true;
    ui.add_space(10.0);
    button_row(ui, |ui| {
        if ui.button(tl!("Close")).clicked() {
            open = false;
        }
    });
    open
}

/// One reply: when and how it ended, its model, its counts and its tools.
fn reply(ui: &mut Ui, r: &ReplyTrace, s: &Session, t: &Tokens) {
    let when = crate::i18n::date(&dates::list_time(r.began, &s.clock));
    let short = matches!(r.end, TraceEnd::Failed | TraceEnd::Limit);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(when).color(t.text));
        ui.label(RichText::new(ended(r)).color(if short { t.danger } else { t.dim }));
    });
    if !r.profile.is_empty() {
        ui.add(egui::Label::new(RichText::new(format!("{} · {}", r.profile, r.model)).color(t.dim).small()).wrap());
    }
    let seconds = r.ended.unwrap_or(s.clock.now).saturating_sub(r.began).max(0);
    let counts = [
        tf!("Questions: {n}", n = r.asks),
        tf!("Steps: {n}", n = r.steps),
        tf!("Tokens read: {n}", n = r.read),
        tf!("Tokens written: {n}", n = r.written),
        tf!("Seconds: {n}", n = seconds),
    ];
    ui.add(egui::Label::new(RichText::new(counts.join(" · ")).color(t.faint).small()).wrap());
    if !r.tools.is_empty() {
        let mut tools: Vec<String> = r.tools.iter().map(|x| format!("{}: {}", x.tool, state_said(x.state))).collect();
        if r.tools_more > 0 {
            tools.push(tf!("and {n} more", n = r.tools_more));
        }
        ui.add(egui::Label::new(RichText::new(tools.join(" · ")).color(t.faint).small()).wrap());
    }
}

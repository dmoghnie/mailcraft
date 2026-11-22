//! UI tests: the whole conversation in the reading pane (File › Options › Mail). Its messages are
//! cards, the open one unfolded with what it quotes of an earlier one folded; a card opens with a
//! click or Enter, the open one folds by its top, Up and Down go from card to card. Off, the pane
//! is as it was: one message under the conversation's list.

use egui::accesskit::{self, Role};

use super::*;

const SCREEN: egui::Vec2 = egui::vec2(1280.0, 900.0);

const ZOE: &[u8] = b"From: Zoe Park <zoe@studio.test>\r\nTo: riley@studio.example\r\nSubject: Set build\r\n\
Date: Wed, 14 Oct 2026 09:00:00 +0000\r\nMessage-ID: <set-1@studio.test>\r\n\r\nCan we start the set build on Monday?\r\n";
const SAM: &[u8] = b"From: Sam Okafor <sam@studio.test>\r\nTo: riley@studio.example\r\nSubject: Re: Set build\r\n\
Date: Wed, 14 Oct 2026 10:00:00 +0000\r\nMessage-ID: <set-2@studio.test>\r\nIn-Reply-To: <set-1@studio.test>\r\n\
References: <set-1@studio.test>\r\n\r\nMonday works, I'll order the timber.\r\n\r\n\
On Wed, 14 Oct 2026, Zoe Park wrote:\r\n> Can we start the set build on Monday?\r\n";
const ANA: &[u8] = b"From: Ana Lima <ana@soundlab.test>\r\nTo: riley@studio.example\r\nSubject: Re: Set build\r\n\
Date: Wed, 14 Oct 2026 11:00:00 +0000\r\nMessage-ID: <set-3@studio.test>\r\nIn-Reply-To: <set-2@studio.test>\r\n\
References: <set-1@studio.test> <set-2@studio.test>\r\n\r\nCount me in for the painting.\r\n\r\n\
________________________________\r\nFrom: Sam Okafor <sam@studio.test>\r\nSent: Wednesday 10:00\r\n\
Subject: Re: Set build\r\n\r\nMonday works, I'll order the timber.\r\n";

/// The three messages of a conversation in the Inbox, grouped by conversation, the whole of it
/// in the reading pane (or not), Ana's (the newest) open.
fn conversation(name: &str, cards: bool) -> (MailApp, egui::Context, [MessageKey; 3]) {
    let (mut a, _) = app(name);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let keys = [arrive(&mut a, 9970, ZOE), arrive(&mut a, 9971, SAM), arrive(&mut a, 9972, ANA)];
    a.run("folder.open", json!({"account": "sample", "folder": "INBOX"}));
    a.run("view.conversations", json!({}));
    if cards {
        a.run("view.conversationPane", json!({}));
    }
    a.run("list.open", json!({"key": keys[2].to_id()}));
    settle(&ctx, &mut a, vec![]);
    (a, ctx, keys)
}

/// Frames for what `events` do to settle (a focus asked for lands in the next one), the last
/// one's output.
fn settle(ctx: &egui::Context, a: &mut MailApp, events: Vec<Event>) -> egui::FullOutput {
    sized_frame(ctx, a, SCREEN, events);
    sized_frame(ctx, a, SCREEN, vec![]);
    sized_frame(ctx, a, SCREEN, vec![])
}

/// The texts the reading pane draws: from the left of the conversation's heading on (the list
/// beside it shows the first words of each message, quotes and all).
fn texts(ctx: &egui::Context, a: &mut MailApp) -> Vec<String> {
    let drawn = drawn_text(ctx, a, SCREEN);
    let left = drawn.iter().find(|d| d.text == "3 messages in this conversation").map(|d| d.rect.left()).expect("the conversation's heading");
    drawn.into_iter().filter(|d| d.rect.left() >= left - 40.0).map(|d| d.text).collect()
}

/// The card of `who`'s message (a folded one, or the open one's top), as a screen reader is told it.
fn card(out: &egui::FullOutput, who: &str) -> Option<Named> {
    let start = format!("{who}, ");
    named_in(out).into_iter().find(|n| n.role == Role::Button && n.label.as_deref().is_some_and(|l| l.starts_with(&start)))
}

fn click_card(ctx: &egui::Context, a: &mut MailApp, who: &str) {
    let out = settle(ctx, a, vec![]);
    let at = card(&out, who).and_then(|n| n.centre()).unwrap_or_else(|| panic!("no card of {who}"));
    click(ctx, a, at);
    settle(ctx, a, vec![]);
}

/// The label of what has the keyboard.
fn focused(out: &egui::FullOutput) -> String {
    let u = out.platform_output.accesskit_update.as_ref().expect("AccessKit is on");
    u.nodes.iter().find(|(id, _)| *id == u.focus).and_then(|(_, n)| n.label().map(str::to_string)).unwrap_or_default()
}

/// The accessibility node of `who`'s card.
fn card_node(out: &egui::FullOutput, who: &str) -> accesskit::NodeId {
    let start = format!("{who}, ");
    let u = out.platform_output.accesskit_update.as_ref().expect("AccessKit is on");
    u.nodes.iter().find(|(_, n)| n.role() == Role::Button && n.label().is_some_and(|l| l.starts_with(&start))).map(|(id, _)| *id).expect("the card")
}

#[test]
fn the_conversation_is_cards_with_the_open_one_unfolded() {
    let (mut a, ctx, keys) = conversation("conversation-cards", true);
    let shown = texts(&ctx, &mut a);
    assert!(has(&shown, "3 messages in this conversation"), "{shown:?}");
    let out = settle(&ctx, &mut a, vec![]);
    for who in ["Zoe Park", "Sam Okafor", "Ana Lima"] {
        assert!(card(&out, who).is_some(), "{who}'s card: {:?}", named_in(&out).iter().filter_map(Named::name).collect::<Vec<_>>());
    }
    // In the order they were sent, the oldest first.
    let tops: Vec<f32> = ["Zoe Park", "Sam Okafor", "Ana Lima"].iter().filter_map(|w| card(&out, w)?.rect.map(|r| r.top())).collect();
    assert!(tops.windows(2).all(|w| w[0] < w[1]), "{tops:?}");
    // Ana's open: her words, what she quotes of Sam's folded under Show quoted text.
    assert!(has(&shown, "Count me in for the painting.") && !has(&shown, "Sent: Wednesday 10:00"), "{shown:?}");
    click_named(&ctx, &mut a, "Show quoted text");
    settle(&ctx, &mut a, vec![]);
    assert!(has(&texts(&ctx, &mut a), "Sent: Wednesday 10:00"));
    assert!(named(&ctx, &mut a, SCREEN).iter().any(|n| n.name() == Some("Hide quoted text")));
    // Sam's card opens his message here, his quote of Zoe's folded.
    click_card(&ctx, &mut a, "Sam Okafor");
    assert_eq!(a.session.reading.as_ref().map(|r| r.key.clone()), Some(keys[1].clone()));
    let shown = texts(&ctx, &mut a);
    assert!(has(&shown, "Monday works, I'll order the timber.") && !has(&shown, "Zoe Park wrote:"), "{shown:?}");
    assert!(a.session.windows.is_empty(), "opened here, not in a window");
    // Its top folds it: no message's text, every card folded.
    click_card(&ctx, &mut a, "Sam Okafor");
    let names: Vec<String> = named(&ctx, &mut a, SCREEN).iter().filter_map(Named::name).map(str::to_string).collect();
    assert!(!names.iter().any(|n| n == "Show quoted text" || n == "Fold this message"), "{names:?}");
    // And it unfolds again.
    click_card(&ctx, &mut a, "Sam Okafor");
    assert!(named(&ctx, &mut a, SCREEN).iter().any(|n| n.name() == Some("Show quoted text")));
}

#[test]
fn up_and_down_go_from_card_to_card_and_enter_opens_one() {
    let (mut a, ctx, keys) = conversation("conversation-cards-keys", true);
    let out = settle(&ctx, &mut a, vec![]);
    settle(
        &ctx,
        &mut a,
        vec![Event::AccessKitActionRequest(accesskit::ActionRequest {
            action: accesskit::Action::Focus,
            target_node: card_node(&out, "Zoe Park"),
            target_tree: accesskit::TreeId::ROOT,
            data: None,
        })],
    );
    let on = |a: &mut MailApp, k: Key| focused(&settle(&ctx, a, vec![key(k, Modifiers::NONE)]));
    assert!(focused(&settle(&ctx, &mut a, vec![])).starts_with("Zoe Park, "));
    assert!(on(&mut a, Key::ArrowDown).starts_with("Sam Okafor, "));
    assert!(on(&mut a, Key::ArrowDown).starts_with("Ana Lima, "), "the open card's top");
    assert!(on(&mut a, Key::ArrowUp).starts_with("Sam Okafor, "));
    assert_eq!(a.session.reading.as_ref().map(|r| r.key.clone()), Some(keys[2].clone()), "moving reads nothing else");
    // Enter opens it here, the keyboard staying on its card.
    let after = on(&mut a, Key::Enter);
    assert_eq!(a.session.reading.as_ref().map(|r| r.key.clone()), Some(keys[1].clone()));
    assert!(a.session.windows.is_empty(), "opened here, not in a window");
    assert!(after.starts_with("Sam Okafor, "), "{after}");
}

#[test]
fn newest_first_and_off_as_it_was() {
    let (mut a, ctx, _) = conversation("conversation-cards-order", true);
    a.run("view.conversationNewestFirst", json!({}));
    let out = settle(&ctx, &mut a, vec![]);
    let tops: Vec<f32> = ["Ana Lima", "Sam Okafor", "Zoe Park"].iter().filter_map(|w| card(&out, w)?.rect.map(|r| r.top())).collect();
    assert!(tops.len() == 3 && tops.windows(2).all(|w| w[0] < w[1]), "the newest at the top: {tops:?}");
    // Off: the conversation's list over the whole of the open message, its quote and all.
    let (mut b, ctx, _) = conversation("conversation-cards-off", false);
    let shown = texts(&ctx, &mut b);
    assert!(has(&shown, "3 messages in this conversation") && has(&shown, "Sent: Wednesday 10:00"), "{shown:?}");
    let out = settle(&ctx, &mut b, vec![]);
    assert!(card(&out, "Zoe Park").is_none() && !named_in(&out).iter().any(|n| n.name() == Some("Show quoted text")));
}

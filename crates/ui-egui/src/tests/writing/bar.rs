//! UI tests: the bar over the message being written is one slim row: the To box starts near the
//! top; Aa shows the formatting row (kept as last left); the rest is in More options, which the
//! keyboard reaches and a screen reader hears.

use super::*;

const SCREEN: egui::Vec2 = egui::vec2(1280.0, 800.0);
/// The most the To row starts below the top of where the message is written: the bar's one row
/// and From take about 95 points (four rows of controls took about 200).
const TO_WITHIN: f32 = 120.0;

/// A reply in the reading pane, drawn a few times.
fn replying(name: &str) -> (MailApp, egui::Context) {
    let (mut a, _) = app(name);
    a.session.prefs.new_mail_window = false;
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    frame(&ctx, &mut a, vec![]);
    let k = a.session.list.first().map(|m| m.key.clone()).unwrap();
    a.run("list.open", json!({"key": k.to_id()}));
    a.run("mail.reply", json!({}));
    for _ in 0..3 {
        sized_frame(&ctx, &mut a, SCREEN, vec![]);
    }
    (a, ctx)
}

fn names_of(ctx: &egui::Context, a: &mut MailApp) -> Vec<String> {
    named(ctx, a, SCREEN).iter().filter_map(Named::name).map(str::to_string).collect()
}

/// Where the To row is: its button (the box beside it is named by what's in it).
fn to_row(ctx: &egui::Context, a: &mut MailApp) -> egui::Rect {
    let nodes = named(ctx, a, SCREEN);
    let to = nodes.iter().filter(|n| n.name() == Some("To") && n.role == egui::accesskit::Role::Button).find_map(|n| n.rect);
    to.unwrap_or_else(|| panic!("no To row: {:?}", nodes.iter().map(|n| (n.role, n.name())).collect::<Vec<_>>()))
}

/// How far below the top of where the message is written the To row starts.
fn to_below_top(ctx: &egui::Context, a: &mut MailApp) -> f32 {
    let to = to_row(ctx, a);
    let area = a.ui.compose_area.map(|(_, r)| r).expect("the message is written in the reading pane");
    to.top() - area.top()
}

/// The label of what has the keyboard.
fn focused(out: &egui::FullOutput) -> Option<String> {
    let tree = out.platform_output.accesskit_update.as_ref()?;
    tree.nodes.iter().find(|(id, _)| *id == tree.focus).and_then(|(_, n)| n.label().map(str::to_string))
}

#[test]
fn the_bar_is_one_row_and_the_formatting_shows_when_asked() {
    let (mut a, ctx) = replying("compose-bar");
    let names = names_of(&ctx, &mut a);
    for what in ["Attach File", "Link", "Pictures", "Signature", "Show or hide the formatting", "More options", "Save draft", "Discard"] {
        assert!(names.iter().any(|n| n == what), "{what}: {names:?}");
    }
    assert!(!names.iter().any(|n| n == "Font" || n == "Word count"), "the formatting and More options' items wait: {names:?}");
    let slim = to_below_top(&ctx, &mut a);
    assert!(slim <= TO_WITHIN, "To starts {slim} below the top");
    // Aa: the formatting row, kept for the next message.
    click_named(&ctx, &mut a, "Show or hide the formatting");
    frame(&ctx, &mut a, vec![]);
    assert!(a.session.prefs.format_bar);
    assert!(names_of(&ctx, &mut a).iter().any(|n| n.starts_with("Bold")), "{:?}", names_of(&ctx, &mut a));
    assert!(to_below_top(&ctx, &mut a) > slim);
    click_named(&ctx, &mut a, "Show or hide the formatting");
    frame(&ctx, &mut a, vec![]);
    assert!(!a.session.prefs.format_bar);
}

#[test]
fn more_options_is_reached_by_the_keyboard() {
    let (mut a, ctx) = replying("compose-bar-keys");
    // Back from the To box, key by key, to More options.
    // In the box beside To's button.
    let row = to_row(&ctx, &mut a);
    let to = egui::pos2(row.right() + 60.0, row.center().y);
    click(&ctx, &mut a, to);
    let mut on = None;
    for _ in 0..30 {
        sized_frame(&ctx, &mut a, SCREEN, vec![key(Key::Tab, Modifiers::SHIFT)]);
        on = focused(&sized_frame(&ctx, &mut a, SCREEN, vec![]));
        if on.as_deref() == Some("More options") {
            break;
        }
    }
    assert_eq!(on.as_deref(), Some("More options"));
    sized_frame(&ctx, &mut a, SCREEN, vec![key(Key::Enter, Modifiers::NONE)]);
    sized_frame(&ctx, &mut a, SCREEN, vec![]);
    let names = names_of(&ctx, &mut a);
    for what in [
        "Address Book",
        "Check Names",
        "Date & time",
        "Word count",
        "Bcc",
        "Replies to",
        "Plain text",
        "High importance",
        "Low importance",
        "Read receipt",
    ] {
        assert!(names.iter().any(|n| n == what), "{what}: {names:?}");
    }
    // Chosen from there: the message's own option.
    click_named(&ctx, &mut a, "Read receipt");
    frame(&ctx, &mut a, vec![]);
    assert!(a.session.compose.as_ref().is_some_and(|c| c.read_receipt));
}

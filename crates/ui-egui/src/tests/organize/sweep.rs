//! Sweep from the ribbon's Delete drop-down: how first, with what each way removes, then the
//! sender's mail in the folder goes.

use super::*;

const SCREEN: egui::Vec2 = egui::vec2(1400.0, 860.0);
const OPS: &str = "ops@studio.example";

/// Click the button called `label` (the dialog's title is called Sweep too).
fn click_named(ctx: &egui::Context, a: &mut MailApp, label: &str) {
    let all = named(ctx, a, SCREEN);
    let at = all.iter().filter(|n| n.name() == Some(label) && n.role == egui::accesskit::Role::Button).filter_map(Named::centre).next_back();
    let at = at.unwrap_or_else(|| panic!("no {label} to click: {:?}", all.iter().filter_map(Named::name).collect::<Vec<_>>()));
    click(ctx, a, at);
}

fn from_ops(a: &MailApp) -> usize {
    a.session.list.iter().filter(|m| m.from.email == OPS).count()
}

#[test]
fn sweep_asks_how_then_clears_the_senders_mail() {
    let _screen = windows::layout::Dressed::new(SCREEN, "");
    let (mut a, _) = app("sweep-ribbon");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    frame(&ctx, &mut a, vec![]);
    let key = a.session.list.iter().find(|m| m.from.email == OPS).map(|m| m.key.clone()).unwrap();
    a.run("list.open", json!({"key": key.to_id()}));
    frame(&ctx, &mut a, vec![]);
    click_named(&ctx, &mut a, "More delete options");
    frame(&ctx, &mut a, vec![]);
    click_named(&ctx, &mut a, "Sweep…");
    frame(&ctx, &mut a, vec![]);
    assert!(matches!(a.ui.dialog, Some(Dialog::Sweep { .. })), "{:?}", a.ui.dialog.as_ref().map(std::mem::discriminant));
    let texts: Vec<String> = drawn_text(&ctx, &mut a, SCREEN).into_iter().map(|d| d.text).collect();
    assert!(texts.iter().any(|t| t.contains(OPS)), "whose mail isn't said: {texts:?}");
    assert!(texts.iter().any(|t| t == "Removed now: 1"), "how many go isn't said: {texts:?}");
    assert_eq!(from_ops(&a), 1, "swept before it was answered");
    click_named(&ctx, &mut a, "Sweep");
    frame(&ctx, &mut a, vec![]);
    assert!(a.ui.dialog.is_none());
    assert_eq!(from_ops(&a), 0, "the sender's mail is still there");
    a.run("edit.undo", json!({}));
    assert_eq!(from_ops(&a), 1, "Undo didn't bring it back");
}

//! VIPs in the window: Mark as VIP on the sender's card, a star before a VIP's name in the list
//! (said to a screen reader too), and the VIP folder under Favorites.

use super::*;

const SCREEN: egui::Vec2 = egui::vec2(1400.0, 860.0);
const SAM: &str = "sam@studio.example";

/// Click what's called `label` (not a row of the message list).
fn click_named(ctx: &egui::Context, a: &mut MailApp, label: &str) {
    let all = named(ctx, a, SCREEN);
    let at = all.iter().filter(|n| n.name() == Some(label) && n.role != egui::accesskit::Role::ListItem).find_map(Named::centre);
    let at = at.unwrap_or_else(|| panic!("no {label} to click: {:?}", all.iter().filter_map(Named::name).collect::<Vec<_>>()));
    click(ctx, a, at);
}

/// What the message list's rows are called, those starting with `who`.
fn rows_of(ctx: &egui::Context, a: &mut MailApp, who: &str) -> Vec<String> {
    named(ctx, a, SCREEN)
        .into_iter()
        .filter(|n| n.role == egui::accesskit::Role::ListItem)
        .filter_map(|n| n.name().map(str::to_string))
        .filter(|n| n.starts_with(who))
        .collect()
}

#[test]
fn a_sender_marked_vip_is_starred_and_has_a_folder() {
    let _screen = windows::layout::Dressed::new(SCREEN, "");
    let (mut a, _) = app("vips-ui");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    frame(&ctx, &mut a, vec![]);
    assert!(!named(&ctx, &mut a, SCREEN).iter().any(|n| n.name() == Some("VIP")), "a VIP folder with no VIPs");
    // Sam's card, from his name over the message: Mark as VIP.
    let key = a.session.list.iter().find(|m| m.from.email == SAM).map(|m| m.key.clone()).unwrap();
    a.run("list.open", json!({"key": key.to_id()}));
    frame(&ctx, &mut a, vec![]);
    click_named(&ctx, &mut a, "Sam Okafor");
    frame(&ctx, &mut a, vec![]);
    click_named(&ctx, &mut a, "Mark as VIP");
    frame(&ctx, &mut a, vec![]);
    assert!(a.session.is_vip(SAM));
    // His row says so; Zoe's doesn't.
    let sams = rows_of(&ctx, &mut a, "Sam Okafor");
    assert!(!sams.is_empty() && sams.iter().all(|n| n.contains("VIP")), "{sams:?}");
    assert!(rows_of(&ctx, &mut a, "Zoe Park").iter().all(|n| !n.contains("VIP")));
    // The VIP folder: his mail.
    click_named(&ctx, &mut a, "VIP");
    frame(&ctx, &mut a, vec![]);
    assert!(a.session.vip_folder_shown());
    assert!(!a.session.list.is_empty() && a.session.list.iter().all(|m| m.from.email == SAM));
}

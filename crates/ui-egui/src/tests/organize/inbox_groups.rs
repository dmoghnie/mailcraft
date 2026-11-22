//! UI tests: the Inbox's four groups as tabs over the list; a sender moved to another group from
//! the right-click menu stays there after a restart; turned off, Focused and Other as before.

use egui::accesskit::Role;
use mailcraft_engine::InboxGroup;

use super::*;

const SIZE: egui::Vec2 = egui::vec2(1280.0, 800.0);

/// The buttons a screen reader is told of named `label`, with where they are.
fn buttons(ctx: &egui::Context, a: &mut MailApp, label: &str) -> Vec<egui::Pos2> {
    named(ctx, a, SIZE).iter().filter(|n| n.role == Role::Button && n.name() == Some(label)).filter_map(Named::centre).collect()
}

#[test]
fn the_inbox_shows_four_groups_and_a_sender_moved_stays_moved() {
    let (mut a, _) = app("groups-ui");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    a.run("view.focused", json!({"on": true}));
    a.run("view.inboxGroups", json!({"on": true}));
    frame(&ctx, &mut a, vec![]);
    frame(&ctx, &mut a, vec![]);
    for g in InboxGroup::ALL {
        assert_eq!(buttons(&ctx, &mut a, g.label()).len(), 1, "the {} tab", g.label());
    }
    assert!(buttons(&ctx, &mut a, "Focused").is_empty());
    // A group with mail in it, from its tab.
    let group = [InboxGroup::Updates, InboxGroup::Promotions, InboxGroup::Transactions]
        .into_iter()
        .find(|g| {
            a.run("list.focusTab", json!({"value": g.code()}));
            !a.session.list.is_empty()
        })
        .expect("the sample has bulk mail");
    a.run("list.focusTab", json!({"value": "primary"}));
    frame(&ctx, &mut a, vec![]);
    let tab = buttons(&ctx, &mut a, group.label()).first().copied().expect("its tab");
    click(&ctx, &mut a, tab);
    frame(&ctx, &mut a, vec![]);
    assert_eq!(a.session.shown_group(), group);
    let sender = a.session.list.first().map(|m| m.from.email.to_ascii_lowercase()).expect("its mail");

    // Right-click its first row: Always move to › Primary.
    let row =
        named(&ctx, &mut a, SIZE).into_iter().filter(|n| n.role == Role::ListItem).filter_map(|n| n.rect).min_by(|x, y| x.min.y.total_cmp(&y.min.y));
    click_at(&ctx, &mut a, row.expect("a row").center(), egui::PointerButton::Secondary, Modifiers::NONE);
    frame(&ctx, &mut a, vec![]);
    click_named(&ctx, &mut a, "Always move to");
    frame(&ctx, &mut a, vec![]);
    let tabs = buttons(&ctx, &mut a, "Primary");
    let item = tabs.iter().max_by(|p, q| p.y.total_cmp(&q.y)).copied().filter(|_| tabs.len() == 2).expect("the menu's Primary, below the tab");
    click(&ctx, &mut a, item);
    frame(&ctx, &mut a, vec![]);
    assert_eq!(a.session.prefs.group_overrides.get(&sender), Some(&InboxGroup::Primary));
    assert!(a.session.list.iter().all(|m| m.from.email.to_ascii_lowercase() != sender), "gone from {group:?}");

    // Kept after a restart.
    let saved = a.saved_state();
    let (mut b, _) = app("groups-ui-restarted");
    b.restore_state(&saved);
    assert_eq!(b.session.prefs.group_overrides.get(&sender), Some(&InboxGroup::Primary));
    assert!(b.session.prefs.inbox_groups);

    // Off: Focused and Other.
    a.run("view.inboxGroups", json!({"on": false}));
    frame(&ctx, &mut a, vec![]);
    assert_eq!(buttons(&ctx, &mut a, "Focused").len(), 1);
    assert_eq!(buttons(&ctx, &mut a, "Other").len(), 1);
    assert!(buttons(&ctx, &mut a, "Promotions").is_empty());
}

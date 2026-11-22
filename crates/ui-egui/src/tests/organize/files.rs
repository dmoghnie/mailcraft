//! UI tests: the Files view, from its button with the modules; a click on a file previews it
//! beside the list, its message shown in the reading pane.

use egui::accesskit::Role;
use mailcraft_engine::{FileKind, Module};

use super::*;
use crate::Pane;

const SIZE: egui::Vec2 = egui::vec2(1280.0, 800.0);

/// What a screen reader is told of, named starting `start`, with where it is.
fn starting(ctx: &egui::Context, a: &mut MailApp, start: &str) -> Vec<egui::Pos2> {
    named(ctx, a, SIZE).iter().filter(|n| n.name().is_some_and(|x| x.starts_with(start))).filter_map(Named::centre).collect()
}

#[test]
fn the_files_view_lists_the_samples_files_and_previews_one() {
    let (mut a, _) = app("files-ui");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    frame(&ctx, &mut a, vec![]);
    let button = named(&ctx, &mut a, SIZE).into_iter().filter(|n| n.role == Role::Button && n.name() == Some("Files")).find_map(|n| n.centre());
    click(&ctx, &mut a, button.expect("the Files button with the modules"));
    assert_eq!(a.session.module, Module::Files);
    // Read a step a frame, as the window does while the view is open.
    for _ in 0..40 {
        frame(&ctx, &mut a, vec![]);
        if a.session.files_done() {
            break;
        }
    }
    assert!(a.session.files_done(), "every folder read");
    let picture = a.session.files_shown().into_iter().find(|f| f.kind == FileKind::Picture).cloned().expect("the sample's colour key");
    assert_eq!(picture.name, "colour-key.png");
    frame(&ctx, &mut a, vec![]);

    // Pictures alone, from the side pane.
    click_named(&ctx, &mut a, "Pictures");
    frame(&ctx, &mut a, vec![]);
    assert!(a.session.files_shown().iter().all(|f| f.kind == FileKind::Picture));
    assert!(starting(&ctx, &mut a, "proof-notes.txt,").is_empty(), "a text file isn't a picture");

    // A click on its row: previewed beside the list, its message in the reading pane.
    let row = starting(&ctx, &mut a, "colour-key.png,").first().copied().expect("its row");
    click(&ctx, &mut a, row);
    frame(&ctx, &mut a, vec![]);
    assert_eq!(a.ui.attachment_view, Some((picture.key.clone(), picture.index)));
    assert_eq!(a.session.reading.as_ref().map(|r| r.key.clone()), Some(picture.key.clone()));
    assert_eq!(a.session.module, Module::Files, "still in Files");
    assert!(a.ui.panes.iter().any(|(p, r)| *p == Pane::Reading && r.width() > 200.0), "the preview beside the list");
}

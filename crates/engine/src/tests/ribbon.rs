//! The simplified ribbon's buttons: arranged by command, checked, and kept in the settings.

use super::*;
use crate::{HOME_BUTTONS, Prefs, RIBBON_ADDED_MOST, RIBBON_ROW};

fn arrange(s: &mut Session, buttons: Value) -> Result<Value, CmdError> {
    s.run("view.arrangeRibbon", &json!({ "buttons": buttons }))
}

/// Commands besides Home's that need nothing given: what can be added to the ribbon.
fn others(s: &Session, n: usize) -> Vec<&'static str> {
    s.registry().all().iter().filter(|c| !HOME_BUTTONS.contains(&c.id) && !c.params.contains("\":")).map(|c| c.id).take(n).collect()
}

#[test]
fn the_ribbon_is_arranged_by_command_and_checked() {
    let mut s = sample_session("ribbon-arrange");
    assert_eq!(s.ribbon_row(), RIBBON_ROW);
    assert!(!s.prefs.simple_ribbon);
    // Home's own and a command that needs nothing, each once; the simplified ribbon shows.
    let back = arrange(&mut s, json!(["mail.forward", "view.compact", "mail.new", "mail.forward"])).unwrap();
    assert_eq!(back["buttons"], json!(["mail.forward", "view.compact", "mail.new"]));
    assert!(s.prefs.simple_ribbon);
    assert_eq!(s.ribbon_row(), ["mail.forward", "view.compact", "mail.new"]);
    // What can't go on it changes nothing: not a command, one that needs something given, not a
    // list, not a name, too many added.
    let too_many = others(&s, RIBBON_ADDED_MOST + 1);
    assert_eq!(too_many.len(), RIBBON_ADDED_MOST + 1);
    for bad in [json!(["no.such"]), json!(["mail.copy"]), json!("mail.new"), json!([1]), json!(too_many)] {
        assert!(matches!(arrange(&mut s, bad.clone()), Err(CmdError::Params(_))), "{bad}");
    }
    assert_eq!(s.ribbon_row(), ["mail.forward", "view.compact", "mail.new"]);
    // Home's own that need something given go on it: their buttons ask.
    arrange(&mut s, json!(["mail.categorize", "list.filter"])).unwrap();
    assert_eq!(s.ribbon_row(), ["mail.categorize", "list.filter"]);
    // [] puts back those it starts with, kept as none chosen; so is choosing them.
    arrange(&mut s, json!([])).unwrap();
    assert!(s.prefs.ribbon_buttons.is_empty());
    assert_eq!(s.ribbon_row(), RIBBON_ROW);
    arrange(&mut s, json!(RIBBON_ROW)).unwrap();
    assert!(s.prefs.ribbon_buttons.is_empty());
}

/// Kept in the settings and read back; a settings file that names what isn't a command, a
/// button twice or too many added keeps what can be shown.
#[test]
fn the_arrangement_is_kept_and_a_damaged_one_is_read_safely() {
    let mut s = sample_session("ribbon-kept");
    arrange(&mut s, json!(["mail.reply", "view.compact", "mail.delete"])).unwrap();
    let saved = serde_json::to_value(&s.prefs).unwrap();
    let (back, unread) = Prefs::from_saved(&saved).unwrap();
    assert!(unread.is_empty());
    assert!(back.simple_ribbon);
    assert_eq!(back.ribbon_buttons, ["mail.reply", "view.compact", "mail.delete"]);
    let extra = others(&s, RIBBON_ADDED_MOST + 2);
    let mut named = vec!["no.such".to_string(), "mail.reply".into(), "mail.reply".into()];
    named.extend(extra.iter().map(|id| id.to_string()));
    s.prefs.ribbon_buttons = named;
    let row = s.ribbon_row();
    assert_eq!(row.first(), Some(&"mail.reply"));
    assert_eq!(row.get(1..), Some(&extra[..RIBBON_ADDED_MOST]));
}

//! The contacts the assistant proposes adding, changing or deleting, in an address book that can
//! be changed. Someone whose address is in the contacts already isn't added again.

use mailcraft_model::vcard::Card;
use serde_json::{Value, json};

use super::*;

/// What the model may say of someone, and the field of the window's contact each is.
const FIELDS: [(&str, &str); 7] = [
    ("name", "name"),
    ("emails", "emails"),
    ("phones", "phones"),
    ("organization", "org"),
    ("title", "title"),
    ("birthday", "birthday"),
    ("notes", "note"),
];

/// `input`'s fields of a contact, as the window's form names them.
fn fields(input: &Value) -> Value {
    let mut params = json!({});
    for (from, to) in FIELDS {
        if let Some(v) = text(input, from) {
            params[to] = json!(v);
        }
    }
    params
}

impl Session {
    /// The contact `add_person` asks for with `input`, checked, and its card's line.
    pub(super) fn person_to_add(&self, chat: &Chat, input: &Value) -> Result<(String, PimChange), String> {
        let account = self.account_for_new(
            chat,
            input,
            |a| self.pim_or_new(a).default_book().is_some(),
            |model| format!("no address book can take a contact: no account whose mail may go to {model} has one"),
        )?;
        if self.pim_or_new(&account).default_book().is_none() {
            return Err(format!("account {account} has no address book a contact can go in"));
        }
        text(input, "name").ok_or("`name` is required")?;
        let card = card_of(input, Card::default())?;
        let profile = self.assistant_profile(chat).map_err(|e| said(&e))?;
        for e in &card.emails {
            let had = self
                .people(&e.value)
                .into_iter()
                .find(|p| self.prefs.ai.allows(&profile, Some(p.account)) && p.card.emails.iter().any(|x| x.value.eq_ignore_ascii_case(&e.value)));
            if let Some(p) = had {
                return Err(format!("{} has {} already: find_people finds them, and change_person changes them", p.card.display(), e.value));
            }
        }
        let what = format!("Add {} to the contacts{}", quoted(&card.display()), said_of(&Card::default(), &card, false));
        Ok((what, PimChange::AddPerson { account, card }))
    }

    /// The change `change_person` asks for with `input`, checked, and its card's line.
    pub(super) fn person_to_change(&self, chat: &Chat, input: &Value) -> Result<(String, PimChange), String> {
        let found = self.found_of(chat, input, "person")?;
        let was = self.person_kept(&found)?;
        let params = fields(input);
        if params.as_object().is_none_or(serde_json::Map::is_empty) {
            return Err("say what changes: name, emails, phones, organization, title, birthday or notes".into());
        }
        let card = card_of(input, was.clone())?;
        if card == *was {
            return Err("that's how they are already: nothing would change".into());
        }
        let what = format!("Change {} in the contacts{}", quoted(&was.display()), said_of(was, &card, true));
        Ok((what, PimChange::ChangePerson { found, card }))
    }

    /// Deleting the contact `found` names, checked: how its card names them.
    pub(super) fn person_to_delete(&self, found: &Found) -> Result<String, String> {
        Ok(format!("the contact {}", quoted(&self.person_kept(found)?.display())))
    }

    /// The contact `found` names, as kept, when they may be changed.
    fn person_kept(&self, found: &Found) -> Result<&Card, String> {
        let Found::Person { account, uid } = found else { return Err(format!("{} isn't a contact", found.name())) };
        let p = self.find_person(uid).filter(|p| p.account == account).ok_or("that contact isn't in the contacts any more")?;
        if self.person_read_only(uid) {
            return Err(format!("{} is in a shared address book that can't be changed", p.card.display()));
        }
        Ok(p.card)
    }

    /// Someone, approved, added to `account`'s contacts: what was done, and them, for the answer
    /// to link.
    pub(super) fn add_person(&mut self, account: &str, card: Card) -> Result<(String, Vec<Found>), String> {
        let added = self.create_card(Some(account), card).map_err(|e| said(&e))?;
        let uid = added.get("uid").and_then(Value::as_str).ok_or("the contact wasn't added")?.to_string();
        let found = Found::Person { account: account.to_string(), uid };
        Ok((format!("Done: added to the contacts, as {}.", found.name()), vec![found]))
    }

    /// The contact `found` names, approved, changed to `card`.
    pub(super) fn change_found_person(&mut self, found: Found, card: Card) -> Result<(String, Vec<Found>), String> {
        let Found::Person { uid, .. } = &found else { return Err("that isn't a contact".into()) };
        self.update_card(uid, |c| *c = card).map_err(|e| said(&e))?;
        Ok(("Done: changed.".into(), vec![found]))
    }
}

/// `base` with what `input` says, as the window's contact form makes it, checked.
fn card_of(input: &Value, base: Card) -> Result<Card, String> {
    let card = crate::cmd::card_from(&fields(input), base).map_err(|e| said(&e))?;
    crate::session::pim::check_card(&card).map_err(|e| said(&e))?;
    Ok(card)
}

/// What `now` has that `was` hadn't, for a card's line: after a colon, or none. `changed`: said
/// as changes (`their title to …`), else as what they have.
fn said_of(was: &Card, now: &Card, changed: bool) -> String {
    let list = |l: &[mailcraft_model::vcard::Typed]| l.iter().map(|t| t.value.as_str()).collect::<Vec<_>>().join(", ");
    let parts: [(&str, String, String); 7] = [
        ("name", was.display(), now.display()),
        ("email", list(&was.emails), list(&now.emails)),
        ("phone", list(&was.phones), list(&now.phones)),
        ("organization", was.org.clone(), now.org.clone()),
        ("title", was.title.clone(), now.title.clone()),
        ("birthday", was.birthday.clone(), now.birthday.clone()),
        ("notes", was.note.clone(), now.note.clone()),
    ];
    let how: Vec<String> = parts
        .iter()
        .filter(|(what, before, after)| before != after && (changed || *what != "name"))
        .map(|(what, _, after)| match (changed, after.trim().is_empty()) {
            (true, true) => format!("no {what}"),
            (true, false) => format!("their {what} to {}", quoted(after)),
            (false, _) => format!("{what} {}", quoted(after)),
        })
        .collect();
    if how.is_empty() { String::new() } else { format!(": {}", how.join("; ")) }
}

//! The Inbox in four groups: Primary, Transactions, Updates and Promotions. Mail is sorted on
//! this computer from what each message says of itself (its sender, its subject, whether it was
//! sent in bulk) and a short list of words, with nothing sent anywhere, so each choice can be
//! explained. It builds on the Focused Inbox: Primary is Focused, less the receipts of senders
//! the user never wrote to, and Other is split three ways. Turned off, the Inbox is Focused and
//! Other as before.
//!
//! In order, a message is in:
//! - **Primary:** a VIP's; a sender's the user put there (or always in Focused); one from
//!   someone the user writes to.
//! - **Transactions:** a receipt, an order, a delivery, a booking or a bank's, by the words of
//!   its subject ([`TRANSACTION_WORDS`]).
//! - **Primary:** anything else not sent in bulk (a mailing list, a newsletter, automatic mail).
//! - **Promotions:** bulk mail whose subject offers something ([`PROMOTION_WORDS`]).
//! - **Updates:** the rest of the bulk mail.
//!
//! A sender put in a group (`mail.moveToGroup`) stays there. In a group other than Primary, each
//! sender's mail can be stacked into one row, as a conversation's is.

use std::collections::HashMap;

use mailcraft_model::thread::Thread;

use super::*;
use crate::session::prefs::InboxGroup;

/// The most senders put in a group by hand (as those put in Focused or Other).
const MOVED_MOST: usize = 10_000;
/// How much of a subject its words are looked for in.
const SUBJECT_LOOKED: usize = 300;

/// Words of a subject that make a message a transaction, in English, German, French, Spanish,
/// Italian, Portuguese and Dutch: a word or phrase alone, or (ending `*`) the start of a word.
pub(crate) const TRANSACTION_WORDS: &[&str] = &[
    // English
    "receipt",
    "invoice",
    "your order",
    "order confirmation",
    "order confirmed",
    "order number",
    "has shipped",
    "shipped",
    "out for delivery",
    "delivered",
    "delivery",
    "tracking number",
    "payment received",
    "payment confirmation",
    "your payment",
    "refund",
    "booking confirmation",
    "your booking",
    "reservation",
    "itinerary",
    "e-ticket",
    "bank statement",
    "account statement",
    "your statement",
    "transaction",
    // German
    "rechnung*",
    "bestellung*",
    "bestellbestätigung",
    "versandbestätigung",
    "versendet",
    "lieferung*",
    "zustellung",
    "zahlung*",
    "quittung",
    "buchung*",
    "kontoauszug",
    "rückerstattung",
    "erstattung",
    // French
    "facture*",
    "votre commande",
    "commande n",
    "confirmation de commande",
    "livraison",
    "expédié*",
    "expédition",
    "paiement",
    "reçu",
    "réservation",
    "relevé",
    "remboursement",
    // Spanish
    "factura*",
    "pedido*",
    "envío",
    "enviado",
    "entrega",
    "pago",
    "recibo",
    "reserva*",
    "extracto",
    "reembolso",
    // Italian
    "fattura",
    "ordine",
    "spedizione",
    "spedito",
    "consegna",
    "pagamento",
    "ricevuta",
    "prenotazione",
    "estratto conto",
    "rimborso",
    // Portuguese
    "fatura",
    "encomenda",
    "recibo",
    "reembolso",
    // Dutch
    "factuur",
    "bestelling*",
    "verzonden",
    "levering",
    "bezorging",
    "betaling*",
    "boeking*",
    "reservering",
    "terugbetaling",
];

/// Words of a subject that make bulk mail a promotion, in the same languages.
pub(crate) const PROMOTION_WORDS: &[&str] = &[
    // English
    "sale",
    "% off",
    "discount*",
    "deal",
    "deals",
    "offer",
    "offers",
    "coupon*",
    "promo*",
    "free shipping",
    "limited time",
    "black friday",
    "cyber monday",
    "save up to",
    "last chance",
    // German
    "angebot*",
    "rabatt*",
    "gutschein*",
    "aktion",
    "sonderangebot*",
    "ausverkauf",
    "gratis",
    // French
    "soldes",
    "réduction*",
    "offre*",
    "remise",
    "bon plan",
    "gratuit*",
    "dernière chance",
    // Spanish
    "oferta*",
    "descuento*",
    "rebajas",
    "cupón",
    "promoción",
    "última oportunidad",
    // Italian
    "offerta",
    "offerte",
    "sconto",
    "sconti",
    "saldi",
    "promozione",
    // Portuguese
    "desconto*",
    "promoção",
    "cupom",
    "liquidação",
    "grátis",
    // Dutch
    "aanbieding*",
    "korting*",
    "uitverkoop",
    "actie",
];

/// Whether `subject` has one of `words` (see [`TRANSACTION_WORDS`]), in any case.
pub(crate) fn says(subject: &str, words: &[&str]) -> bool {
    let text: String = subject.chars().take(SUBJECT_LOOKED).collect::<String>().to_lowercase();
    words.iter().any(|w| has(&text, w))
}

/// Whether `text` has `word` with no letter or digit against it (but after a `*`'s start, or
/// against a sign like `%`).
fn has(text: &str, word: &str) -> bool {
    let (word, start_only) = match word.strip_suffix('*') {
        Some(w) => (w, true),
        None => (word, false),
    };
    if word.is_empty() {
        return false;
    }
    let lettered = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    let (first, last) = (word.chars().next(), word.chars().next_back());
    text.match_indices(word).any(|(at, _)| {
        let before = text.get(..at).and_then(|s| s.chars().next_back());
        let after = text.get(at.saturating_add(word.len())..).and_then(|s| s.chars().next());
        (!lettered(first) || !lettered(before)) && (start_only || !lettered(last) || !lettered(after))
    })
}

/// The list's messages stacked by sender, a stack where its newest message is: each stack's
/// messages oldest first, the newest stacks first, as conversations are.
pub(crate) fn sender_stacks(list: &[SharedSummary]) -> Vec<Thread> {
    let mut at: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<Thread> = Vec::new();
    for (i, m) in list.iter().enumerate() {
        let n = *at.entry(m.from.email.trim().to_ascii_lowercase()).or_insert_with(|| {
            out.push(Thread { messages: Vec::new(), latest: i64::MIN });
            out.len().saturating_sub(1)
        });
        if let Some(t) = out.get_mut(n) {
            t.messages.push(i);
            t.latest = t.latest.max(m.when());
        }
    }
    for t in &mut out {
        t.messages.sort_by_key(|&i| (list.get(i).map_or(0, |m| m.when()), i));
    }
    out.sort_by(|a, b| b.latest.cmp(&a.latest).then_with(|| a.messages.first().cmp(&b.messages.first())));
    out
}

impl Session {
    /// Whether the Inbox on view is shown in its four groups.
    pub fn groups_split(&self) -> bool {
        self.prefs.inbox_groups && self.focus_split()
    }

    /// The group a message is in (see the module's notes).
    pub fn inbox_group(&self, m: &Summary) -> InboxGroup {
        let sender = m.from.email.trim().to_ascii_lowercase();
        if self.is_vip(&sender) {
            return InboxGroup::Primary;
        }
        if let Some(&g) = self.prefs.group_overrides.get(&sender) {
            return g;
        }
        let chosen = self.prefs.focus_overrides.get(&sender).copied();
        if chosen == Some(true) || self.focus_book().and_then(|b| b.get(&sender)).is_some_and(|c| c.sent > 0) {
            return InboxGroup::Primary;
        }
        if says(&m.subject, TRANSACTION_WORDS) {
            return InboxGroup::Transactions;
        }
        if chosen.is_none() && !m.bulk {
            return InboxGroup::Primary;
        }
        if says(&m.subject, PROMOTION_WORDS) { InboxGroup::Promotions } else { InboxGroup::Updates }
    }

    /// Whether a message is in the Inbox's first pane: Primary, with the four groups, else
    /// Focused.
    pub(crate) fn in_first_pane(&self, m: &Summary) -> bool {
        if self.prefs.inbox_groups { self.inbox_group(m) == InboxGroup::Primary } else { self.is_focused(m) }
    }

    /// The group shown, with the four groups: Primary in Focused's place, and one of the others
    /// in Other's (Updates, until another is chosen).
    pub fn shown_group(&self) -> InboxGroup {
        match (self.focus_other, self.group_tab) {
            (false, _) => InboxGroup::Primary,
            (true, InboxGroup::Primary) => InboxGroup::Updates,
            (true, g) => g,
        }
    }

    /// Show a group of the Inbox (Primary is Focused's pane, the others Other's).
    pub fn show_group(&mut self, g: InboxGroup) {
        self.group_tab = g;
        self.focus_other = g != InboxGroup::Primary;
        self.selection.clear();
        self.reading = None;
        self.refresh_list();
    }

    /// Always put `sender`'s mail (an address in lower case) in `g`, with the four groups or
    /// not: in Focused for Primary, in Other for the rest.
    pub(crate) fn put_sender_in_group(&mut self, sender: &str, g: InboxGroup) {
        if self.prefs.group_overrides.len() < MOVED_MOST || self.prefs.group_overrides.contains_key(sender) {
            self.prefs.group_overrides.insert(sender.to_string(), g);
        }
        if self.prefs.focus_overrides.len() < MOVED_MOST || self.prefs.focus_overrides.contains_key(sender) {
            self.prefs.focus_overrides.insert(sender.to_string(), g == InboxGroup::Primary);
        }
    }

    /// Always put the selection's senders in `g`, with the four groups or not: in Focused for
    /// Primary, in Other for the rest.
    pub fn move_senders_to_group(&mut self, g: InboxGroup) -> CmdResult {
        let sel = self.selected_shared();
        if sel.is_empty() {
            return Err(CmdError::Disabled("no message selected".into()));
        }
        for m in &sel {
            let sender = m.from.email.trim().to_ascii_lowercase();
            if !sender.is_empty() {
                self.put_sender_in_group(&sender, g);
            }
        }
        self.reading = None;
        self.selection.clear();
        // Which pane its mail is in changed: a list kept from before a search is built again.
        self.mail_changed();
        self.refresh_list();
        Ok(serde_json::json!({ "group": g.code(), "senders": sel.len() }))
    }

    /// Each sender's mail is stacked into one row: asked for, in a group other than Primary,
    /// the list arranged by date.
    pub(crate) fn digest_shown(&self) -> bool {
        self.groups_split() && self.prefs.group_digest && self.focus_other && self.prefs.arrange == Arrange::Date
    }

    /// The list's rows are stacks: conversations, or a group's senders ([`Session::digest_shown`]).
    pub(crate) fn stacked(&self) -> bool {
        self.prefs.conversations || self.digest_shown()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_found_whole_or_by_their_start() {
        for (subject, words, found) in [
            ("Your order #1234 has shipped", TRANSACTION_WORDS, true),
            ("Ihre Bestellbestätigung Nr. 55", TRANSACTION_WORDS, true),
            ("Rechnungsnummer 2026-11", TRANSACTION_WORDS, true),
            ("Votre commande est en route", TRANSACTION_WORDS, true),
            ("Notes from the order of service", TRANSACTION_WORDS, false),
            ("Salesforce release notes", PROMOTION_WORDS, false),
            ("50% off everything this weekend", PROMOTION_WORDS, true),
            ("Nur heute: 20 % Rabatt", PROMOTION_WORDS, true),
            ("Rabattaktion endet bald", PROMOTION_WORDS, true),
            ("The weekly digest", PROMOTION_WORDS, false),
            ("", PROMOTION_WORDS, false),
        ] {
            assert_eq!(says(subject, words), found, "{subject}");
        }
        assert!(!has("anything", ""), "an empty word is nowhere");
        assert!(!has("a sale", "sale*x"));
    }
}

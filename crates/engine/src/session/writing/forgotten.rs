//! A message that says a file comes with it when none does ([`super::SendCheck::NoAttachment`]):
//! asked about before it goes, as Outlook's attachment reminder and Gmail ask.

use mailcraft_mime::rich;

/// Words saying a file comes with the message, in lower case, matched at the start of a word:
/// "attached", "attachment", "enclosed" and theirs in each language MailCraft's interface is in.
/// Stems where the word bends (`adjunt`: adjunto, adjunta, adjuntamos).
const AT_WORD_START: &[&str] = &[
    // English
    "attach",
    "enclos",
    // German
    "anhang",
    "angehängt",
    "anbei",
    "beigefügt",
    "beiliegend",
    // French
    "pièce jointe",
    "pièces jointes",
    "ci-joint",
    "joint",
    // Spanish
    "adjunt",
    // Italian
    "allegat",
    // Portuguese
    "anex",
    // Dutch
    "bijlage",
    "bijgevoegd",
    // Polish
    "załącz",
    "w załączeniu",
    // Russian
    "вложен",
    "прикреп",
    // Slovak
    "príloh",
    "prilož",
    // Hungarian
    "csatol",
    "mellékel",
    "melléklet",
    // Turkish ("ek" alone is any suffix)
    "ekte",
    "ekli",
    "eklen",
    "ekledi",
];

/// The same where a word takes a prefix of its own (Arabic's ال, Hebrew's ה and ב) or words
/// aren't parted by spaces (Chinese, Japanese, Korean): matched anywhere.
const ANYWHERE: &[&str] = &["مرفق", "مرفقات", "מצורף", "מצורפת", "מצורפים", "מצ\"ב", "מצ״ב", "添付", "첨부", "附件", "随附", "隨附"];

/// The text the user wrote: what comes before the original of a reply or forward (its rule, an
/// empty paragraph drawn as a line) and before the signature (its `-- ` line).
pub(super) fn own_text<'a>(body: &'a str, styles: &rich::Styles) -> &'a str {
    let mut end = 0;
    for (i, line) in body.split('\n').enumerate() {
        if styles.paras.get(i).is_some_and(|p| p.rule) || matches!(line.trim_end(), "--" | "-- ") {
            break;
        }
        end += line.len() + 1;
    }
    body.get(..end.min(body.len())).unwrap_or(body)
}

/// Whether `text` says a file comes with it.
pub(super) fn mentions_attachment(text: &str) -> bool {
    let lower = text.to_lowercase();
    let at_start = |at: usize| lower.get(..at).and_then(|before| before.chars().next_back()).is_none_or(|c| !c.is_alphanumeric());
    AT_WORD_START.iter().any(|w| lower.match_indices(w).any(|(at, _)| at_start(at))) || ANYWHERE.iter().any(|w| lower.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_are_found_at_the_start_of_a_word() {
        for said in [
            "I've attached the frames.",
            "See the attachment",
            "Enclosed is the invoice",
            "Im Anhang die Datei",
            "Vous trouverez ci-joint le devis",
            "Te envío el archivo adjunto",
            "In allegato il preventivo",
            "Segue em anexo",
            "Zie bijlage",
            "W załączeniu przesyłam",
            "Во вложении отчёт",
            "V prílohe posielam",
            "Csatolom a fájlt",
            "Dosya ekte",
            "الملف المرفق",
            "הקובץ המצורף",
            "資料を添付します",
            "첨부 파일",
            "请查收附件",
        ] {
            assert!(mentions_attachment(said), "{said}");
        }
        // Inside another word ("detached", "disjoint"), or not there at all.
        for said in ["The cable came detached", "See you on Thursday", "A weekly plan", "Two disjoint sets"] {
            assert!(!mentions_attachment(said), "{said}");
        }
    }

    #[test]
    fn the_own_text_ends_at_the_signature_or_the_original() {
        let body = "See the frames.\n\n-- \nRiley\n\nFrom: Zoe\nI attached the key.";
        assert_eq!(own_text(body, &rich::Styles::plain(body)), "See the frames.\n\n");
        let body = "Thanks!\n\nFrom: Zoe\nI attached the key.";
        let mut styles = rich::Styles::plain(body);
        if let Some(p) = styles.paras.get_mut(2) {
            p.rule = true;
        }
        assert_eq!(own_text(body, &styles), "Thanks!\n\n");
        assert_eq!(own_text("No quote here", &rich::Styles::plain("No quote here")), "No quote here");
    }
}

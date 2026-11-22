//! A person's or an account's avatar when there's no photo: their initials on a colour of their
//! own, the same on both phones and each time (the colour comes from the address).

use serde::Serialize;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Avatar {
    /// One or two letters: a name's first and last words', else the address's first letter.
    pub initials: String,
    /// The colour's hue in degrees (0–359); the shells pick its lightness for light and dark.
    pub hue: u16,
}

/// The avatar of `who`: `Name <address>`, a bare address, or a name and an address apart.
pub fn of(name: &str, address: &str) -> Avatar {
    let (n, a) = split(name, address);
    Avatar { initials: initials(&n, &a), hue: hue(if a.is_empty() { &n } else { &a }) }
}

/// The name and the address of `Name <address>` (or of the two given apart).
fn split(name: &str, address: &str) -> (String, String) {
    let name = name.trim();
    if !address.trim().is_empty() {
        return (name.trim_matches('"').trim().to_string(), address.trim().to_ascii_lowercase());
    }
    match (name.find('<'), name.rfind('>')) {
        (Some(open), Some(close)) if open < close => {
            let n = name.get(..open).unwrap_or_default().trim().trim_matches('"').trim();
            let a = name.get(open + 1..close).unwrap_or_default().trim().to_ascii_lowercase();
            (n.to_string(), a)
        }
        _ if name.contains('@') => (String::new(), name.to_ascii_lowercase()),
        _ => (name.to_string(), String::new()),
    }
}

fn initials(name: &str, address: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().filter(|w| w.chars().next().is_some_and(char::is_alphanumeric)).collect();
    let first = |w: &str| w.chars().next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
    match words.as_slice() {
        [] => address.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into()),
        [one] => first(one),
        [a, .., z] => first(a) + &first(z),
    }
}

/// A hue from the text (FNV-1a): stable across runs and machines, spread round the circle.
fn hue(text: &str) -> u16 {
    let h = text.bytes().fold(0x811c_9dc5_u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193));
    u16::try_from(h % 360).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_from_the_name_else_the_address() {
        assert_eq!(of("Zoe Park <zoe@studio.example>", "").initials, "ZP");
        assert_eq!(of("\"Sam\" <sam@studio.example>", "").initials, "S");
        assert_eq!(of("riley@studio.example", "").initials, "R");
        assert_eq!(of("Mary Ann van Dyke", "").initials, "MD");
        assert_eq!(of("", "").initials, "?");
        assert_eq!(of("élodie roux", "").initials, "ÉR");
        assert_eq!(of("Zoe", "zoe@studio.example").initials, "Z");
    }

    #[test]
    fn the_colour_follows_the_address_not_the_name() {
        assert_eq!(of("Zoe Park <zoe@studio.example>", "").hue, of("Z. Park <ZOE@studio.example>", "").hue);
        assert!(of("x <zoe@studio.example>", "").hue < 360);
    }

    #[test]
    fn junk_never_panics() {
        for s in ["<", ">", "<>", "a <", "> b <", "\u{0}", "🙂 🙃", "<@>"] {
            let _ = of(s, "");
        }
    }
}

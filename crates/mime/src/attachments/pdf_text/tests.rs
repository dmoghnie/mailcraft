//! A PDF's text as MailCraft's AI reads it: words and lines from where the glyphs are drawn, a
//! standard font's text read without the font in the file, pages apart; junk, a PDF without
//! text and a file too big give none.

use super::*;

/// A PDF of `pages`, each a content stream, in Helvetica (not in the file), its cross-reference
/// table where it says.
pub(crate) fn pdf(pages: &[&str]) -> Vec<u8> {
    let n = pages.len();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect::<Vec<_>>().join(" ")),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
    ];
    for (i, content) in pages.iter().enumerate() {
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {} 0 R /Resources << /Font << /F1 3 0 R >> >> >>",
            5 + 2 * i
        ));
        objects.push(format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()));
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut at = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        at.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for a in at {
        out.extend(format!("{a:010} 00000 n \n").as_bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    out
}

#[test]
fn words_and_lines_from_where_the_glyphs_are() {
    // Two words drawn apart on one line, a third on the next line; a word drawn glyph by glyph.
    let page = "BT /F1 12 Tf 72 700 Td (Invoice) Tj 60 0 Td (4471) Tj ET\nBT /F1 12 Tf 72 680 Td (Total: 1,200 EUR) Tj ET\nBT /F1 12 Tf 72 660 Td [(Pa) 0 (id)] TJ ET";
    assert_eq!(pdf_text(&pdf(&[page])).as_deref(), Some("Invoice 4471\nTotal: 1,200 EUR\nPaid"));
}

#[test]
fn pages_are_apart() {
    let one = "BT /F1 12 Tf 72 700 Td (First) Tj ET";
    let two = "BT /F1 12 Tf 72 700 Td (Second) Tj ET";
    assert_eq!(pdf_text(&pdf(&[one, two])).as_deref(), Some("First\n\nSecond"));
}

#[test]
fn junk_a_scan_and_too_much_give_none() {
    assert_eq!(pdf_text(b"not a pdf"), None);
    assert_eq!(pdf_text(&pdf(&["0 0 m 100 100 l S"])), None, "drawing, no text");
    assert_eq!(pdf_text(&vec![b' '; MAX_FILE + 1]), None);
}

#[test]
fn the_text_kept_is_capped() {
    let line = "BT /F1 1 Tf 0 0 Td (0123456789012345678901234567890123456789) Tj ET\n";
    let page = line.repeat(500);
    let pages: Vec<&str> = std::iter::repeat_n(page.as_str(), 30).collect();
    let text = pdf_text(&pdf(&pages)).unwrap();
    assert_eq!(text.chars().count(), MAX_CHARS);
}

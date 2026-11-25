//! A file attached to a message, read as text for MailCraft's AI: its first byte says what it is
//! (a PDF, or a Word, Excel or PowerPoint file), the rest is the file. Whatever it holds, reading
//! ends without a crash and keeps no more than its caps.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mailcraft_mime::office;

fuzz_target!(|data: &[u8]| {
    let Some((kind, file)) = data.split_first() else { return };
    let text = match kind % 4 {
        0 => mailcraft_mime::pdf_text(file),
        1 => office::preview("a.docx", "", file).map(|p| p.text()),
        2 => office::preview("a.xlsx", "", file).map(|p| p.text()),
        _ => office::preview("a.pptx", "", file).map(|p| p.text()),
    };
    assert!(text.is_none_or(|t| t.chars().count() <= 4_000_000));
});

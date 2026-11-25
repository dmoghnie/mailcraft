//! A PDF's text, read for MailCraft's AI: each page interpreted as for drawing (hayro), with a
//! device that keeps only what each glyph stands for, in the order drawn. A word ends where the
//! next glyph starts well past the last one's end, a line where the next one is on another
//! baseline. Everything here comes from mail: the file, the pages and the text kept are capped,
//! and a panic in the interpreter is caught and comes back as no text.

use std::sync::Arc;

use hayro_interpret::font::Glyph;
use hayro_interpret::font::GlyphRun;
use hayro_interpret::hayro_syntax::Pdf;
use hayro_interpret::kurbo::{Affine, BezPath, Point, Rect};
use hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps, InterpreterCache, InterpreterSettings, SoftMask, interpret_page,
};

#[cfg(test)]
mod tests;

/// Largest file read, most pages read, and most characters kept.
const MAX_FILE: usize = 64 << 20;
const MAX_PAGES: usize = 300;
const MAX_CHARS: usize = 400_000;
/// A glyph's advance when its font gives none, in thousandths of an em (glyph space).
const ADVANCE_GUESS: f64 = 500.0;
/// A word's gap, and a line's change of baseline, as parts of an em.
const WORD_GAP: f64 = 0.2;
const LINE_GAP: f64 = 0.5;

/// The text of `data`, a PDF: its pages' in order, a blank line between them; none when it
/// isn't one, has no text (a scan), or the interpreter gives up.
pub fn pdf_text(data: &[u8]) -> Option<String> {
    if data.len() > MAX_FILE {
        return None;
    }
    let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read(data)));
    read.ok().flatten().filter(|t| !t.trim().is_empty())
}

fn read(data: &[u8]) -> Option<String> {
    let pdf = Pdf::new(Arc::new(data.to_vec())).ok()?;
    let settings = InterpreterSettings::default();
    let cache = InterpreterCache::new();
    let mut device = Text::default();
    for page in pdf.pages().iter().take(MAX_PAGES) {
        if device.chars >= MAX_CHARS {
            break;
        }
        device.page_break();
        let bbox = page.intersected_crop_box();
        let bbox = Rect::new(bbox.x0, bbox.y0, bbox.x1, bbox.y1);
        let mut context = Context::new(Affine::IDENTITY, bbox, &cache, page.xref(), settings.clone());
        interpret_page(page, &mut context, &mut device);
    }
    Some(device.text)
}

/// The text so far, and where the last glyph ended: its end on the line, its baseline, its em.
#[derive(Default)]
struct Text {
    text: String,
    chars: usize,
    last: Option<(Point, f64, f64)>,
}

impl Text {
    fn page_break(&mut self) {
        if !self.text.is_empty() {
            self.push("\n\n");
        }
        self.last = None;
    }

    fn push(&mut self, s: &str) {
        for c in s.chars() {
            if self.chars >= MAX_CHARS {
                return;
            }
            // Control characters stand for nothing a reader sees; a line break is kept.
            let c = if c.is_control() && c != '\n' { ' ' } else { c };
            self.text.push(c);
            self.chars += 1;
        }
    }

    /// A glyph standing for `s`, drawn by `t` (glyph space to the page), `advance` wide.
    fn glyph(&mut self, s: &str, t: Affine, advance: f64) {
        let start = t * Point::ORIGIN;
        let end = t * Point::new(advance, 0.0);
        let up = t * Point::new(0.0, 1000.0);
        let em = start.distance(up);
        if !(em.is_finite() && em > 0.0 && start.x.is_finite() && start.y.is_finite()) {
            return;
        }
        if let Some((last_end, last_y, last_em)) = self.last {
            let scale = em.max(last_em);
            let spaced = self.text.ends_with(char::is_whitespace) || s.starts_with(char::is_whitespace);
            if (start.y - last_y).abs() > LINE_GAP * scale {
                self.push("\n");
            } else if !spaced && (start.x - last_end.x).abs() > WORD_GAP * scale {
                self.push(" ");
            }
        }
        self.push(s);
        self.last = Some((end, start.y, em));
    }
}

impl<'a> Device<'a> for Text {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, _: &DrawMode) {
        for g in run.glyphs() {
            let Some(s) = g.as_unicode() else { continue };
            let advance = match &**g {
                Glyph::Outline(o) => o.advance_width(),
                Glyph::Type3(t3) => t3.advance_width(),
            };
            let s = match s {
                hayro_interpret::hayro_cmap::BfString::Char(c) => c.to_string(),
                hayro_interpret::hayro_cmap::BfString::String(s) => s,
            };
            self.glyph(&s, props.transform * g.transform(), advance.map_or(ADVANCE_GUESS, f64::from));
        }
    }
    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

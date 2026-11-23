//! The assistant pane's own tests: what an answer's Markdown is shown as.

use super::markdown::pictures_as_links;

/// Every picture in an answer, however it's written, is shown as a link: nothing is fetched.
/// Code and escaped text are left as they are.
#[test]
fn an_answers_pictures_are_links_and_nothing_is_fetched() {
    let images = |md: &str| {
        pulldown_cmark::Parser::new_ext(md, pulldown_cmark::Options::all())
            .filter(|e| matches!(e, pulldown_cmark::Event::Start(pulldown_cmark::Tag::Image { .. })))
            .count()
    };
    for md in [
        "![logo](https://tracker.example/p.png)",
        "See !![x](https://a.example/y.png) and ![](file:///etc/passwd)",
        "!!!![x](y)",
        "[![badge](https://b.example/b.svg)](https://c.example)",
        "![ref][r]\n\n[r]: https://d.example/i.png",
        "- ![a](b)\n- > ![c](d)\n\n| x |\n|---|\n| ![e](f) |",
        "\\\\![x](y)",
    ] {
        assert!(images(md) > 0, "{md} has a picture");
        let shown = pictures_as_links(md);
        assert_eq!(images(&shown), 0, "{md} → {shown}");
    }
    assert_eq!(pictures_as_links("![a](b)"), "\u{200B}[a](b)");
    for kept in ["`![a](b)`", "```\n![a](b)\n```", "\\![a](b)", "Wow! [a](b)", "plain text"] {
        assert_eq!(pictures_as_links(kept), kept);
    }
}

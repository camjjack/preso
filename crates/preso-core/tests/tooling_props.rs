//! Property tests for the source-tooling modules (`outline`, `lint`,
//! `edit`, `complete`, `hover`): arbitrary input must never panic them, and every
//! code action must produce a deck the parser reads the way the action
//! promises.

use preso_core::edit::{self, Action};
use preso_core::{complete, hover, lint, outline, parser, style};
use proptest::prelude::*;

/// Lines that exercise the format's edge cases: delimiters, fences, notes,
/// math, directives, frontmatter closers, non-ASCII.
const VOCAB: &[&str] = &[
    "---",
    "---",
    "",
    "",
    "# Title",
    "## Sub ##",
    "text",
    "- item",
    "***",
    "```",
    "~~~~",
    "$$",
    "...",
    "title: x",
    "<!-- layout: TwoColumn -->",
    "<!-- layout: Content -->",
    "<!-- slide: kind=title -->",
    "<!-- slide: hidden -->",
    "<!-- slide: number=5 align=top -->",
    "<!-- note: one line -->",
    "<!-- note: opens",
    "-->",
    "<!-- pause -->",
    "<!-- highlight: rect w=10 h=10 -->",
    "<!-- include: x.md -->",
    "<!--layuot: x-->",
    "![a](b.png)",
    "![a](b.png) label",
    "é\u{a0}ünïcode ![x](ü.png)",
    "   ",
    "![a](a.png){width=50% boarder}",
    "![b](b.png){align=center}",
    "| h1 | h2 |",
    "|---|:--:|",
    "| x | y |",
    "<!-- table: size=24 -->",
    "```rust {2,4-5 size=20}",
    "```mermaid",
    "```rust {all|2|9 zoom sise=3}",
    "```mermaid {all|Parse, Pai zoom}",
    "<!-- zoom[1]: 40%,20%,2x -->",
    "<!-- zoom: banana -->",
    "<!-- highlight[2]: ellipse x=1 y=1 w=5 h=5 spotlight -->",
    "<!-- image: a.png width=wide opacity=0.5 -->",
    "<!-- note[x]: hm -->",
    "<!-- footnote: -->",
    "<!-- slide: transition=pan-up fit=contain -->",
    "$$ x^2 $$",
    "theme: themes/t.toml",
    "transition: fdae",
    "presenter: \"notes\"",
];

fn deck() -> impl Strategy<Value = String> {
    (
        prop::collection::vec(prop::sample::select(VOCAB), 0..40),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(lines, crlf, final_newline)| {
            let eol = if crlf { "\r\n" } else { "\n" };
            let mut s = lines.join(eol);
            if final_newline && !s.is_empty() {
                s.push_str(eol);
            }
            s
        })
}

fn line_count(src: &str) -> usize {
    src.lines().count()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn tooling_never_panics_on_any_text(src in "\\PC*", line in 0usize..8, col in 0usize..40) {
        let _ = outline::outline(&src);
        let _ = lint::check(&src);
        let _ = complete::context(&src, line, col);
        let _ = hover::at(&src, line, col);
        for e in edit::actions(&src, line) {
            let _ = edit::apply(&src, &e.edits);
        }
    }

    #[test]
    fn completion_never_panics_anywhere(src in deck()) {
        for (i, text) in src.lines().enumerate() {
            for col in 0..=text.len() + 1 {
                let _ = complete::context(&src, i, col);
            }
        }
    }

    #[test]
    fn hovers_point_inside_their_lines(src in deck()) {
        for (i, text) in src.lines().enumerate() {
            for col in 0..=text.len() + 1 {
                if let Some(h) = hover::at(&src, i, col) {
                    prop_assert!(h.cols.start <= h.cols.end && h.cols.end <= text.len());
                    prop_assert!(text.is_char_boundary(h.cols.start) && text.is_char_boundary(h.cols.end));
                }
            }
        }
    }

    #[test]
    fn findings_and_references_point_inside_their_lines(src in deck()) {
        let lines: Vec<&str> = src.lines().collect();
        let a = lint::check(&src);
        for f in &a.findings {
            prop_assert!(f.cols.end <= lines[f.line].len(), "{f:?}");
            prop_assert!(lines[f.line].get(f.cols.clone()).is_some(), "{f:?}");
        }
        for r in &a.references {
            prop_assert_eq!(&lines[r.line][r.cols.clone()], r.path.as_str());
        }
    }

    #[test]
    fn actions_keep_their_promises(src in deck(), line in 0usize..40) {
        let before = outline::outline(&src);
        let parses = parser::parse(&src).is_ok();
        for e in edit::actions(&src, line) {
            // Edits stay in bounds and don't overlap.
            let mut ranges: Vec<_> = e.edits.iter().map(|x| x.lines.clone()).collect();
            ranges.sort_by_key(|r| (r.start, r.end));
            for r in &ranges {
                prop_assert!(r.start <= r.end && r.end <= line_count(&src), "{e:?}");
            }
            for w in ranges.windows(2) {
                prop_assert!(w[0].end <= w[1].start, "overlap in {:?}", e);
            }

            let out = edit::apply(&src, &e.edits);
            prop_assert_eq!(parser::parse(&out).is_ok(), parses, "{:?} → {:?}", e.action, out);
            let after = outline::outline(&out);
            let (n0, n1) = (before.slides.len(), after.slides.len());
            let visible = |o: &outline::Outline| o.slides.iter().filter(|s| !s.hidden()).count();
            match e.action {
                Action::InsertSlideAfter | Action::DuplicateSlide => {
                    prop_assert_eq!(n1, n0 + 1, "{:?} → {:?}", e.action, out);
                }
                Action::HideSlide => prop_assert_eq!(visible(&after) + 1, visible(&before)),
                Action::UnhideSlide => prop_assert_eq!(visible(&after), visible(&before) + 1),
                _ => prop_assert_eq!(n1, n0, "{:?} → {:?}", e.action, out),
            }
            if e.action == Action::TwoColumns {
                // The slide really splits.
                let index = before.slide_at(line).unwrap();
                let slide = &parser::parse(&out).map(|d| d.slides).unwrap_or_default();
                let visible_index = before.slides[..index].iter().filter(|s| !s.hidden()).count();
                if !before.slides[index].hidden() && parses {
                    let s = &slide[visible_index];
                    prop_assert!(
                        s.columns_at(s.steps.len() - 1).is_some(),
                        "no split in {:?}", out
                    );
                }
            }
            if !src.contains('\r') {
                prop_assert!(!out.contains('\r'));
            }
        }
    }

    #[test]
    fn inserted_media_lands_on_the_slide(src in deck(), line in 0usize..40, which in 0usize..3) {
        let (action, path) = [
            (Action::InsertImage, "shot.png"),
            (Action::BackgroundImage, "bg.jpg"),
            (Action::InsertVideo, "clip.mp4"),
        ][which];
        let Some(e) = edit::insert_media(&src, line, action, path) else {
            return Ok(());
        };
        let out = edit::apply(&src, &e.edits);
        let (before, after) = (outline::outline(&src), outline::outline(&out));
        prop_assert_eq!(before.slides.len(), after.slides.len(), "{:?}", out);
        prop_assert_eq!(parser::parse(&out).is_ok(), parser::parse(&src).is_ok());
        // The file is referenced from the slide the cursor was on.
        let index = before.slide_at(line).unwrap();
        let lines = after.slides[index].lines.clone();
        let refs = lint::check(&out).references;
        prop_assert!(
            refs.iter().any(|r| r.path == path && lines.contains(&r.line)),
            "{:?} not on slide {} of {:?}", path, index, out
        );
    }

    #[test]
    fn styles_keep_the_deck_intact(src in deck(), line in 0usize..40, col in 0usize..40) {
        let before = outline::outline(&src).slides.len();
        let parses = parser::parse(&src).is_ok();
        for s in style::styles(&src, line, col) {
            let out = preso_core::edit::apply(&src, &s.edits);
            prop_assert_eq!(outline::outline(&out).slides.len(), before, "{} → {:?}", s.title, out);
            prop_assert_eq!(parser::parse(&out).is_ok(), parses);
            prop_assert!(s.kind.starts_with("refactor.preso."));
        }
    }

    #[test]
    fn moving_down_then_up_is_the_identity(src in deck(), line in 0usize..40) {
        let Some(down) = edit::actions(&src, line)
            .into_iter()
            .find(|e| e.action == Action::MoveSlideDown)
        else {
            return Ok(());
        };
        let moved = edit::apply(&src, &down.edits);
        let index = outline::outline(&src).slide_at(line).unwrap();
        let anchor = outline::outline(&moved).slides[index + 1].anchor();
        let up = edit::actions(&moved, anchor)
            .into_iter()
            .find(|e| e.action == Action::MoveSlideUp)
            .expect("the moved slide can move back");
        prop_assert_eq!(edit::apply(&moved, &up.edits), src);
    }
}

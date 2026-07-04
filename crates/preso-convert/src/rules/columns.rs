//! Two-column slot separators → preso's `***` split.
//!
//! Slidev's `two-cols` uses `::right::` (with `::left::` for the left slot and
//! `::title::` for the `two-cols-header` header). preso splits a TwoColumn
//! slide at a single `***` line, with no header slot — so `::right::` becomes
//! `***`, `::left::` is dropped, and `::title::` content stays inline above
//! the columns (with a warning).

use super::{Rule, SlideCtx};

pub struct Columns;

impl Rule for Columns {
    fn apply(&self, ctx: &mut SlideCtx) {
        if !ctx.body.contains("::") {
            return;
        }
        let mut out = String::new();
        let mut right_count = 0;
        let mut had_title = false;
        for line in ctx.body.lines() {
            match line.trim() {
                "::right::" => {
                    right_count += 1;
                    // preso splits at the first `***`; collapse extra slots.
                    out.push_str(if right_count == 1 { "***" } else { "" });
                    out.push('\n');
                }
                "::left::" => {} // left is the default first column
                "::title::" => had_title = true,
                _ => {
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
        if right_count > 1 {
            ctx.warn(format!(
                "{right_count} `::right::` slots collapsed to one `***` (preso has two columns)"
            ));
        }
        if had_title {
            ctx.warn("`::title::` header kept inline above the columns");
        }
        ctx.body = out.trim_end().to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_norway::Mapping;

    #[test]
    fn right_slot_becomes_star_split() {
        let mut c = SlideCtx {
            index: 0,
            front: Mapping::new(),
            overrides: vec![],
            layout: Some("TwoColumn".into()),
            body: "left side\n\n::right::\n\nright side\n".into(),
            notes: vec![],
            warnings: vec![],
        };
        Columns.apply(&mut c);
        assert!(c.body.contains("***"));
        assert!(!c.body.contains("::right::"));
        assert_eq!(c.body.matches("***").count(), 1);
    }
}

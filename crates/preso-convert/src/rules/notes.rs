//! Speaker notes: Slidev uses the slide's trailing `<!-- … -->` comment.
//! preso uses an explicit `<!-- note: … -->`, so we lift the trailing comment
//! out of the body into [`SlideCtx::notes`].

use super::{Rule, SlideCtx};

pub struct Notes;

impl Rule for Notes {
    fn apply(&self, ctx: &mut SlideCtx) {
        let trimmed = ctx.body.trim_end();
        if !trimmed.ends_with("-->") {
            return;
        }
        // The comment must be the last thing on the slide; find its opening.
        let Some(open) = trimmed.rfind("<!--") else {
            return;
        };
        let close = trimmed.len() - "-->".len();
        if open >= close {
            return;
        }
        let inner = trimmed[open + "<!--".len()..close].trim();
        // Don't treat an actual preso directive as a note (defensive — a
        // converted deck shouldn't contain these yet, but be safe).
        if inner.starts_with("note")
            || inner.starts_with("slide:")
            || inner.starts_with("layout:")
            || inner == "pause"
        {
            return;
        }
        if !inner.is_empty() {
            ctx.notes.push(inner.to_string());
        }
        ctx.body = trimmed[..open].trim_end().to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_norway::Mapping;

    fn ctx(body: &str) -> SlideCtx {
        SlideCtx {
            index: 0,
            front: Mapping::new(),
            overrides: vec![],
            layout: None,
            body: body.to_string(),
            notes: vec![],
            warnings: vec![],
        }
    }

    #[test]
    fn extracts_trailing_comment() {
        let mut c = ctx("# Title\n\nbody\n\n<!-- remember the demo -->\n");
        Notes.apply(&mut c);
        assert_eq!(c.notes, vec!["remember the demo".to_string()]);
        assert!(!c.body.contains("remember"));
        assert!(c.body.contains("# Title"));
    }

    #[test]
    fn leaves_body_without_trailing_comment() {
        let mut c = ctx("# Title\n\njust body\n");
        Notes.apply(&mut c);
        assert!(c.notes.is_empty());
    }
}

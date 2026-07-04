//! Fenced-code-block tracking, shared by every line scanner that must treat
//! fence content as literal (slide splitting, directive stripping, column
//! splitting, list grouping — and `include` expansion in the app).
//!
//! CommonMark rules: a fence is 3+ backticks or tildes after at most 3
//! spaces of indentation, closed only by a fence of the *same character at
//! least as long*. Keeping one implementation here prevents the drift where
//! a naive `starts_with("```")` toggle mis-closes a ```` fence on its inner
//! ``` lines.

/// An open fence: its marker character (`` ` `` or `~`) and length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fence {
    pub ch: char,
    pub len: usize,
}

/// If this line opens (or closes) a code fence, return its marker.
/// CommonMark: up to 3 leading spaces, then 3+ backticks or tildes.
pub fn marker(line: &str) -> Option<Fence> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let ch = rest.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let len = rest.chars().take_while(|&c| c == ch).count();
    if len < 3 {
        return None;
    }
    // An opening backtick fence cannot contain backticks in its info string;
    // such a line is not a fence at all (CommonMark §119).
    if ch == '`' && rest[len..].contains('`') {
        return None;
    }
    Some(Fence { ch, len })
}

/// Line-by-line fence state for a scanner walking a document top to bottom.
#[derive(Debug, Default)]
pub struct Tracker {
    open: Option<Fence>,
}

impl Tracker {
    /// Whether the tracker is currently inside an (unclosed) fence.
    pub fn in_fence(&self) -> bool {
        self.open.is_some()
    }

    /// Feed the next line. Returns `true` when the line belongs to fenced
    /// code — the opening fence line, its content, or the closing fence
    /// line — i.e. whenever the caller must treat it as literal text.
    pub fn process(&mut self, line: &str) -> bool {
        match self.open {
            Some(open) => {
                if let Some(close) = marker(line)
                    && close.ch == open.ch
                    && close.len >= open.len
                {
                    self.open = None;
                }
                true
            }
            None => {
                if let Some(fence) = marker(line) {
                    self.open = Some(fence);
                    true
                } else {
                    false
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_recognizes_fences() {
        assert_eq!(marker("```"), Some(Fence { ch: '`', len: 3 }));
        assert_eq!(marker("````rust"), Some(Fence { ch: '`', len: 4 }));
        assert_eq!(marker("~~~"), Some(Fence { ch: '~', len: 3 }));
        assert_eq!(marker("   ```"), Some(Fence { ch: '`', len: 3 }));
        // Not fences: too short, too indented, backtick in the info string.
        assert_eq!(marker("``"), None);
        assert_eq!(marker("    ```"), None);
        assert_eq!(marker("``` a`b"), None);
        assert_eq!(marker("text"), None);
    }

    #[test]
    fn tracker_requires_matching_closer() {
        let mut t = Tracker::default();
        assert!(t.process("````md")); // opens
        assert!(t.process("```")); // shorter: content, does not close
        assert!(t.in_fence());
        assert!(t.process("~~~~")); // wrong char: content
        assert!(t.in_fence());
        assert!(t.process("````")); // closes
        assert!(!t.in_fence());
        assert!(!t.process("plain text"));
    }
}

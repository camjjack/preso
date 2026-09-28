//! Between preso-core's coordinates (0-based lines of `str::lines`, byte
//! columns) and the protocol's (lines, columns in the negotiated encoding),
//! and between `file://` URIs and paths.

use lsp_types::{Position, Range, Uri};
use std::path::{Path, PathBuf};

/// How the client counts columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    /// The protocol default.
    Utf16,
}

/// A document's line starts, for converting positions both ways.
///
/// Lines break at `\n` (a preceding `\r` belongs to the break), as
/// `str::lines` — which preso-core uses — breaks them. A lone `\r` is not a
/// break here, although the protocol says it is; no editor saves markdown
/// that way.
pub struct LineIndex<'a> {
    text: &'a str,
    starts: Vec<usize>,
    encoding: Encoding,
}

impl<'a> LineIndex<'a> {
    pub fn new(text: &'a str, encoding: Encoding) -> Self {
        let starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        LineIndex {
            text,
            starts,
            encoding,
        }
    }

    /// Line `line`'s text without its line break (empty past the end).
    pub fn line(&self, line: usize) -> &'a str {
        let Some(&start) = self.starts.get(line) else {
            return "";
        };
        let end = self
            .starts
            .get(line + 1)
            .map_or(self.text.len(), |&n| n - 1);
        let text = &self.text[start..end];
        text.strip_suffix('\r').unwrap_or(text)
    }

    /// The protocol position of byte column `col` on `line`.
    pub fn position(&self, line: usize, col: usize) -> Position {
        let text = self.line(line);
        let mut col = col.min(text.len());
        while !text.is_char_boundary(col) {
            col -= 1;
        }
        let character = match self.encoding {
            Encoding::Utf8 => col,
            Encoding::Utf16 => text[..col].encode_utf16().count(),
        };
        Position::new(to_u32(line), to_u32(character))
    }

    /// The byte column on `position.line` that `position` names.
    pub fn byte_col(&self, position: Position) -> usize {
        let text = self.line(position.line as usize);
        let target = position.character as usize;
        match self.encoding {
            Encoding::Utf8 => {
                let mut col = target.min(text.len());
                while !text.is_char_boundary(col) {
                    col -= 1;
                }
                col
            }
            Encoding::Utf16 => {
                let mut units = 0;
                for (i, c) in text.char_indices() {
                    if units >= target {
                        return i;
                    }
                    units += c.len_utf16();
                }
                text.len()
            }
        }
    }

    pub fn range(&self, line: usize, cols: std::ops::Range<usize>) -> Range {
        Range::new(
            self.position(line, cols.start),
            self.position(line, cols.end),
        )
    }

    /// Whole lines `lines` — or through the end of the document when they
    /// run to the last line, so the range's end is always a valid position.
    pub fn line_range(&self, lines: std::ops::Range<usize>) -> Range {
        let end = if lines.end < self.text.lines().count() {
            Position::new(to_u32(lines.end), 0)
        } else {
            self.end()
        };
        Range::new(Position::new(to_u32(lines.start), 0), end)
    }

    /// The position just past the last character.
    pub fn end(&self) -> Position {
        let last = self.starts.len() - 1;
        self.position(last, usize::MAX)
    }
}

fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The local path a `file://` URI names; `None` for any other scheme.
pub fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    let rest = uri.as_str().strip_prefix("file://")?;
    // An authority, if any, is only ever `localhost` for a local file.
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let decoded = percent_decode(rest)?;
    // `/C:/Users/…` → `C:/Users/…`
    let decoded = if cfg!(windows) && decoded.as_bytes().get(2) == Some(&b':') {
        decoded[1..].to_string()
    } else {
        decoded
    };
    Some(PathBuf::from(decoded))
}

/// A `file://` URI for an absolute path.
pub fn path_to_uri(path: &Path) -> Option<Uri> {
    let text = path.to_str()?.replace('\\', "/");
    let mut encoded = String::from("file://");
    if !text.starts_with('/') {
        encoded.push('/'); // a Windows drive path
    }
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded.parse().ok()
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_columns_count_surrogate_pairs() {
        let idx = LineIndex::new("a😀b\nx", Encoding::Utf16);
        // 😀 is 4 bytes, 2 UTF-16 units.
        assert_eq!(idx.position(0, 5), Position::new(0, 3));
        assert_eq!(idx.byte_col(Position::new(0, 3)), 5);
        let idx = LineIndex::new("a😀b\nx", Encoding::Utf8);
        assert_eq!(idx.position(0, 5), Position::new(0, 5));
    }

    #[test]
    fn columns_clamp_to_the_line_and_char_boundaries() {
        let idx = LineIndex::new("é\r\n", Encoding::Utf8);
        assert_eq!(idx.line(0), "é");
        assert_eq!(idx.position(0, 1), Position::new(0, 0));
        assert_eq!(idx.position(0, 99), Position::new(0, 2));
        assert_eq!(idx.byte_col(Position::new(0, 1)), 0);
        assert_eq!(idx.position(7, 3), Position::new(7, 0));
    }

    #[test]
    fn line_ranges_end_at_the_document_end() {
        let idx = LineIndex::new("a\nb\n", Encoding::Utf16);
        assert_eq!(idx.line_range(0..1).end, Position::new(1, 0));
        assert_eq!(idx.line_range(1..2).end, Position::new(2, 0));
        let idx = LineIndex::new("a\nbc", Encoding::Utf16);
        assert_eq!(idx.line_range(1..2).end, Position::new(1, 2));
        // Appending to a terminated file: an empty range at the end.
        let idx = LineIndex::new("a\n", Encoding::Utf16);
        let r = idx.line_range(1..1);
        assert_eq!((r.start, r.end), (Position::new(1, 0), Position::new(1, 0)));
    }

    #[test]
    fn file_uris_round_trip() {
        let path = Path::new("/Users/me/My Deck/talk ü.md");
        let uri = path_to_uri(path).unwrap();
        assert_eq!(uri.as_str(), "file:///Users/me/My%20Deck/talk%20%C3%BC.md");
        assert_eq!(uri_to_path(&uri).unwrap(), path);
        let vscode: Uri = "file:///c%3A/deck/talk.md".parse().unwrap();
        let expected = if cfg!(windows) {
            "c:/deck/talk.md"
        } else {
            "/c:/deck/talk.md"
        };
        assert_eq!(uri_to_path(&vscode).unwrap(), PathBuf::from(expected));
        let untitled: Uri = "untitled:Untitled-1".parse().unwrap();
        assert_eq!(uri_to_path(&untitled), None);
    }
}

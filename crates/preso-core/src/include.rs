//! Master-deck includes.
//!
//! `<!-- include: path.md -->` on its own line splices another markdown file
//! in place, resolved relative to the including file. Expansion runs before
//! parsing, so the parser only ever sees one combined document — slides from
//! an included file are delimited by its own `---` lines, exactly as if pasted
//! in. Includes are recursive (a chapter can include sub-chapters) with cycle
//! detection, and an included file's leading frontmatter is dropped so a
//! chapter file can carry its own (for standalone preview) without polluting
//! the master deck.

use crate::fence;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Guards against pathological nesting even if cycle detection is somehow
/// evaded (e.g. via symlinks that don't canonicalize equal).
const MAX_DEPTH: usize = 32;

#[derive(Debug, Error)]
pub enum IncludeError {
    #[error("cannot include {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("circular include: {path}")]
    Circular { path: PathBuf },

    #[error("include nesting too deep (cyclic include?)")]
    TooDeep,
}

/// Expand every `<!-- include: … -->` directive in `source`, resolving paths
/// relative to `base_dir`. Returns the combined markdown.
pub fn expand(source: &str, base_dir: &Path) -> Result<String, IncludeError> {
    let mut stack = Vec::new();
    expand_inner(source, base_dir, &mut stack, 0)
}

fn expand_inner(
    source: &str,
    dir: &Path,
    stack: &mut Vec<PathBuf>,
    depth: usize,
) -> Result<String, IncludeError> {
    if depth > MAX_DEPTH {
        return Err(IncludeError::TooDeep);
    }

    let mut out = String::new();
    let mut fence = fence::Tracker::default();
    for line in source.lines() {
        // Inside a fence, everything is literal — including `<!-- include -->`.
        if fence.process(line) {
            out.push_str(line);
            out.push('\n');
            continue;
        }

        if let Some(rel) = parse_include(line.trim()) {
            let path = dir.join(rel);
            let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
            if stack.contains(&canon) {
                return Err(IncludeError::Circular { path });
            }

            let child = std::fs::read_to_string(&path).map_err(|source| IncludeError::Read {
                path: path.clone(),
                source,
            })?;
            let body = strip_frontmatter(&child);
            let child_dir = path.parent().unwrap_or(dir);

            stack.push(canon);
            let expanded = expand_inner(&body, child_dir, stack, depth + 1)?;
            stack.pop();

            // Blank lines around the splice so the chapter never merges into
            // a neighbouring line.
            out.push('\n');
            out.push_str(expanded.trim());
            out.push('\n');
            continue;
        }

        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

/// `<!-- include: path -->` → `Some("path")`, else `None`.
fn parse_include(line: &str) -> Option<&str> {
    let inner = line.strip_prefix("<!-- include:")?.strip_suffix("-->")?;
    let path = inner.trim();
    (!path.is_empty()).then_some(path)
}

/// Drop a leading YAML frontmatter block (`---` … `---`/`...`) so only the
/// master deck's frontmatter applies. Returns the source unchanged if it
/// doesn't open with a closed frontmatter block.
fn strip_frontmatter(source: &str) -> String {
    let mut lines = source.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return source.to_string();
    }
    let mut body = Vec::new();
    let mut closed = false;
    for line in lines {
        if !closed {
            if matches!(line.trim_end(), "---" | "...") {
                closed = true;
            }
            continue;
        }
        body.push(line);
    }
    if closed {
        body.join("\n")
    } else {
        // Opened with `---` but never closed: not frontmatter, keep as-is.
        source.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("preso-include-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn splices_child_and_strips_its_frontmatter() {
        let dir = tmp();
        fs::write(
            dir.join("intro.md"),
            "---\ntheme: light\n---\n\n# Intro\n\nbody\n",
        )
        .unwrap();
        let master = "---\ntitle: M\n---\n\n# Title\n\n---\n\n<!-- include: intro.md -->\n";
        let out = expand(master, &dir).unwrap();
        assert!(out.contains("title: M")); // master frontmatter kept
        assert!(out.contains("# Intro")); // child body spliced
        assert!(!out.contains("theme: light")); // child frontmatter dropped
    }

    #[test]
    fn nested_includes_resolve_relative_to_each_file() {
        let dir = tmp();
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("a.md"), "# A\n\n<!-- include: sub/b.md -->\n").unwrap();
        fs::write(dir.join("sub/b.md"), "# B\n").unwrap();
        let out = expand("<!-- include: a.md -->\n", &dir).unwrap();
        assert!(out.contains("# A") && out.contains("# B"));
    }

    #[test]
    fn detects_cycles() {
        let dir = tmp();
        fs::write(dir.join("x.md"), "# X\n\n<!-- include: y.md -->\n").unwrap();
        fs::write(dir.join("y.md"), "# Y\n\n<!-- include: x.md -->\n").unwrap();
        let err = expand("<!-- include: x.md -->\n", &dir).unwrap_err();
        assert!(matches!(err, IncludeError::Circular { .. }), "got: {err}");
    }

    #[test]
    fn missing_file_is_an_error() {
        let dir = tmp();
        let err = expand("<!-- include: nope.md -->\n", &dir).unwrap_err();
        assert!(err.to_string().contains("cannot include"));
    }

    #[test]
    fn include_inside_code_fence_is_literal() {
        let dir = tmp();
        let src = "```markdown\n<!-- include: nope.md -->\n```\n";
        let out = expand(src, &dir).unwrap();
        assert!(out.contains("<!-- include: nope.md -->"));
    }

    #[test]
    fn no_includes_is_unchanged_ish() {
        let dir = tmp();
        let out = expand("# Hello\n\nworld\n", &dir).unwrap();
        assert!(out.contains("# Hello") && out.contains("world"));
    }
}

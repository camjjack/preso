//! Which directory a file's asset paths resolve against.
//!
//! preso resolves every image, video and background path against the
//! directory of the deck it was *launched* on, even inside a chapter pulled
//! in with `<!-- include: … -->`; only an include's own path is relative to
//! the file holding it. So a chapter at `day1/intro.md` writing
//! `![](assets/x.png)` means `<master's dir>/assets/x.png`. A language server
//! sees one file at a time, so it has to find that master: the nearest file
//! whose includes name this one, repeated until nothing includes the last.

use preso_core::lint::{self, RefKind};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// How many directories to search for an including deck: the file's own,
/// its parent and grandparent. Chapters sit beside or under their master.
const MAX_LEVELS: usize = 3;

/// Files larger than this aren't decks worth reading for includes.
const MAX_DECK_BYTES: u64 = 4 * 1024 * 1024;

/// Most files one deck walk reads, however its includes are arranged.
const MAX_DECK_FILES: usize = 500;

/// The directory `file`'s asset paths resolve against: its top-level
/// deck's, or its own when nothing includes it.
pub fn media_dir(file: &Path) -> Option<PathBuf> {
    master(file).parent().map(Path::to_path_buf)
}

/// The top-level deck `file` belongs to: itself when nothing includes it.
fn master(file: &Path) -> PathBuf {
    let mut current = canonical(file);
    let mut seen = HashSet::from([current.clone()]);
    while let Some(master) = includer(&current) {
        if !seen.insert(master.clone()) {
            break; // An include cycle; preso refuses those anyway.
        }
        current = master;
    }
    current
}

/// Every file the deck containing `file` references, as canonical paths —
/// the whole deck, from its top-level file through every include, so a
/// chapter knows what its siblings already use. `read` supplies each file's
/// text, so an open buffer's unsaved edits can count over what's on disk.
pub fn deck_references(file: &Path, read: impl Fn(&Path) -> Option<String>) -> HashSet<PathBuf> {
    let master = master(file);
    let media = master.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut used = HashSet::new();
    let mut seen = HashSet::new();
    let mut queue = vec![master];
    while let Some(path) = queue.pop() {
        if seen.len() >= MAX_DECK_FILES || !seen.insert(path.clone()) {
            continue;
        }
        let Some(text) = read(&path).or_else(|| std::fs::read_to_string(&path).ok()) else {
            continue;
        };
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        for r in lint::check(&text).references {
            if r.kind == RefKind::Include {
                queue.push(canonical(&dir.join(&r.path)));
            } else {
                // As the deck resolves it, or as the chapter previewed alone.
                for base in [&media, &dir] {
                    if let Ok(p) = base.join(&r.path).canonicalize() {
                        used.insert(p);
                    }
                }
            }
        }
    }
    used
}

/// A markdown file near `file` that includes it.
fn includer(file: &Path) -> Option<PathBuf> {
    for dir in file.parent()?.ancestors().take(MAX_LEVELS) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut candidates: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "md") && p != file)
            .filter(|p| {
                std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() <= MAX_DECK_BYTES)
            })
            .collect();
        candidates.sort(); // Deterministic when two decks include one chapter.
        for candidate in candidates {
            let Ok(text) = std::fs::read_to_string(&candidate) else {
                continue;
            };
            if !text.contains("include:") {
                continue;
            }
            let base = candidate.parent().unwrap_or(dir);
            let includes_file = lint::check(&text)
                .references
                .iter()
                .filter(|r| r.kind == RefKind::Include)
                .any(|r| canonical(&base.join(&r.path)) == file);
            if includes_file {
                return Some(canonical(&candidate));
            }
        }
    }
    None
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tree(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("preso-lsp-roots-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("day1/extra")).unwrap();
        canonical(&root)
    }

    #[test]
    fn chapters_resolve_against_their_top_level_deck() {
        let root = tree("nested");
        fs::write(
            root.join("course.md"),
            "# Course\n\n<!-- include: day1/intro.md -->\n",
        )
        .unwrap();
        fs::write(
            root.join("day1/intro.md"),
            "# Intro\n\n<!-- include: extra/deep.md -->\n",
        )
        .unwrap();
        fs::write(root.join("day1/extra/deep.md"), "# Deep\n").unwrap();
        fs::write(
            root.join("unrelated.md"),
            "# Other\n\n<!-- include: nope.md -->\n",
        )
        .unwrap();

        assert_eq!(
            media_dir(&root.join("day1/extra/deep.md")),
            Some(root.clone())
        );
        assert_eq!(media_dir(&root.join("day1/intro.md")), Some(root.clone()));
        assert_eq!(media_dir(&root.join("course.md")), Some(root.clone()));
    }

    #[test]
    fn deck_references_span_every_chapter() {
        let root = tree("refs");
        fs::write(
            root.join("course.md"),
            "![](a.png)\n\n<!-- include: day1/intro.md -->\n",
        )
        .unwrap();
        fs::write(
            root.join("day1/intro.md"),
            "![](b.png)\n<!-- video: c.mp4 -->\n",
        )
        .unwrap();
        for f in ["a.png", "b.png", "c.mp4", "unused.png"] {
            fs::write(root.join(f), b"").unwrap();
        }
        let names = |used: HashSet<PathBuf>| -> Vec<String> {
            let mut v: Vec<String> = used
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            v.sort();
            v
        };
        // From the chapter: its siblings' references count too.
        let used = deck_references(&root.join("day1/intro.md"), |_| None);
        assert_eq!(names(used), vec!["a.png", "b.png", "c.mp4"]);
        // An open buffer's unsaved text wins over the disk.
        let chapter = root.join("day1/intro.md");
        let used = deck_references(&chapter, |p| {
            (p == chapter).then(|| "![](unused.png)\n".to_string())
        });
        assert_eq!(names(used), vec!["a.png", "unused.png"]);
    }

    #[test]
    fn a_file_nothing_includes_is_its_own_root() {
        let root = tree("alone");
        fs::write(root.join("day1/solo.md"), "# Solo\n").unwrap();
        assert_eq!(
            media_dir(&root.join("day1/solo.md")),
            Some(root.join("day1"))
        );
    }

    #[test]
    fn include_cycles_terminate() {
        let root = tree("cycle");
        fs::write(root.join("a.md"), "<!-- include: b.md -->\n").unwrap();
        fs::write(root.join("b.md"), "<!-- include: a.md -->\n").unwrap();
        assert_eq!(media_dir(&root.join("a.md")), Some(root.clone()));
    }

    #[test]
    fn fenced_includes_do_not_count() {
        let root = tree("fenced");
        fs::write(
            root.join("doc.md"),
            "```md\n<!-- include: day1/ch.md -->\n```\n",
        )
        .unwrap();
        fs::write(root.join("day1/ch.md"), "# Ch\n").unwrap();
        assert_eq!(media_dir(&root.join("day1/ch.md")), Some(root.join("day1")));
    }
}

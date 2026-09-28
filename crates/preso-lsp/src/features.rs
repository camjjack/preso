//! Each LSP feature as a function of one document: preso-core does the
//! analysis, this module maps it onto protocol types and does the
//! filesystem work core leaves to its caller.

use crate::convert::{LineIndex, path_to_uri};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CompletionItem, CompletionItemKind,
    CompletionTextEdit, Diagnostic, DiagnosticSeverity, DocumentLink, DocumentSymbol, FoldingRange,
    FoldingRangeKind, InsertTextFormat, NumberOrString, SymbolKind, TextEdit, Uri, WorkspaceEdit,
};
use preso_core::complete::{self, Context};
use preso_core::edit;
use preso_core::lint::{self, Analysis, RefKind, Severity};
use preso_core::outline;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const SOURCE: &str = "preso";

/// Most entries a path completion lists, so a huge directory can't stall
/// the editor.
const MAX_PATH_ITEMS: usize = 500;

/// A document and where it lives, if on disk.
pub struct Doc<'a> {
    pub uri: &'a Uri,
    pub text: &'a str,
    /// The file's own directory: where its includes resolve.
    pub dir: Option<&'a Path>,
    /// Where its asset paths resolve: its top-level deck's directory (see
    /// [`crate::roots`]).
    pub media_dir: Option<&'a Path>,
    /// Every file the whole deck references, from its last open or save.
    pub deck_used: Option<&'a HashSet<PathBuf>>,
    pub index: LineIndex<'a>,
    /// Draws diagrams to check and complete their zoom labels.
    pub diagrams: &'a crate::diagrams::Diagrams,
}

impl Doc<'_> {
    /// Directories a reference of `kind` may resolve against, preferred
    /// first. An asset path is also accepted relative to the file itself,
    /// which is how a chapter previewed on its own resolves it.
    fn bases(&self, kind: RefKind) -> Vec<&Path> {
        let mut bases: Vec<&Path> = match kind {
            RefKind::Include | RefKind::Theme => vec![],
            _ => self.media_dir.into_iter().collect(),
        };
        bases.extend(self.dir.filter(|d| !bases.contains(d)));
        bases
    }

    /// Where `r` points, if the file is there.
    fn resolve(&self, r: &lint::Reference) -> Option<std::path::PathBuf> {
        // A theme named rather than pathed is looked up in the user's theme
        // folder first, as the app does.
        if r.kind == RefKind::Theme
            && !r.path.contains(['/', '\\'])
            && !r.path.ends_with(".toml")
            && let Some(found) = dirs::config_dir()
                .map(|d| d.join("preso/themes").join(format!("{}.toml", r.path)))
                .filter(|p| p.is_file())
        {
            return Some(found);
        }
        self.bases(r.kind)
            .into_iter()
            .map(|base| base.join(&r.path))
            .find(|p| p.exists())
    }
}

/// The slide outline: each slide named by its first heading, the rest of
/// its headings nested beneath.
pub fn symbols(doc: &Doc) -> Vec<DocumentSymbol> {
    let o = outline::outline(doc.text);
    o.slides
        .iter()
        .map(|slide| {
            let name = match (slide.title(), slide.number) {
                (Some(title), _) => title.to_string(),
                (None, Some(n)) => format!("Slide {n}"),
                (None, None) => "Hidden slide".to_string(),
            };
            let mut detail = vec![match slide.number {
                Some(n) => format!("slide {n}"),
                None => "hidden".to_string(),
            }];
            detail.extend(slide.kind.clone());
            if matches!(slide.layout, preso_core::Layout::TwoColumn { .. }) {
                detail.push("two columns".to_string());
            }
            let titled = slide.title().is_some();
            let children: Vec<DocumentSymbol> = slide
                .headings
                .iter()
                .skip(usize::from(titled))
                .map(|h| {
                    let range = doc.index.range(h.line, 0..usize::MAX);
                    symbol(h.text.clone(), None, SymbolKind::STRING, range, range, None)
                })
                .collect();
            let anchor = doc.index.range(slide.anchor(), 0..usize::MAX);
            let range = doc.index.line_range(slide.content.clone());
            symbol(
                name,
                Some(detail.join(" · ")),
                SymbolKind::MODULE,
                range,
                anchor,
                (!children.is_empty()).then_some(children),
            )
        })
        .collect()
}

#[allow(deprecated)] // `deprecated` must be named; `tags` replaces it.
fn symbol(
    name: String,
    detail: Option<String>,
    kind: SymbolKind,
    range: lsp_types::Range,
    selection_range: lsp_types::Range,
    children: Option<Vec<DocumentSymbol>>,
) -> DocumentSymbol {
    DocumentSymbol {
        name,
        detail,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children,
    }
}

/// One fold per slide, and one for the frontmatter.
pub fn folding(doc: &Doc) -> Vec<FoldingRange> {
    let o = outline::outline(doc.text);
    let fold = |lines: std::ops::Range<usize>| FoldingRange {
        start_line: lines.start as u32,
        end_line: lines.end.saturating_sub(1) as u32,
        kind: Some(FoldingRangeKind::Region),
        ..FoldingRange::default()
    };
    o.frontmatter
        .into_iter()
        .chain(o.slides.iter().map(|s| s.content.clone()))
        .filter(|r| r.len() > 1)
        .map(fold)
        .collect()
}

/// Lint findings, plus a warning for every referenced file that isn't there.
pub fn diagnostics(doc: &Doc, analysis: &Analysis) -> Vec<Diagnostic> {
    let mut out: Vec<Diagnostic> = analysis
        .findings
        .iter()
        .map(|f| Diagnostic {
            range: doc.index.range(f.line, f.cols.clone()),
            severity: Some(match f.severity {
                Severity::Error => DiagnosticSeverity::ERROR,
                Severity::Warning => DiagnosticSeverity::WARNING,
                Severity::Hint => DiagnosticSeverity::HINT,
            }),
            code: Some(NumberOrString::String(f.code.to_string())),
            source: Some(SOURCE.to_string()),
            message: f.message.clone(),
            ..Diagnostic::default()
        })
        .collect();
    // A zoom stage naming a node the diagram doesn't draw leaves it whole.
    for diagram in &analysis.diagrams {
        for (label, cols) in &diagram.labels {
            if doc
                .diagrams
                .finds(&diagram.language, &diagram.source, label)
                == Some(false)
            {
                out.push(Diagnostic {
                    range: doc.index.range(diagram.line, cols.clone()),
                    severity: Some(DiagnosticSeverity::WARNING),
                    code: Some(NumberOrString::String("zoom-label".to_string())),
                    source: Some(SOURCE.to_string()),
                    message: format!(
                        "no node in this diagram is labelled `{label}`, so this stage shows \
                         the whole diagram"
                    ),
                    ..Diagnostic::default()
                });
            }
        }
    }
    if doc.dir.is_some() {
        for r in &analysis.references {
            if doc.resolve(r).is_some() {
                continue;
            }
            let what = match r.kind {
                RefKind::Include => "included file",
                RefKind::Video => "video",
                RefKind::Theme => "theme",
                _ => "image",
            };
            let theme_note = if r.kind == RefKind::Theme {
                " (a theme is a built-in name, a `NAME.toml` in your preso themes folder, \
                 or a path relative to the deck)"
            } else {
                ""
            };
            out.push(Diagnostic {
                range: doc.index.range(r.line, r.cols.clone()),
                severity: Some(DiagnosticSeverity::WARNING),
                code: Some(NumberOrString::String("missing-file".to_string())),
                source: Some(SOURCE.to_string()),
                message: format!("{what} not found: {}{theme_note}", r.path),
                ..Diagnostic::default()
            });
        }
    }
    out
}

/// Quick fixes for findings on the requested lines, then the structural
/// actions available at the cursor — filtered to the kinds the client asked
/// for, if it asked.
pub fn code_actions(
    doc: &Doc,
    analysis: &Analysis,
    range: lsp_types::Range,
    only: Option<&[CodeActionKind]>,
) -> Vec<CodeActionOrCommand> {
    let wanted = |kind: &str| {
        only.is_none_or(|only| {
            only.iter().any(|o| {
                let o = o.as_str();
                kind == o || kind.starts_with(&format!("{o}."))
            })
        })
    };
    let lines = range.start.line as usize..=range.end.line as usize;
    let mut out = Vec::new();

    if wanted(CodeActionKind::QUICKFIX.as_str()) {
        for f in &analysis.findings {
            let Some(fix) = f.fix.as_ref().filter(|_| lines.contains(&f.line)) else {
                continue;
            };
            let edit = TextEdit::new(doc.index.range(f.line, f.cols.clone()), fix.clone());
            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!("Change to `{fix}`"),
                kind: Some(CodeActionKind::QUICKFIX),
                edit: Some(workspace_edit(doc.uri, vec![edit])),
                is_preferred: Some(true),
                ..CodeAction::default()
            }));
        }
    }

    // Styles for what's under the cursor, then layout changes, inserts and
    // slide operations.
    let line = range.start.line as usize;
    let col = doc.index.byte_col(range.start);
    for s in preso_core::style::styles(doc.text, line, col) {
        if !wanted(s.kind) {
            continue;
        }
        let edits = s
            .edits
            .into_iter()
            .map(|x| TextEdit::new(doc.index.line_range(x.lines), x.text))
            .collect();
        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: s.title,
            kind: Some(CodeActionKind::from(s.kind.to_string())),
            edit: Some(workspace_edit(doc.uri, edits)),
            ..CodeAction::default()
        }));
    }
    let (slide_ops, layout): (Vec<edit::Edit>, Vec<edit::Edit>) = edit::actions(doc.text, line)
        .into_iter()
        .partition(|e| e.action.kind().starts_with("refactor.preso.slide."));
    for e in layout {
        push_edit(doc, &mut out, &wanted, e, None);
    }
    let insert_kinds = [
        edit::Action::InsertImage,
        edit::Action::BackgroundImage,
        edit::Action::InsertVideo,
    ];
    if insert_kinds.iter().any(|a| wanted(a.kind())) {
        for (action, path) in media_offers(doc, analysis) {
            if let Some(e) = edit::insert_media(doc.text, line, action, &path) {
                push_edit(doc, &mut out, &wanted, e, Some(&path));
            }
        }
    }
    for e in slide_ops {
        push_edit(doc, &mut out, &wanted, e, None);
    }
    out
}

/// Add `e` as a code action if its kind was asked for. `file` is appended
/// to the title of an insert.
fn push_edit(
    doc: &Doc,
    out: &mut Vec<CodeActionOrCommand>,
    wanted: &impl Fn(&str) -> bool,
    e: edit::Edit,
    file: Option<&str>,
) {
    let kind = e.action.kind();
    if !wanted(kind) {
        return;
    }
    let edits = e
        .edits
        .into_iter()
        .map(|x| TextEdit::new(doc.index.line_range(x.lines), x.text))
        .collect();
    let title = match file {
        Some(file) => format!("{}: {file}", e.title),
        None => e.title.to_string(),
    };
    out.push(CodeActionOrCommand::CodeAction(CodeAction {
        title,
        kind: Some(CodeActionKind::from(kind.to_string())),
        edit: Some(workspace_edit(doc.uri, edits)),
        ..CodeAction::default()
    }));
}

/// How many recent files of each kind to offer, so the menu stays short.
const RECENT_IMAGES: usize = 3;
const RECENT_VIDEOS: usize = 2;
/// Bounds on the asset scan, so a deck in a huge folder can't stall it.
const SCAN_DEPTH: usize = 4;
const SCAN_ENTRIES: usize = 5000;

/// The inserts to offer: the newest images and videos under the deck's
/// asset directory that the deck doesn't reference yet — most often the
/// screenshot or clip just saved there. Paths are relative to that
/// directory, as the deck writes them. The newest image is also offered as
/// the background.
fn media_offers(doc: &Doc, analysis: &Analysis) -> Vec<(edit::Action, String)> {
    let Some(root) = doc.media_dir.or(doc.dir) else {
        return Vec::new();
    };
    // This file as it stands, plus the rest of the deck.
    let mut used: HashSet<PathBuf> = analysis
        .references
        .iter()
        .filter_map(|r| doc.resolve(r)?.canonicalize().ok())
        .collect();
    used.extend(doc.deck_used.into_iter().flatten().cloned());
    let mut files = Vec::new();
    let mut budget = SCAN_ENTRIES;
    scan_media(root, SCAN_DEPTH, &mut budget, &mut files);
    files.retain(|(path, ..)| path.canonicalize().is_ok_and(|p| !used.contains(&p)));
    // Newest first; by path when saved in the same instant.
    files.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let relative = |path: &Path| {
        let rel = path.strip_prefix(root).ok()?;
        let parts: Option<Vec<&str>> = rel.iter().map(|p| p.to_str()).collect();
        Some(parts?.join("/"))
    };
    let newest = |video: bool, n: usize| -> Vec<String> {
        files
            .iter()
            .filter(|(_, _, is_video)| *is_video == video)
            .filter_map(|(path, ..)| relative(path))
            .filter(|p| edit::is_bare_path(p))
            .take(n)
            .collect()
    };
    let images = newest(false, RECENT_IMAGES);
    let videos = newest(true, RECENT_VIDEOS);
    let mut offers: Vec<(edit::Action, String)> = images
        .iter()
        .map(|p| (edit::Action::InsertImage, p.clone()))
        .collect();
    offers.extend(
        images
            .first()
            .map(|p| (edit::Action::BackgroundImage, p.clone())),
    );
    offers.extend(videos.into_iter().map(|p| (edit::Action::InsertVideo, p)));
    offers
}

/// Collect `(path, modified, is_video)` for the images and videos under
/// `dir`, skipping hidden directories and build output.
fn scan_media(
    dir: &Path,
    depth: usize,
    budget: &mut usize,
    out: &mut Vec<(PathBuf, std::time::SystemTime, bool)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            if depth > 0 && !matches!(name, "node_modules" | "target") {
                scan_media(&path, depth - 1, budget, out);
            }
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let ext = ext.to_ascii_lowercase();
        let is_video = complete::extensions(RefKind::Video).contains(&ext.as_str());
        let is_image = complete::extensions(RefKind::Image).contains(&ext.as_str());
        if (is_image || is_video)
            && let Ok(modified) = meta.modified()
        {
            out.push((path, modified, is_video));
        }
    }
}

fn workspace_edit(uri: &Uri, edits: Vec<TextEdit>) -> WorkspaceEdit {
    WorkspaceEdit {
        changes: Some(HashMap::from([(uri.clone(), edits)])),
        ..WorkspaceEdit::default()
    }
}

/// What the word at a byte column does, as a tooltip.
pub fn hover(doc: &Doc, line: usize, col: usize) -> Option<lsp_types::Hover> {
    let found = preso_core::hover::at(doc.text, line, col)?;
    Some(lsp_types::Hover {
        contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
            kind: lsp_types::MarkupKind::Markdown,
            value: found.text,
        }),
        range: Some(doc.index.range(line, found.cols)),
    })
}

/// Completions at a byte column. `snippets`: whether the client takes
/// snippet syntax.
pub fn completions(doc: &Doc, line: usize, col: usize, snippets: bool) -> Vec<CompletionItem> {
    let Some(context) = complete::context(doc.text, line, col) else {
        return Vec::new();
    };
    let item =
        |label: String, kind, replace: std::ops::Range<usize>, text: String| CompletionItem {
            label,
            kind: Some(kind),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
                doc.index.range(line, replace),
                text,
            ))),
            ..CompletionItem::default()
        };
    match context {
        Context::Directive { replace } => complete::DIRECTIVE_SNIPPETS
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let plain = complete::plain(s.snippet);
                let (text, format) = if snippets {
                    (s.snippet.to_string(), InsertTextFormat::SNIPPET)
                } else {
                    (plain.clone(), InsertTextFormat::PLAIN_TEXT)
                };
                CompletionItem {
                    detail: Some(s.detail.to_string()),
                    // Editors filter on the text being replaced, which
                    // starts with `<!--`.
                    filter_text: Some(plain),
                    sort_text: Some(format!("{i:02}")),
                    insert_text_format: Some(format),
                    ..item(
                        s.label.to_string(),
                        CompletionItemKind::SNIPPET,
                        replace.clone(),
                        text,
                    )
                }
            })
            .collect(),
        Context::Value { values, replace } => values
            .iter()
            .map(|v| {
                item(
                    v.to_string(),
                    CompletionItemKind::ENUM_MEMBER,
                    replace.clone(),
                    v.to_string(),
                )
            })
            .collect(),
        Context::DiagramAnnotation {
            language,
            source,
            attr,
            label,
        } => {
            let labels = doc.diagrams.labels(&language, &source).unwrap_or_default();
            labels
                .into_iter()
                .map(|l| item(l.clone(), CompletionItemKind::VALUE, label.clone(), l))
                .chain(complete::FENCE_ATTRS.iter().map(|v| {
                    item(
                        v.to_string(),
                        CompletionItemKind::ENUM_MEMBER,
                        attr.clone(),
                        v.to_string(),
                    )
                }))
                .collect()
        }
        Context::SlideKey { replace } => lint::SLIDE_KEYS
            .iter()
            .map(|(k, _)| format!("{k}="))
            .chain(lint::SLIDE_FLAGS.iter().map(|f| f.to_string()))
            .map(|k| item(k.clone(), CompletionItemKind::PROPERTY, replace.clone(), k))
            .collect(),
        Context::Path { kind, dir, replace } => {
            let Some(base) = doc.bases(kind).first().copied() else {
                return Vec::new();
            };
            path_items(&base.join(&dir), complete::extensions(kind))
                .into_iter()
                .map(|(name, is_dir)| {
                    let kind = if is_dir {
                        CompletionItemKind::FOLDER
                    } else {
                        CompletionItemKind::FILE
                    };
                    item(name.clone(), kind, replace.clone(), name)
                })
                .collect()
        }
    }
}

/// `(name, is_dir)` for the visible subdirectories of `dir` (as `name/`)
/// and its files with one of `extensions`, directories first.
fn path_items(dir: &Path, extensions: &[&str]) -> Vec<(String, bool)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut items: Vec<(String, bool)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if name.starts_with('.') {
                return None;
            }
            if entry.path().is_dir() {
                return Some((format!("{name}/"), true));
            }
            let ext = Path::new(&name).extension()?.to_str()?.to_ascii_lowercase();
            extensions.contains(&ext.as_str()).then_some((name, false))
        })
        .take(MAX_PATH_ITEMS)
        .collect();
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    items
}

/// Clickable asset and include paths, for the ones that exist.
pub fn links(doc: &Doc, analysis: &Analysis) -> Vec<DocumentLink> {
    analysis
        .references
        .iter()
        .filter_map(|r| {
            let path = std::path::absolute(doc.resolve(r)?).ok()?;
            Some(DocumentLink {
                range: doc.index.range(r.line, r.cols.clone()),
                target: Some(path_to_uri(&path)?),
                tooltip: None,
                data: None,
            })
        })
        .collect()
}

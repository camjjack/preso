use serde::Deserialize;

/// Deck-level metadata from the YAML frontmatter block.
///
/// All fields are optional; unknown keys are ignored so decks written for
/// other tools (Slidev, Marp) load without errors.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Frontmatter {
    pub title: Option<String>,
    pub theme: Option<String>,
    pub transition: Option<String>,
    pub aspect: Option<String>,
}

/// A speaker note extracted from `<!-- note: ... -->` / `<!-- speaker: ... -->`.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    /// `None` for slide-level notes; `Some(n)` for `<!-- note[n]: ... -->`,
    /// shown when step `n` is active (0-based: note[1] = after first pause).
    pub step: Option<usize>,
    pub text: String,
}

/// Info parsed from a code fence: `` ```rust {2,4-6} ``.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeBlock {
    /// First token of the fence info string (`rust`).
    pub language: Option<String>,
    /// Everything after the language (`{2,4-6}`), for line highlighting.
    pub annotation: Option<String>,
}

impl CodeBlock {
    /// 1-indexed lines to highlight, from a `{1,3-5}` annotation.
    /// Returns `None` when there is no (parseable) annotation.
    /// Key/value tokens (e.g. `width=60%`) are ignored here.
    pub fn highlighted_lines(&self) -> Option<std::collections::BTreeSet<usize>> {
        parse_line_set(brace_inner(self.annotation.as_deref()?)?)
    }

    /// Number of click-through highlight stages: the count of `|`-separated
    /// groups in the annotation (`{2-3|5|all}` → 3). `1` when there is no
    /// annotation or no `|`, so a static `{2,4-6}` is a single stage.
    pub fn stage_count(&self) -> usize {
        self.annotation
            .as_deref()
            .and_then(brace_inner)
            .map_or(1, |inner| inner.split('|').count())
            .max(1)
    }

    /// 1-indexed lines highlighted at the given 0-based `stage` (clamped to
    /// the last stage). `None` when the stage highlights everything (`all`) or
    /// nothing — i.e. no specific lines should be emphasised.
    pub fn highlighted_lines_at(&self, stage: usize) -> Option<std::collections::BTreeSet<usize>> {
        let inner = brace_inner(self.annotation.as_deref()?)?;
        let stages: Vec<&str> = inner.split('|').collect();
        let spec = stages.get(stage.min(stages.len() - 1))?;
        parse_line_set(spec)
    }

    /// The annotation's individual tokens: the `{…}` inner spec split on
    /// commas, whitespace, and `|`. Splitting on `|` too means word flags
    /// and `key=value` tokens are found even when they ride alongside
    /// click-through stage specs (`{2-3|5 size=20}`). Empty when there is
    /// no (braced) annotation.
    fn tokens(&self) -> impl Iterator<Item = &str> {
        self.annotation
            .as_deref()
            .and_then(brace_inner)
            .into_iter()
            .flat_map(|spec| spec.split([',', ' ', '|']))
            .map(str::trim)
            .filter(|t| !t.is_empty())
    }

    /// Whether a `transparent` flag is present in the annotation:
    /// `` ```mermaid {transparent} `` — diagrams render without the
    /// light card, straight onto the slide background.
    pub fn transparent_background(&self) -> bool {
        self.tokens().any(|t| t == "transparent")
    }

    /// Requested width as a percentage of the slide content width, from a
    /// `width=NN%` token in the annotation: `` ```mermaid {width=60%} ``.
    pub fn width_percent(&self) -> Option<f32> {
        let value = self.tokens().find_map(|t| t.strip_prefix("width="))?;
        let pct: f32 = value.strip_suffix('%')?.trim().parse().ok()?;
        (pct > 0.0 && pct <= 100.0).then_some(pct)
    }

    /// Per-block code font size in design units, from a `size=NN` token:
    /// `` ```rust {size=22} ``. Overrides the theme's `code_size` for this
    /// block (e.g. to shrink a long listing to fit). `None` (use the theme's
    /// size) when absent or non-positive.
    pub fn font_size(&self) -> Option<f32> {
        let value = self.tokens().find_map(|t| t.strip_prefix("size="))?;
        let n: f32 = value.trim().parse().ok()?;
        (n > 0.0).then_some(n)
    }

    /// Per-block highlight style override, from a bare `dim` or `background`
    /// flag in the annotation (`` ```rust {3 background} ``). Overrides the
    /// theme's `[code_block] highlight_style` for this block; `None` to use
    /// the theme default.
    pub fn highlight_style(&self) -> Option<&str> {
        self.tokens().find(|t| matches!(*t, "dim" | "background"))
    }

    /// Horizontal placement of the block from an `align=left|center|right`
    /// token (`` ```rust {align=center} ``); `None` keeps the default (left).
    pub fn align(&self) -> Option<&str> {
        self.tokens()
            .filter_map(|t| t.strip_prefix("align="))
            .find(|v| matches!(*v, "left" | "center" | "right"))
    }
}

/// Strip the `{…}` wrapper from a fence annotation, returning the inner spec.
fn brace_inner(annotation: &str) -> Option<&str> {
    annotation.trim().strip_prefix('{')?.strip_suffix('}')
}

/// Parse one highlight spec (`2,4-6`) into 1-indexed line numbers, ignoring
/// key/value (`width=60%`) and word-flag (`transparent`, `all`) tokens.
/// `None` when the spec selects no specific lines or has an inverted range.
fn parse_line_set(spec: &str) -> Option<std::collections::BTreeSet<usize>> {
    let mut lines = std::collections::BTreeSet::new();
    for part in spec.split([',', ' ']) {
        let part = part.trim();
        if part.is_empty() || part.contains('=') || part.chars().all(char::is_alphabetic) {
            continue;
        }
        match part.split_once('-') {
            Some((a, b)) => {
                let a: usize = a.trim().parse().ok()?;
                let b: usize = b.trim().parse().ok()?;
                if a > b {
                    return None;
                }
                lines.extend(a..=b);
            }
            None => {
                lines.insert(part.parse::<usize>().ok()?);
            }
        }
    }
    (!lines.is_empty()).then_some(lines)
}

/// A display math block extracted from `$$ ... $$`.
#[derive(Debug, Clone, PartialEq)]
pub struct MathBlock {
    pub latex: String,
}

/// Per-column alignment of a GFM table, from the `:---`/`:--:`/`---:` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// A GFM table extracted from the slide source. The rendered source refers
/// to it as `![table](preso-table:<index>)`, the same marker scheme as math.
/// Cell strings keep their inline markdown (`code`, `**bold**`); the renderer
/// resolves it.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub headers: Vec<String>,
    /// One per column (padded to the header width).
    pub aligns: Vec<TableAlign>,
    pub rows: Vec<Vec<String>>,
    /// Cell font size in design units, from a `<!-- table: size=NN -->`
    /// directive before the table; `None` uses the theme's body text size.
    pub font_size: Option<f32>,
}

/// A horizontal row of images, lifted from a run of adjacent image lines so
/// the renderer lays them side by side instead of stacked. Referenced in the
/// source as `![](preso-imagerow:<index>)`, the same marker scheme as tables.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageRow {
    pub images: Vec<ImageRef>,
}

/// One image within an [`ImageRow`].
#[derive(Debug, Clone, PartialEq)]
pub struct ImageRef {
    /// Image URL, including any `#preso-img=` attribute fragment.
    pub url: String,
    pub alt: String,
}

/// A `<!-- image: … -->` decoration placed on a layer between the slide
/// background and its content, so any overlapping text stays on top.
/// Positioned and sized like the theme logo, but authored per slide.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerImage {
    /// Image path, resolved relative to the deck file.
    pub path: String,
    pub position: Anchor,
    /// Width as a percent of the slide width; `None` = the image's natural size.
    pub width: Option<f32>,
    /// Opacity, `0.0`–`1.0` (`1.0` = opaque).
    pub opacity: f32,
    /// Inset from the edges, design units, as `[top, right, bottom, left]`
    /// (`padding=` sets all four; `padding-left=` etc. override per side).
    pub padding: [f32; 4],
}

/// Nine-point anchor for a [`LayerImage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

/// Slide layout, from `<!-- layout: Name -->`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Layout {
    #[default]
    Content,
    /// Content split at `***` into two columns, sized `left:right`
    /// (`<!-- layout: TwoColumn 2:1 -->`; defaults to `1:1`).
    TwoColumn { left: u16, right: u16 },
}

impl Layout {
    /// The column width portions for a two-column layout, else `None`.
    pub fn column_portions(self) -> Option<(u16, u16)> {
        match self {
            Layout::TwoColumn { left, right } => Some((left, right)),
            Layout::Content => None,
        }
    }
}

/// Per-slide style overrides from `<!-- slide: key=value ... -->`.
/// Values stay as strings; the renderer interprets them, keeping this
/// crate style-agnostic.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlideOverrides {
    /// `align=top|center`
    pub align: Option<String>,
    /// `halign=left|center|right`
    pub halign: Option<String>,
    /// `background=#rrggbb`
    pub background: Option<String>,
    /// `kind=title|section` — selects the theme's kind overlay.
    pub kind: Option<String>,
    /// `transition=fade|wipe|none|…` — overrides the deck's frontmatter
    /// `transition` for the change *into* this slide. `None` = use the deck
    /// default. Parsed verbatim; the app maps it to a transition kind.
    pub transition: Option<String>,
    /// `number=N` — reset the slide-number counter so this slide displays
    /// `N`; later slides continue from there. Lets a deck re-sync its
    /// numbering with another (e.g. one that counted hidden slides). See
    /// [`display_number`].
    pub number: Option<usize>,
    /// `hidden` — drop the slide from the deck entirely (it appears in
    /// neither the presentation nor the exported PDF). Slides are filtered
    /// on this at the end of parsing, so it is never seen downstream; it
    /// lives here only so the directive parser has somewhere to record it.
    pub hidden: bool,
}

impl SlideOverrides {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// The 1-based number to display for the slide at `index`, honoring
/// `<!-- slide: number=N -->` resets: the counter starts at 1 and increments
/// per slide, but a slide carrying `number=N` sets it to `N`, and later
/// slides continue from there. With no resets this is just `index + 1`.
pub fn display_number(slides: &[Slide], index: usize) -> usize {
    let mut counter = 1usize;
    for (i, slide) in slides.iter().enumerate() {
        if let Some(set) = slide.overrides.number {
            counter = set;
        }
        if i == index {
            return counter;
        }
        counter += 1;
    }
    counter
}

/// The largest display number across the deck — the value for `{total}`.
/// Equals the slide count when there are no `number=` resets.
pub fn display_total(slides: &[Slide]) -> usize {
    let mut counter = 1usize;
    let mut max = 0usize;
    for slide in slides {
        if let Some(set) = slide.overrides.number {
            counter = set;
        }
        max = max.max(counter);
        counter += 1;
    }
    max
}

/// One slide: a contiguous chunk of markdown between `---` delimiters.
#[derive(Debug, Clone, PartialEq)]
pub struct Slide {
    /// Cleaned markdown source: note comments and pause markers stripped,
    /// fence info strings reduced to the language token.
    /// This is the fully-revealed content (equal to the last step).
    pub source: String,
    /// 1-based line number in the original file where this slide starts.
    /// Used for error reporting and hot-reload index preservation.
    pub start_line: usize,
    /// Speaker notes, in document order.
    pub notes: Vec<Note>,
    /// Optional footnote: a small disclaimer line shown along the bottom of
    /// the slide (e.g. image credits), from `<!-- footnote: … -->`.
    pub footnote: Option<String>,
    /// Optional video clip from `<!-- video: clip.mp4 -->`, as a deck-relative
    /// path. preso can't decode video on the tiny-skia backend, so the slide
    /// shows a ▶ affordance and playback launches an external fullscreen
    /// player. A future wgpu migration would let the `iced_video_player` crate
    /// play it inline instead.
    pub video: Option<String>,
    /// Cumulative markdown per reveal step. Always non-empty; if the slide
    /// has no `<!-- pause -->` markers this is a single element equal to
    /// `source`.
    pub steps: Vec<String>,
    /// Code fences in document order, with language and annotation.
    pub code_blocks: Vec<CodeBlock>,
    /// Display math blocks in document order. The rendered source refers
    /// to them as `![math](preso-math:<index>)`.
    pub math_blocks: Vec<MathBlock>,
    /// GFM tables in document order, referenced as
    /// `![table](preso-table:<index>)`.
    pub tables: Vec<Table>,
    /// Rows of side-by-side images in document order, referenced as
    /// `![](preso-imagerow:<index>)`.
    pub image_rows: Vec<ImageRow>,
    /// Decoration images placed below the content layer (`<!-- image: … -->`).
    pub layer_images: Vec<LayerImage>,
    /// Layout directive for this slide.
    pub layout: Layout,
    /// Per-slide style overrides.
    pub overrides: SlideOverrides,
}

impl Slide {
    /// For [`Layout::TwoColumn`]: split a step's source at the `***`
    /// separator line. Returns `None` if there is no separator.
    pub fn columns_at(&self, step: usize) -> Option<(String, String)> {
        if !matches!(self.layout, Layout::TwoColumn { .. }) {
            return None;
        }
        let source = self.step_source(step);
        let mut left = String::new();
        let mut right = String::new();
        let mut in_right = false;
        let mut fence = crate::fence::Tracker::default();
        for line in source.lines() {
            let in_code = fence.process(line);
            if !in_code && !in_right && line.trim() == "***" {
                in_right = true;
                continue;
            }
            let target = if in_right { &mut right } else { &mut left };
            target.push_str(line);
            target.push('\n');
        }
        in_right.then_some((left, right))
    }

    /// Two-column split as `((left_src, left_slide), (right_src, right_slide))`.
    /// Each column is re-parsed into its own [`Slide`] for per-column code
    /// metadata, but the column source is the *cleaned* text — fence
    /// annotations (`{2,4-6}`, `{width=NN%}`, `{transparent}`) were moved
    /// into `code_blocks` by the original parse and can't be recovered by
    /// re-parsing. So each column's `code_blocks` are restored from this
    /// (parent) slide's `code_blocks`, which keep the annotations and run
    /// in document order: the left column's fences first, then the right's.
    pub fn column_slides(&self, step: usize) -> Option<((String, Slide), (String, Slide))> {
        let (left_src, right_src) = self.columns_at(step)?;
        let mut next = 0usize;
        let mut build = |src: &str| -> Slide {
            let mut sub = crate::parser::parse(src)
                .ok()
                .and_then(|d| d.slides.into_iter().next())
                .unwrap_or_else(|| self.clone());
            let end = (next + sub.code_blocks.len()).min(self.code_blocks.len());
            sub.code_blocks = self.code_blocks[next..end].to_vec();
            next = end;
            sub
        };
        let left = build(&left_src);
        let right = build(&right_src);
        Some(((left_src, left), (right_src, right)))
    }
}

impl Slide {
    pub fn step_count(&self) -> usize {
        // Click-through code highlights add reveal steps too: a slide has as
        // many steps as the larger of its `<!-- pause -->` chunks and its
        // longest code-highlight stage sequence (the "parallel" model — both
        // advance off the same step counter).
        let code_stages = self
            .code_blocks
            .iter()
            .map(CodeBlock::stage_count)
            .max()
            .unwrap_or(1);
        self.steps.len().max(code_stages)
    }

    /// Markdown to render at the given step (clamped).
    pub fn step_source(&self, step: usize) -> &str {
        &self.steps[step.min(self.steps.len() - 1)]
    }

    /// Slide-level notes plus any step notes visible at `step`.
    pub fn notes_at(&self, step: usize) -> impl Iterator<Item = &Note> {
        self.notes
            .iter()
            .filter(move |n| n.step.is_none_or(|s| s <= step))
    }

    /// ATX heading level (1–6) of this slide's first content line, if it
    /// is a heading. Two-column rendering uses this to align the columns'
    /// body text under a shared header band.
    pub fn leading_heading_level(&self) -> Option<u8> {
        leading_heading_level(&self.source)
    }
}

/// ATX heading level (1–6) of the first non-blank line of `source`, or
/// `None` if it is not a heading. CommonMark ATX rules: up to 3 spaces of
/// indent, 1–6 `#`, then a space or end of line.
pub fn leading_heading_level(source: &str) -> Option<u8> {
    let line = source.lines().find(|l| !l.trim().is_empty())?;
    let indent = line.len() - line.trim_start().len();
    if indent > 3 {
        return None; // 4+ spaces is an indented code block, not a heading.
    }
    let trimmed = line.trim_start();
    let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
    let rest = &trimmed[hashes..];
    if (1..=6).contains(&hashes) && (rest.is_empty() || rest.starts_with(char::is_whitespace)) {
        Some(hashes as u8)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::CodeBlock;
    use super::leading_heading_level as lvl;

    fn block(annotation: &str) -> CodeBlock {
        CodeBlock {
            language: Some("rust".into()),
            annotation: Some(annotation.into()),
        }
    }

    #[test]
    fn static_highlight_is_one_stage() {
        let b = block("{2,4-6}");
        assert_eq!(b.stage_count(), 1);
        assert_eq!(b.highlighted_lines_at(0).unwrap(), [2, 4, 5, 6].into());
        // Clamps past the last stage.
        assert_eq!(b.highlighted_lines_at(9).unwrap(), [2, 4, 5, 6].into());
    }

    #[test]
    fn click_through_stages() {
        let b = block("{2-3|5|all}");
        assert_eq!(b.stage_count(), 3);
        assert_eq!(b.highlighted_lines_at(0).unwrap(), [2, 3].into());
        assert_eq!(b.highlighted_lines_at(1).unwrap(), [5].into());
        // `all` highlights everything → no specific line set.
        assert_eq!(b.highlighted_lines_at(2), None);
    }

    #[test]
    fn leading_heading_detection() {
        assert_eq!(lvl("# Title\nbody"), Some(1));
        assert_eq!(lvl("### Problem\n\nbody"), Some(3));
        assert_eq!(lvl("###### Deep"), Some(6));
        // Skips blank lines to the first content line.
        assert_eq!(lvl("\n\n  ## Heading\nx"), Some(2));
        // Not headings:
        assert_eq!(lvl("just prose\n# later"), None);
        assert_eq!(lvl("####### too many"), None); // 7 hashes
        assert_eq!(lvl("#no-space"), None);
        assert_eq!(lvl("    # indented code"), None); // 4-space indent
        assert_eq!(lvl(""), None);
    }
}

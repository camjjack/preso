//! PowerPoint (`.pptx`) import.
//!
//! A `.pptx` is a zip of Office Open XML. Slide order comes from
//! `ppt/presentation.xml`'s `<p:sldIdLst>`, resolved through the package
//! relationships (`ppt/_rels/presentation.xml.rels`) — the `slideN.xml` file
//! numbers do *not* match presentation order. From each slide we extract its
//! text (title, subtitle, bullets nested by paragraph level), tables, images,
//! and speaker notes.
//!
//! Not converted (reported as warnings): charts, SmartArt, and embedded
//! objects, plus anything to do with layout, positioning, fonts/colours, or
//! animation. Extracted images are placed after each slide's text rather than
//! at their original position.

use crate::Conversion;
use anyhow::Context as _;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

/// Relationships namespace (`r:id`/`r:embed` attributes live here).
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// Convert a `.pptx` file to preso markdown.
///
/// `assets` is the directory name (relative to the output file) to extract
/// images into, e.g. `"talk.assets"`. When `None` — for stdout output —
/// images can't be written, so they're reported as warnings instead.
pub fn convert(path: &Path, assets: Option<&str>) -> anyhow::Result<Conversion> {
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file).context("read .pptx (not a valid zip)")?;

    let presentation = read_entry(&mut zip, "ppt/presentation.xml")?;
    let rels = read_entry(&mut zip, "ppt/_rels/presentation.xml.rels")?;
    let aspect = slide_aspect(&presentation);
    let order = slide_order(&presentation, &rels)?;

    let mut warnings = Vec::new();
    let mut slides = Vec::new();
    let mut layout_cache: HashMap<String, Option<String>> = HashMap::new();
    for (i, slide_path) in order.iter().enumerate() {
        let num = i + 1;
        let xml = read_entry(&mut zip, slide_path).with_context(|| format!("read {slide_path}"))?;
        let rels = read_entry_opt(&mut zip, &rels_path(slide_path))?;
        let rels_map = rels.as_deref().map(parse_rels).unwrap_or_default();

        let layout = slide_layout_type(&mut zip, &rels_map, &mut layout_cache);
        let mut slide = parse_slide(
            &xml,
            &rels_map,
            layout.as_deref(),
            num,
            assets.is_some(),
            &mut warnings,
        );
        slide.notes = notes_for(&mut zip, &rels_map);
        slides.push(slide);
    }

    // Extract referenced images (deduped by source path) and map each to its
    // link target under the assets directory.
    let mut media = Vec::new();
    let mut links: HashMap<String, String> = HashMap::new();
    if let Some(dir) = assets {
        let mut sources: Vec<&str> = slides
            .iter()
            .flat_map(|s| s.images.iter().map(|img| img.src.as_str()))
            .collect();
        sources.sort_unstable();
        sources.dedup();
        for src in sources {
            let name = src.rsplit('/').next().unwrap_or(src);
            let link = format!("{dir}/{name}");
            match read_bytes(&mut zip, src) {
                Ok(bytes) => {
                    media.push((link.clone(), bytes));
                    links.insert(src.to_string(), link);
                }
                Err(_) => warnings.push(format!("could not extract image {src}")),
            }
        }
        if !media.is_empty() {
            warnings.push(format!(
                "{} image(s) extracted to {dir}/ and placed after each slide's text — \
                 reposition as needed (PowerPoint layout isn't preserved)",
                media.len()
            ));
        }
    }

    let title = slides.iter().find_map(|s| s.title.clone());
    let output = render(&slides, title.as_deref(), &aspect, &links);
    Ok(Conversion {
        output,
        warnings,
        media,
    })
}

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> anyhow::Result<String> {
    let mut entry = zip
        .by_name(name)
        .with_context(|| format!("{name} missing from .pptx"))?;
    let mut s = String::new();
    entry
        .read_to_string(&mut s)
        .with_context(|| format!("read {name}"))?;
    Ok(s)
}

/// Read an entry that may be absent (e.g. a slide with no relationships).
fn read_entry_opt(
    zip: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> anyhow::Result<Option<String>> {
    match zip.by_name(name) {
        Ok(mut entry) => {
            let mut s = String::new();
            entry
                .read_to_string(&mut s)
                .with_context(|| format!("read {name}"))?;
            Ok(Some(s))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(e) => Err(e).with_context(|| format!("read {name}")),
    }
}

fn read_bytes(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> anyhow::Result<Vec<u8>> {
    let mut entry = zip.by_name(name)?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Reduced `W:H` aspect from `<p:sldSz>` (EMU); defaults to `16:9`.
fn slide_aspect(presentation: &str) -> String {
    let dims = roxmltree::Document::parse(presentation)
        .ok()
        .and_then(|doc| {
            let n = doc.descendants().find(|n| local(n) == "sldSz")?;
            let cx: u64 = n.attribute("cx")?.parse().ok()?;
            let cy: u64 = n.attribute("cy")?.parse().ok()?;
            (cy > 0).then_some((cx, cy))
        });
    match dims {
        Some((cx, cy)) => {
            let g = gcd(cx, cy);
            format!("{}:{}", cx / g, cy / g)
        }
        None => "16:9".to_string(),
    }
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a.max(1) } else { gcd(b, a % b) }
}

/// Ordered slide entry paths, following `<p:sldIdLst>` → `r:id` → the rels map.
fn slide_order(presentation: &str, rels: &str) -> anyhow::Result<Vec<String>> {
    let rdoc = roxmltree::Document::parse(rels).context("parse presentation rels")?;
    let mut targets: HashMap<&str, &str> = HashMap::new();
    for n in rdoc.descendants().filter(|n| local(n) == "Relationship") {
        if let (Some(id), Some(target)) = (n.attribute("Id"), n.attribute("Target")) {
            targets.insert(id, target);
        }
    }

    let pdoc = roxmltree::Document::parse(presentation).context("parse presentation.xml")?;
    let mut order = Vec::new();
    for n in pdoc.descendants().filter(|n| local(n) == "sldId") {
        // `r:id` — the "id" attribute in the relationships namespace (the
        // plain `id` attribute is a different, numeric id).
        let rid = n
            .attributes()
            .find(|a| a.name() == "id" && a.namespace() == Some(REL_NS))
            .map(|a| a.value());
        if let Some(target) = rid.and_then(|rid| targets.get(rid)) {
            order.push(resolve("ppt", target));
        }
    }
    Ok(order)
}

/// One package relationship: its type and (raw, unresolved) target.
struct Rel {
    rtype: String,
    target: String,
}

/// Parse a `.rels` part into a map of relationship id → relationship.
fn parse_rels(xml: &str) -> HashMap<String, Rel> {
    let mut map = HashMap::new();
    let Ok(doc) = roxmltree::Document::parse(xml) else {
        return map;
    };
    for n in doc.descendants().filter(|n| local(n) == "Relationship") {
        // Skip external targets (hyperlinks etc.) — they aren't package parts.
        if n.attribute("TargetMode") == Some("External") {
            continue;
        }
        if let (Some(id), Some(rtype), Some(target)) = (
            n.attribute("Id"),
            n.attribute("Type"),
            n.attribute("Target"),
        ) {
            map.insert(
                id.to_string(),
                Rel {
                    rtype: rtype.to_string(),
                    target: target.to_string(),
                },
            );
        }
    }
    map
}

/// The `.rels` entry path for a part, e.g.
/// `ppt/slides/slide5.xml` → `ppt/slides/_rels/slide5.xml.rels`.
fn rels_path(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// Resolve a relationship target against the part's base directory,
/// collapsing `.`/`..` segments. A leading `/` is package-root-relative.
fn resolve(base: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

struct Image {
    /// Zip entry path of the source media (e.g. `ppt/media/image3.png`).
    src: String,
    alt: String,
}

/// A slide's preso "kind", inferred from its PowerPoint layout.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Title,
    Section,
}

#[derive(Default)]
struct Slide {
    title: Option<String>,
    kind: Option<Kind>,
    subtitle: Vec<String>,
    /// `(indent level, text)` bullets in document order — the left column of
    /// a two-column slide, or the whole body otherwise.
    body: Vec<(usize, String)>,
    /// The right column's bullets when `two_column` is set.
    right: Vec<(usize, String)>,
    two_column: bool,
    /// PowerPoint's "Hide Slide" (`<p:sld show="0">`) → preso `hidden`.
    hidden: bool,
    /// Tables as rows of cell text (the first row is the header).
    tables: Vec<Vec<Vec<String>>>,
    images: Vec<Image>,
    notes: Option<String>,
}

fn parse_slide(
    xml: &str,
    rels: &HashMap<String, Rel>,
    layout: Option<&str>,
    num: usize,
    extract_images: bool,
    warnings: &mut Vec<String>,
) -> Slide {
    let doc = match roxmltree::Document::parse(xml) {
        Ok(doc) => doc,
        Err(e) => {
            warnings.push(format!("slide {num}: XML parse error ({e})"));
            return Slide::default();
        }
    };

    // Kind from the slide layout; `ctrTitle` is a fallback for the title slide.
    let mut slide = Slide {
        kind: match layout {
            Some("title") => Some(Kind::Title),
            Some("secHead") => Some(Kind::Section),
            _ => None,
        },
        // PowerPoint "Hide Slide": `<p:sld show="0">` (absent ⇒ shown).
        hidden: doc
            .root_element()
            .attribute("show")
            .is_some_and(|v| !is_true(v)),
        ..Slide::default()
    };
    // "Two Content"-family layouts (twoObj, twoTxTwoObj, twoColTx, …).
    let two_layout = layout.is_some_and(|t| t.starts_with("two"));

    // Body placeholders, kept separate (with their placeholder index) so a
    // two-column layout can route them into left/right columns.
    let mut body_shapes: Vec<(u32, Vec<(usize, String)>)> = Vec::new();
    for (order, sp) in doc.descendants().filter(|n| local(n) == "sp").enumerate() {
        let ph = sp.descendants().find(|n| local(n) == "ph");
        let ph_type = ph.and_then(|n| n.attribute("type"));
        let idx = ph
            .and_then(|n| n.attribute("idx"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(order as u32);

        // A title becomes `## …`; everything else here opens a block of its
        // own (a bare subtitle line, or a bullet's content).
        let spot = match ph_type {
            Some("ctrTitle") | Some("title") => Spot::Heading,
            _ => Spot::BlockStart,
        };
        let paras: Vec<(usize, String)> = sp
            .descendants()
            .filter(|n| local(n) == "p")
            .map(|p| (para_level(p), para_text(p, spot)))
            .filter(|(_, t)| !t.trim().is_empty())
            .collect();
        if paras.is_empty() {
            continue;
        }

        match ph_type {
            Some("ctrTitle") | Some("title") => {
                if ph_type == Some("ctrTitle") {
                    slide.kind = slide.kind.or(Some(Kind::Title));
                }
                let text = paras
                    .iter()
                    .map(|(_, t)| t.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                slide.title = Some(text);
            }
            Some("subTitle") => slide.subtitle.extend(paras.into_iter().map(|(_, t)| t)),
            // Chrome placeholders carry no slide content.
            Some("sldNum") | Some("ftr") | Some("dt") => {}
            // Body placeholder, indexed content placeholder, or a free text box.
            _ => body_shapes.push((idx, paras)),
        }
    }

    // Route body shapes into one or two columns. Splitting only makes sense
    // with at least two non-empty content shapes; otherwise it's one column.
    if two_layout && body_shapes.len() >= 2 {
        body_shapes.sort_by_key(|(idx, _)| *idx);
        let mut shapes = body_shapes.into_iter();
        slide.body = shapes.next().map(|(_, p)| p).unwrap_or_default();
        slide.right = shapes.flat_map(|(_, p)| p).collect();
        slide.two_column = true;
    } else {
        slide.body = body_shapes.into_iter().flat_map(|(_, p)| p).collect();
    }

    // Tables and other graphic frames (charts, SmartArt, embedded objects).
    let mut other_frames = 0;
    for gf in doc.descendants().filter(|n| local(n) == "graphicFrame") {
        match gf.descendants().find(|n| local(n) == "tbl") {
            Some(tbl) => slide.tables.push(parse_table(tbl)),
            None => other_frames += 1,
        }
    }
    if other_frames > 0 {
        warnings.push(format!(
            "slide {num}: {other_frames} chart/diagram/object(s) skipped (not convertible)"
        ));
    }

    // Pictures.
    let mut unresolved = 0;
    let mut unsupported = 0;
    for pic in doc.descendants().filter(|n| local(n) == "pic") {
        let blip_embed = |name: &str| {
            pic.descendants()
                .find(|n| local(n) == name)
                .and_then(|b| b.attribute((REL_NS, "embed")))
        };
        // PowerPoint stores an SVG picture as a raster fallback in `a:blip`,
        // with the vector itself hanging off an `asvg:svgBlip` extension.
        // preso rasterises SVG at display size, so prefer the vector (the
        // fallback then goes unreferenced and isn't extracted); fall back to
        // the raster if that relationship doesn't resolve.
        let embed = blip_embed("svgBlip")
            .filter(|rid| rels.contains_key(*rid))
            .or_else(|| blip_embed("blip"));
        let alt = pic
            .descendants()
            .find(|n| local(n) == "cNvPr")
            .and_then(|c| c.attribute("descr").or_else(|| c.attribute("name")))
            .unwrap_or("")
            .to_string();
        match embed.and_then(|rid| rels.get(rid)) {
            Some(rel) => {
                let src = resolve("ppt/slides", &rel.target);
                // Skip formats preso can't render (EMF/WMF and exotica); an
                // undecodable bitmap crashes export.
                if supported_image(&src) {
                    slide.images.push(Image { src, alt });
                } else {
                    unsupported += 1;
                }
            }
            None => unresolved += 1,
        }
    }
    if !extract_images && !slide.images.is_empty() {
        warnings.push(format!(
            "slide {num}: {} image(s) skipped (writing to stdout — use -o to extract them)",
            slide.images.len()
        ));
    }
    if unsupported > 0 {
        warnings.push(format!(
            "slide {num}: {unsupported} vector image(s) (EMF/WMF) skipped — preso renders raster and SVG images"
        ));
    }
    if unresolved > 0 {
        warnings.push(format!(
            "slide {num}: {unresolved} image(s) could not be resolved"
        ));
    }

    slide
}

/// Parse an `<a:tbl>` into rows of cell text.
fn parse_table(tbl: roxmltree::Node) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    for tr in tbl.children().filter(|n| local(n) == "tr") {
        let cells = tr
            .children()
            .filter(|n| local(n) == "tc")
            .map(|tc| {
                tc.descendants()
                    .filter(|n| local(n) == "p")
                    .map(|p| para_text(p, Spot::Inline))
                    .collect::<Vec<_>>()
                    .join(" ")
                    .trim()
                    .to_string()
            })
            .collect();
        rows.push(cells);
    }
    rows
}

/// The slide's layout `type` (e.g. `title`, `secHead`, `twoObj`), read from
/// its linked `slideLayout` part. Cached, since slides share layouts.
fn slide_layout_type(
    zip: &mut zip::ZipArchive<std::fs::File>,
    rels: &HashMap<String, Rel>,
    cache: &mut HashMap<String, Option<String>>,
) -> Option<String> {
    let rel = rels.values().find(|r| r.rtype.ends_with("/slideLayout"))?;
    let path = resolve("ppt/slides", &rel.target);
    if !cache.contains_key(&path) {
        let kind = read_entry(zip, &path)
            .ok()
            .and_then(|xml| layout_type(&xml));
        cache.insert(path.clone(), kind);
    }
    cache.get(&path).cloned().flatten()
}

/// The `type` attribute of a `slideLayout` part's `<p:sldLayout>` root.
fn layout_type(xml: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let layout = doc.descendants().find(|n| local(n) == "sldLayout")?;
    layout.attribute("type").map(str::to_string)
}

/// Speaker-note text from the linked notes slide, if any.
fn notes_for(
    zip: &mut zip::ZipArchive<std::fs::File>,
    rels: &HashMap<String, Rel>,
) -> Option<String> {
    let rel = rels.values().find(|r| r.rtype.ends_with("/notesSlide"))?;
    let path = resolve("ppt/slides", &rel.target);
    let xml = read_entry(zip, &path).ok()?;
    extract_notes(&xml)
}

/// The body text of a notes slide (its `type="body"` placeholder).
fn extract_notes(xml: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    for sp in doc.descendants().filter(|n| local(n) == "sp") {
        let is_body = sp
            .descendants()
            .find(|n| local(n) == "ph")
            .and_then(|n| n.attribute("type"))
            == Some("body");
        if !is_body {
            continue;
        }
        // Each note paragraph gets its own line inside the `<!-- note: … -->`
        // comment, so treat every one as opening a block.
        let text = sp
            .descendants()
            .filter(|n| local(n) == "p")
            .map(|p| para_text(p, Spot::BlockStart))
            .filter(|t| !t.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !text.trim().is_empty() {
            return Some(text.trim().to_string());
        }
    }
    None
}

/// Paragraph indent level from `<a:pPr lvl="…">` (0 when absent).
fn para_level(p: roxmltree::Node) -> usize {
    p.children()
        .find(|n| local(n) == "pPr")
        .and_then(|pp| pp.attribute("lvl"))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// A paragraph's text as escaped markdown, with bold/italic runs marked.
///
/// Each `<a:r>` run carries `<a:rPr b=… i=…>`. Adjacent runs with the same
/// formatting are merged (PowerPoint often splits a word across runs). When
/// the whole paragraph shares one format we emit plain text — uniform styling
/// is the base look, not inline emphasis; emphasis is only meaningful where a
/// run *contrasts* with its neighbours.
///
/// `spot` says where the finished text lands, which decides how much of it
/// needs escaping — see [`Spot`].
fn para_text(p: roxmltree::Node, spot: Spot) -> String {
    let mut runs: Vec<(bool, bool, String)> = Vec::new();
    for r in p.children().filter(|n| matches!(local(n), "r" | "fld")) {
        let text: String = r
            .children()
            .filter(|n| local(n) == "t")
            .filter_map(|t| t.text())
            .collect();
        if text.is_empty() {
            continue;
        }
        // Fields (slide numbers, dates) carry no meaningful emphasis.
        let (bold, italic) = if local(&r) == "fld" {
            (false, false)
        } else {
            let rpr = r.children().find(|n| local(n) == "rPr");
            (
                rpr.and_then(|n| n.attribute("b")).is_some_and(is_true),
                rpr.and_then(|n| n.attribute("i")).is_some_and(is_true),
            )
        };
        match runs.last_mut() {
            Some(last) if last.0 == bold && last.1 == italic => last.2.push_str(&text),
            _ => runs.push((bold, italic, text)),
        }
    }

    // Whether a bracket in this paragraph could close a link is a property of
    // the whole paragraph, not of one run: PowerPoint splits text across runs
    // freely, so the `](` completing a `[…]` may well sit in the next one.
    let joined: String = runs.iter().map(|(_, _, t)| t.as_str()).collect();
    let links = joined.contains("](") || joined.contains("][");
    let last = runs.len().saturating_sub(1);
    let escape_at = |i: usize, emphasised: bool| Escape {
        // Emphasis markers go in front of the run's text, so an emphasised
        // run no longer starts its block.
        block_start: i == 0 && spot == Spot::BlockStart && !emphasised,
        heading_end: i == last && spot == Spot::Heading && !emphasised,
        links,
    };

    if runs.len() <= 1 {
        runs.iter()
            .map(|(_, _, t)| escape_md(t, escape_at(0, false)))
            .collect()
    } else {
        runs.iter()
            .enumerate()
            .map(|(i, (bold, italic, t))| {
                emphasize(t, *bold, *italic, escape_at(i, *bold || *italic))
            })
            .collect()
    }
}

/// An OOXML boolean attribute (`1`/`true`).
fn is_true(v: &str) -> bool {
    v == "1" || v.eq_ignore_ascii_case("true")
}

/// Wrap escaped `text` in emphasis markers, keeping any leading/trailing
/// whitespace *outside* the markers (markdown won't emphasise `** x **`).
fn emphasize(text: &str, bold: bool, italic: bool, esc: Escape) -> String {
    let marker = match (bold, italic) {
        (true, true) => "***",
        (true, false) => "**",
        (false, true) => "*",
        (false, false) => return escape_md(text, esc),
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return text.to_string();
    }
    let lead = &text[..text.len() - text.trim_start().len()];
    let trail = &text[text.trim_end().len()..];
    format!("{lead}{marker}{}{marker}{trail}", escape_md(trimmed, esc))
}

fn local<'i>(n: &roxmltree::Node<'_, 'i>) -> &'i str {
    n.tag_name().name()
}

/// Emit preso markdown for the parsed deck. `links` maps an image's source
/// zip path to its extracted link target (absent → the image is dropped).
fn render(
    slides: &[Slide],
    deck_title: Option<&str>,
    aspect: &str,
    links: &HashMap<String, String>,
) -> String {
    let mut front = String::from("---\n");
    if let Some(title) = deck_title {
        front.push_str(&format!("title: {title:?}\n"));
    }
    front.push_str(&format!("aspect: {aspect:?}\n"));
    front.push_str("---");

    let mut out = front;
    for slide in slides {
        out.push_str("\n\n---\n\n");
        out.push_str(&render_slide(slide, links));
    }
    out.trim_end().to_string() + "\n"
}

fn render_slide(slide: &Slide, links: &HashMap<String, String>) -> String {
    let mut blocks: Vec<String> = Vec::new();

    // Directives (kind/hidden, then layout) lead, with the heading attached.
    let mut head = String::new();
    let mut attrs: Vec<&str> = Vec::new();
    match slide.kind {
        Some(Kind::Title) => attrs.push("kind=title"),
        Some(Kind::Section) => attrs.push("kind=section"),
        None => {}
    }
    if slide.hidden {
        attrs.push("hidden");
    }
    if !attrs.is_empty() {
        head.push_str(&format!("<!-- slide: {} -->\n", attrs.join(" ")));
    }
    if slide.two_column {
        head.push_str("<!-- layout: TwoColumn -->\n");
    }
    if let Some(title) = &slide.title {
        // Title and section slides get a top-level heading; a two-column
        // slide's heading stays `##` so it forms the shared header band.
        let hashes = if slide.kind.is_some() && !slide.two_column {
            "#"
        } else {
            "##"
        };
        head.push_str(&format!("{hashes} {}", oneline(title)));
    }
    let head = head.trim_end();
    if !head.is_empty() {
        blocks.push(head.to_string());
    }

    for line in &slide.subtitle {
        blocks.push(oneline(line));
    }

    if let Some(bullets) = bullet_list(&slide.body) {
        blocks.push(bullets);
    }
    if slide.two_column {
        blocks.push("***".to_string());
        if let Some(bullets) = bullet_list(&slide.right) {
            blocks.push(bullets);
        }
    }

    for table in &slide.tables {
        if let Some(md) = render_table(table) {
            blocks.push(md);
        }
    }

    for img in &slide.images {
        if let Some(link) = links.get(&img.src) {
            blocks.push(format!("![{}]({link})", inline(&img.alt)));
        }
    }

    for note in slide.notes.iter() {
        blocks.push(format!("<!-- note: {note} -->"));
    }

    blocks.join("\n\n")
}

/// Render `(level, text)` bullets as a nested markdown list, or `None` if
/// empty. The text is already escaped markdown (see [`para_text`]).
fn bullet_list(bullets: &[(usize, String)]) -> Option<String> {
    if bullets.is_empty() {
        return None;
    }
    let list = bullets
        .iter()
        .map(|(level, text)| format!("{}- {}", "  ".repeat(*level), oneline(text)))
        .collect::<Vec<_>>()
        .join("\n");
    Some(list)
}

/// Render rows of cell text as a GitHub-flavoured markdown table (first row
/// is the header). `None` if there are no rows.
fn render_table(rows: &[Vec<String>]) -> Option<String> {
    let cols = rows.iter().map(Vec::len).max().filter(|&c| c > 0)?;
    let cell = |row: &[String], i: usize| oneline(row.get(i).map(String::as_str).unwrap_or(""));
    let line = |row: &[String]| {
        let cells: Vec<String> = (0..cols).map(|i| cell(row, i)).collect();
        format!("| {} |", cells.join(" | "))
    };

    let mut out = vec![
        line(&rows[0]),
        format!("| {} |", vec!["---"; cols].join(" | ")),
    ];
    out.extend(rows[1..].iter().map(|r| line(r)));
    Some(out.join("\n"))
}

/// Collapse all whitespace to single spaces so already-escaped text (a
/// heading, bullet, or table cell, which came through [`para_text`]) fits on
/// one markdown line; runs can carry embedded newlines.
fn oneline(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Like [`oneline`], but also escapes markdown punctuation. For raw text not
/// already escaped — currently image alt text, which comes from an attribute.
/// Alt text sits inside `![…]`, so its brackets always have to escape however
/// the rest of the line reads.
fn inline(s: &str) -> String {
    escape_md(
        &oneline(s),
        Escape {
            links: true,
            ..Escape::default()
        },
    )
}

/// Whether preso can render an image of this path's format: the bitmaps iced
/// decodes, plus SVG (which preso rasterises through resvg). Office's other
/// vector formats (EMF/WMF) and exotica are skipped — nothing downstream can
/// decode them, and an undecodable bitmap panics PDF export.
fn supported_image(path: &str) -> bool {
    let ext = path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg"
    )
}

/// Where a paragraph's text will be emitted. Most markdown punctuation is
/// only markup in one particular position — `#` opens a heading at the start
/// of a line and closes one at the end of a heading, `>` opens a blockquote
/// at the start — so what has to be escaped depends on where the text lands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Spot {
    /// A heading's text, emitted after `## `.
    Heading,
    /// Text that opens a block: a subtitle on a bare line, or a bullet after
    /// its `- ` marker (a list item's content is a block of its own, so a
    /// leading `#` or `>` still bites there).
    BlockStart,
    /// Inline, with markdown already ahead of it on the line — a table cell,
    /// an image's alt text. Nothing positional can trigger here.
    Inline,
}

/// What [`escape_md`] needs to know beyond the text itself: the positional
/// hazards its [`Spot`] carries, and whether the *paragraph* could form a
/// link (a per-run view can't see a `](` that landed in the next run).
#[derive(Clone, Copy, Default)]
struct Escape {
    block_start: bool,
    heading_end: bool,
    links: bool,
}

/// Backslash-escape the markdown punctuation in a run of PowerPoint text that
/// would otherwise be read as formatting — and only that.
///
/// Escaping every candidate character is the safe-looking option, and it's
/// what this did at first, but the output is a deck someone then edits by
/// hand: `- ldr Xt, \[Rn, \#4\]` and `Fetch -\> Decode` are worse than the
/// hazard they guard against, because *neither* was ever going to be markup.
/// So each character is judged where it actually stands:
///
/// - `_` between two alphanumerics can't open or close emphasis under
///   CommonMark's flanking rules — that's what keeps `snake_case` upright —
///   so `x86_64` passes through. At a word boundary it still escapes, and
///   that includes `__init__`, which really is strong emphasis.
/// - `[` and `]` are only link syntax if some `]` in the paragraph is
///   followed by `(` or `[`. Failing that, `[Rn, #4]` is plain text.
/// - `#` opens a heading only as the first thing on a line (and only when
///   followed by a space), and closes one only as a trailing run inside a
///   heading. `C#` and `#4` are text.
/// - `>` quotes only as the first thing on a line, so `Fetch -> Decode` is
///   text.
///
/// The rest (`\`, `` ` ``, `*`, `<`, `|`, `~`) escape unconditionally: each
/// can bite mid-text, and unlike `_` they carry no flanking rule to lean on.
fn escape_md(s: &str, esc: Escape) -> String {
    // The one `#` that would open a heading, and the one that would start a
    // heading's closing run. Escaping the first of a run is enough to break
    // it: `\##` is no longer "all hashes", so it reads as text.
    let opens_heading = esc.block_start.then(|| atx_open(s)).flatten();
    let closes_heading = esc.heading_end.then(|| atx_close(s)).flatten();
    let opens_quote = esc.block_start.then(|| block_marker(s, '>')).flatten();

    let mut out = String::with_capacity(s.len());
    let mut rest = s.char_indices();
    let mut prev: Option<char> = None;
    while let Some((i, c)) = rest.next() {
        let next = rest.clone().next().map(|(_, c)| c);
        let needs_escape = match c {
            '_' => {
                !(prev.is_some_and(char::is_alphanumeric)
                    && next.is_some_and(char::is_alphanumeric))
            }
            '[' | ']' => esc.links,
            '#' => Some(i) == opens_heading || Some(i) == closes_heading,
            '>' => Some(i) == opens_quote,
            '\\' | '`' | '*' | '<' | '|' | '~' => true,
            _ => false,
        };
        if needs_escape {
            out.push('\\');
        }
        out.push(c);
        prev = Some(c);
    }
    out
}

/// Byte index of the `#` opening an ATX heading — one to six of them, indented
/// at most three spaces, then a space or the end of the line. `#4 things`
/// isn't a heading, and neither is a seventh hash.
fn atx_open(s: &str) -> Option<usize> {
    let indent = block_marker(s, '#')?;
    let rest = &s[indent..];
    let hashes = rest.len() - rest.trim_start_matches('#').len();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    match rest[hashes..].chars().next() {
        None | Some(' ') | Some('\t') => Some(indent),
        _ => None,
    }
}

/// Byte index of the `#` starting the run that would close an ATX heading: a
/// trailing run of hashes with whitespace (or nothing) before it. In `Uses C#`
/// the hash follows a letter, so it closes nothing.
fn atx_close(s: &str) -> Option<usize> {
    let trimmed = s.trim_end();
    let start = trimmed.trim_end_matches('#').len();
    if start == trimmed.len() {
        return None;
    }
    match trimmed[..start].chars().next_back() {
        None => Some(start),
        Some(c) if c.is_whitespace() => Some(start),
        _ => None,
    }
}

/// Byte index of `marker` when it leads the line (up to three spaces of
/// indent, as CommonMark allows before a block marker).
fn block_marker(s: &str, marker: char) -> Option<usize> {
    let indent = s.len() - s.trim_start_matches(' ').len();
    (indent <= 3 && s[indent..].starts_with(marker)).then_some(indent)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_links() -> HashMap<String, String> {
        HashMap::new()
    }

    #[test]
    fn aspect_from_sldsz() {
        let p =
            r#"<p:presentation xmlns:p="x"><p:sldSz cx="12192000" cy="6858000"/></p:presentation>"#;
        assert_eq!(slide_aspect(p), "16:9");
        let p43 =
            r#"<p:presentation xmlns:p="x"><p:sldSz cx="9144000" cy="6858000"/></p:presentation>"#;
        assert_eq!(slide_aspect(p43), "4:3");
    }

    #[test]
    fn resolves_relationship_targets() {
        assert_eq!(
            resolve("ppt/slides", "../media/image3.png"),
            "ppt/media/image3.png"
        );
        assert_eq!(resolve("ppt", "slides/slide1.xml"), "ppt/slides/slide1.xml");
        assert_eq!(resolve("ppt/slides", "/ppt/media/x.png"), "ppt/media/x.png");
        assert_eq!(
            rels_path("ppt/slides/slide5.xml"),
            "ppt/slides/_rels/slide5.xml.rels"
        );
    }

    #[test]
    fn title_slide_extracts_title_and_subtitle() {
        let xml = r#"<p:sld xmlns:p="p" xmlns:a="a">
          <p:cSld><p:spTree>
            <p:sp><p:nvSpPr><p:nvPr><p:ph type="ctrTitle"/></p:nvPr></p:nvSpPr>
              <p:txBody><a:p><a:r><a:t>Deck Title</a:t></a:r></a:p></p:txBody></p:sp>
            <p:sp><p:nvSpPr><p:nvPr><p:ph type="subTitle" idx="1"/></p:nvPr></p:nvSpPr>
              <p:txBody><a:p><a:r><a:t>A Presenter</a:t></a:r></a:p>
                       <a:p><a:r><a:t>https://example.com</a:t></a:r></a:p></p:txBody></p:sp>
            <p:sp><p:nvSpPr><p:nvPr><p:ph type="sldNum" idx="12"/></p:nvPr></p:nvSpPr>
              <p:txBody><a:p><a:r><a:t>1</a:t></a:r></a:p></p:txBody></p:sp>
          </p:spTree></p:cSld></p:sld>"#;
        let mut w = Vec::new();
        let s = parse_slide(xml, &HashMap::new(), None, 1, false, &mut w);
        assert_eq!(s.kind, Some(Kind::Title)); // ctrTitle fallback
        assert_eq!(s.title.as_deref(), Some("Deck Title"));
        assert_eq!(s.subtitle, vec!["A Presenter", "https://example.com"]);
        assert!(s.body.is_empty()); // the slide number is dropped
    }

    #[test]
    fn body_bullets_nest_by_level() {
        let xml = r#"<p:sld xmlns:p="p" xmlns:a="a">
          <p:cSld><p:spTree>
            <p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
              <p:txBody><a:p><a:r><a:t>Topic Heading</a:t></a:r></a:p></p:txBody></p:sp>
            <p:sp><p:nvSpPr><p:nvPr><p:ph idx="1"/></p:nvPr></p:nvSpPr>
              <p:txBody>
                <a:p><a:r><a:t>Top point</a:t></a:r></a:p>
                <a:p><a:pPr lvl="1"/><a:r><a:t>Sub point</a:t></a:r></a:p>
              </p:txBody></p:sp>
          </p:spTree></p:cSld></p:sld>"#;
        let mut w = Vec::new();
        let s = parse_slide(xml, &HashMap::new(), None, 2, false, &mut w);
        assert!(s.kind.is_none());
        assert_eq!(s.title.as_deref(), Some("Topic Heading"));
        assert_eq!(
            s.body,
            vec![(0, "Top point".into()), (1, "Sub point".into())]
        );
    }

    #[test]
    fn table_frame_becomes_gfm() {
        let xml = r#"<p:sld xmlns:p="p" xmlns:a="a">
          <p:cSld><p:spTree>
            <p:graphicFrame><a:graphic><a:graphicData><a:tbl>
              <a:tr><a:tc><a:txBody><a:p><a:r><a:t>H1</a:t></a:r></a:p></a:txBody></a:tc>
                    <a:tc><a:txBody><a:p><a:r><a:t>H2</a:t></a:r></a:p></a:txBody></a:tc></a:tr>
              <a:tr><a:tc><a:txBody><a:p><a:r><a:t>a</a:t></a:r></a:p></a:txBody></a:tc>
                    <a:tc><a:txBody><a:p><a:r><a:t>b</a:t></a:r></a:p></a:txBody></a:tc></a:tr>
            </a:tbl></a:graphicData></a:graphic></p:graphicFrame>
          </p:spTree></p:cSld></p:sld>"#;
        let mut w = Vec::new();
        let s = parse_slide(xml, &HashMap::new(), None, 3, false, &mut w);
        assert_eq!(s.tables.len(), 1);
        let md = render_slide(&s, &no_links());
        assert_eq!(md, "| H1 | H2 |\n| --- | --- |\n| a | b |");
    }

    #[test]
    fn non_table_frame_warns() {
        let xml = r#"<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree>
            <p:graphicFrame><a:graphic><a:graphicData uri="chart"/></a:graphic></p:graphicFrame>
          </p:spTree></p:cSld></p:sld>"#;
        let mut w = Vec::new();
        let s = parse_slide(xml, &HashMap::new(), None, 4, false, &mut w);
        assert!(s.tables.is_empty());
        assert!(w.iter().any(|m| m.contains("chart/diagram/object")));
    }

    #[test]
    fn image_resolves_through_rels_and_renders() {
        let xml = r#"<p:sld xmlns:p="p" xmlns:a="a"
              xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
          <p:cSld><p:spTree>
            <p:pic><p:nvPicPr><p:cNvPr name="Pic 1" descr="A diagram"/></p:nvPicPr>
              <p:blipFill><a:blip r:embed="rId7"/></p:blipFill></p:pic>
          </p:spTree></p:cSld></p:sld>"#;
        let mut rels = HashMap::new();
        rels.insert(
            "rId7".to_string(),
            Rel {
                rtype: "http://example/image".into(),
                target: "../media/image2.png".into(),
            },
        );
        let mut w = Vec::new();
        let s = parse_slide(xml, &rels, None, 5, true, &mut w);
        assert_eq!(s.images.len(), 1);
        assert_eq!(s.images[0].src, "ppt/media/image2.png");
        assert_eq!(s.images[0].alt, "A diagram");

        let mut links = HashMap::new();
        links.insert(
            "ppt/media/image2.png".to_string(),
            "talk.assets/image2.png".to_string(),
        );
        let md = render_slide(&s, &links);
        assert_eq!(md, "![A diagram](talk.assets/image2.png)");
    }

    #[test]
    fn vector_images_are_skipped_with_a_warning() {
        let xml = r#"<p:sld xmlns:p="p" xmlns:a="a"
              xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
          <p:cSld><p:spTree>
            <p:pic><p:nvPicPr><p:cNvPr name="Diagram"/></p:nvPicPr>
              <p:blipFill><a:blip r:embed="rId4"/></p:blipFill></p:pic>
          </p:spTree></p:cSld></p:sld>"#;
        let mut rels = HashMap::new();
        rels.insert(
            "rId4".to_string(),
            Rel {
                rtype: "http://example/image".into(),
                target: "../media/image1.emf".into(),
            },
        );
        let mut w = Vec::new();
        let s = parse_slide(xml, &rels, None, 1, true, &mut w);
        assert!(s.images.is_empty());
        assert!(w.iter().any(|m| m.contains("vector image")));
    }

    #[test]
    fn svg_images_are_kept() {
        let xml = r#"<p:sld xmlns:p="p" xmlns:a="a"
              xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
          <p:cSld><p:spTree>
            <p:pic><p:nvPicPr><p:cNvPr descr="A logo"/></p:nvPicPr>
              <p:blipFill><a:blip r:embed="rId1"/></p:blipFill></p:pic>
          </p:spTree></p:cSld></p:sld>"#;
        let mut rels = HashMap::new();
        rels.insert(
            "rId1".to_string(),
            Rel {
                rtype: "http://example/image".into(),
                target: "../media/image1.svg".into(),
            },
        );
        let mut w = Vec::new();
        let s = parse_slide(xml, &rels, None, 1, true, &mut w);
        assert_eq!(s.images[0].src, "ppt/media/image1.svg");
        assert!(!w.iter().any(|m| m.contains("vector image")));
    }

    #[test]
    fn svg_blip_extension_wins_over_the_raster_fallback() {
        // How PowerPoint itself writes an inserted SVG: the raster fallback in
        // `a:blip`, the vector in an `asvg:svgBlip` extension under it.
        let xml = r#"<p:sld xmlns:p="p" xmlns:a="a" xmlns:asvg="asvg"
              xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
          <p:cSld><p:spTree>
            <p:pic><p:nvPicPr><p:cNvPr descr="A logo"/></p:nvPicPr>
              <p:blipFill><a:blip r:embed="rId1"><a:extLst>
                <a:ext uri="{96DAC541-7B7A-43D3-8B79-37D633B846F1}">
                  <asvg:svgBlip r:embed="rId2"/>
                </a:ext></a:extLst></a:blip></p:blipFill></p:pic>
          </p:spTree></p:cSld></p:sld>"#;
        let mut rels = HashMap::new();
        rels.insert(
            "rId1".to_string(),
            Rel {
                rtype: "http://example/image".into(),
                target: "../media/image1.png".into(),
            },
        );
        rels.insert(
            "rId2".to_string(),
            Rel {
                rtype: "http://example/image".into(),
                target: "../media/image1.svg".into(),
            },
        );
        let mut w = Vec::new();
        let s = parse_slide(xml, &rels, None, 1, true, &mut w);
        assert_eq!(s.images.len(), 1);
        assert_eq!(s.images[0].src, "ppt/media/image1.svg");

        // An unresolvable extension falls back to the raster blip.
        rels.remove("rId2");
        let s = parse_slide(xml, &rels, None, 1, true, &mut Vec::new());
        assert_eq!(s.images[0].src, "ppt/media/image1.png");
    }

    #[test]
    fn multiline_alt_text_is_collapsed_to_one_line() {
        // PowerPoint's auto-generated alt text often spans lines; a multi-line
        // `![…]()` would not parse as an image.
        let img = Image {
            src: "ppt/media/image1.png".into(),
            alt: "A screen with text\n\nAI-generated content may be incorrect.".into(),
        };
        let slide = Slide {
            images: vec![img],
            ..Slide::default()
        };
        let mut links = HashMap::new();
        links.insert(
            "ppt/media/image1.png".to_string(),
            "t.assets/image1.png".to_string(),
        );
        let md = render_slide(&slide, &links);
        assert_eq!(
            md,
            "![A screen with text AI-generated content may be incorrect.](t.assets/image1.png)"
        );
        assert_eq!(md.lines().count(), 1);
    }

    #[test]
    fn extracts_notes_body() {
        let xml = r#"<p:notes xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree>
            <p:sp><p:nvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr>
              <p:txBody><a:p><a:r><a:t>Remember to demo this.</a:t></a:r></a:p>
                       <a:p><a:r><a:t>Second line.</a:t></a:r></a:p></p:txBody></p:sp>
          </p:spTree></p:cSld></p:notes>"#;
        assert_eq!(
            extract_notes(xml).as_deref(),
            Some("Remember to demo this.\nSecond line.")
        );
    }

    #[test]
    fn kind_comes_from_the_slide_layout() {
        let title = r#"<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree>
            <p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
              <p:txBody><a:p><a:r><a:t>Hello</a:t></a:r></a:p></p:txBody></p:sp>
          </p:spTree></p:cSld></p:sld>"#;
        let mut w = Vec::new();
        // The `title` placeholder alone is *not* a title slide…
        assert!(
            parse_slide(title, &HashMap::new(), None, 1, false, &mut w)
                .kind
                .is_none()
        );
        // …but the `title` layout makes it one, and `secHead` a section.
        assert_eq!(
            parse_slide(title, &HashMap::new(), Some("title"), 1, false, &mut w).kind,
            Some(Kind::Title)
        );
        assert_eq!(
            parse_slide(title, &HashMap::new(), Some("secHead"), 1, false, &mut w).kind,
            Some(Kind::Section)
        );
    }

    #[test]
    fn hidden_powerpoint_slide_becomes_hidden_directive() {
        let shown = r#"<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree>
            <p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
              <p:txBody><a:p><a:r><a:t>Visible</a:t></a:r></a:p></p:txBody></p:sp>
          </p:spTree></p:cSld></p:sld>"#;
        let hidden = shown.replacen("<p:sld ", r#"<p:sld show="0" "#, 1);
        let mut w = Vec::new();
        assert!(!parse_slide(shown, &HashMap::new(), None, 1, false, &mut w).hidden);

        let s = parse_slide(&hidden, &HashMap::new(), Some("title"), 1, false, &mut w);
        assert!(s.hidden);
        // The directive combines with the slide kind on one line.
        let md = render_slide(&s, &no_links());
        assert!(md.starts_with("<!-- slide: kind=title hidden -->\n# Visible"));
    }

    #[test]
    fn two_content_layout_splits_into_columns() {
        let xml = r#"<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree>
            <p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
              <p:txBody><a:p><a:r><a:t>Compare</a:t></a:r></a:p></p:txBody></p:sp>
            <p:sp><p:nvSpPr><p:nvPr><p:ph idx="2"/></p:nvPr></p:nvSpPr>
              <p:txBody><a:p><a:r><a:t>right one</a:t></a:r></a:p></p:txBody></p:sp>
            <p:sp><p:nvSpPr><p:nvPr><p:ph idx="1"/></p:nvPr></p:nvSpPr>
              <p:txBody><a:p><a:r><a:t>left one</a:t></a:r></a:p></p:txBody></p:sp>
          </p:spTree></p:cSld></p:sld>"#;
        let mut w = Vec::new();
        let s = parse_slide(xml, &HashMap::new(), Some("twoObj"), 1, false, &mut w);
        assert!(s.two_column);
        // Routed by placeholder index, not document order.
        assert_eq!(s.body, vec![(0, "left one".into())]);
        assert_eq!(s.right, vec![(0, "right one".into())]);
        let md = render_slide(&s, &no_links());
        assert!(md.contains("<!-- layout: TwoColumn -->\n## Compare"));
        assert!(md.contains("- left one\n\n***\n\n- right one"));
    }

    #[test]
    fn mixed_run_formatting_becomes_emphasis() {
        // A paragraph with a bold run amid normal text.
        let xml = r#"<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree>
            <p:sp><p:nvSpPr><p:nvPr><p:ph idx="1"/></p:nvPr></p:nvSpPr>
              <p:txBody><a:p>
                <a:r><a:t>plain and </a:t></a:r>
                <a:r><a:rPr b="1"/><a:t>bold</a:t></a:r>
                <a:r><a:t> and </a:t></a:r>
                <a:r><a:rPr i="1"/><a:t>italic</a:t></a:r>
              </a:p></p:txBody></p:sp>
          </p:spTree></p:cSld></p:sld>"#;
        let mut w = Vec::new();
        let s = parse_slide(xml, &HashMap::new(), None, 1, false, &mut w);
        assert_eq!(s.body, vec![(0, "plain and **bold** and *italic*".into())]);
    }

    #[test]
    fn uniformly_bold_paragraph_is_not_emphasised() {
        // A whole paragraph bold is the base style, not inline emphasis —
        // and PowerPoint splits it across runs, which must be coalesced.
        let xml = r#"<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree>
            <p:sp><p:nvSpPr><p:nvPr><p:ph idx="1"/></p:nvPr></p:nvSpPr>
              <p:txBody><a:p>
                <a:r><a:rPr b="1"/><a:t>All </a:t></a:r>
                <a:r><a:rPr b="1"/><a:t>bold here</a:t></a:r>
              </a:p></p:txBody></p:sp>
          </p:spTree></p:cSld></p:sld>"#;
        let mut w = Vec::new();
        let s = parse_slide(xml, &HashMap::new(), None, 1, false, &mut w);
        assert_eq!(s.body, vec![(0, "All bold here".into())]);
    }

    /// `escape_md` for a bullet (the commonest spot), with no link in sight.
    fn esc_bullet(s: &str) -> String {
        escape_md(
            s,
            Escape {
                block_start: true,
                ..Escape::default()
            },
        )
    }

    #[test]
    fn punctuation_that_cannot_be_markup_is_left_alone() {
        // The technical-deck staples. None of this is markup where it stands,
        // and escaping it made the converted file worse to read and edit.
        assert_eq!(esc_bullet("ldr Xt, [Rn, #4]"), "ldr Xt, [Rn, #4]");
        assert_eq!(
            esc_bullet("Fetch -> Decode -> Execute -> Mem -> Wb"),
            "Fetch -> Decode -> Execute -> Mem -> Wb"
        );
        assert_eq!(esc_bullet("written in C# and F#"), "written in C# and F#");
        assert_eq!(
            esc_bullet("x86_64 with snake_case_fn"),
            "x86_64 with snake_case_fn"
        );
        // `#4` can't open a heading either — that needs a space after the run.
        assert_eq!(esc_bullet("#4 in the list"), "#4 in the list");
    }

    #[test]
    fn punctuation_in_a_position_that_bites_still_escapes() {
        // Same characters, now where they really would become markup.
        assert_eq!(esc_bullet("# heading?"), "\\# heading?");
        assert_eq!(esc_bullet("> quoted?"), "\\> quoted?");
        assert_eq!(esc_bullet("  > indented quote"), "  \\> indented quote");
        // A boundary underscore emphasises; `__init__` is strong emphasis.
        assert_eq!(esc_bullet("_word_"), "\\_word\\_");
        assert_eq!(
            esc_bullet("dunder __init__ method"),
            "dunder \\_\\_init\\_\\_ method"
        );
        // `*` has no flanking rule to lean on — it emphasises intraword too.
        assert_eq!(esc_bullet("a*b*c"), "a\\*b\\*c");
        // Brackets escape once the paragraph could actually form a link.
        let linky = Escape {
            links: true,
            ..Escape::default()
        };
        // Breaking the brackets is enough — the parens are then plain text.
        assert_eq!(escape_md("see [docs](url)", linky), "see \\[docs\\](url)");
    }

    #[test]
    fn a_heading_escapes_only_its_closing_hashes() {
        let heading = |s: &str| {
            escape_md(
                s,
                Escape {
                    heading_end: true,
                    ..Escape::default()
                },
            )
        };
        // A trailing hash run after whitespace is ATX's closing sequence, so
        // `## Sharp #` would lose it.
        assert_eq!(heading("Sharp #"), "Sharp \\#");
        assert_eq!(heading("Sharp ##"), "Sharp \\##");
        // Attached to a word it closes nothing — the reported case.
        assert_eq!(heading("Written in C#"), "Written in C#");
        // A heading isn't a line start, so a leading hash is safe there.
        assert_eq!(heading("#4 Function Example"), "#4 Function Example");
    }

    /// Every escaper decision above, checked against the parser preso
    /// actually renders with (iced's markdown widget is pulldown-cmark with
    /// these options) rather than against my reading of CommonMark: assemble
    /// the line as the converter would emit it, parse it, and require the
    /// text to come back exactly as PowerPoint had it.
    #[test]
    fn escaped_text_round_trips_through_the_renderer() {
        use pulldown_cmark::{Event, Options, Parser};

        let literal = [
            "ldr Xt, [Rn, #4]",
            "str x0, [sp, #-16]!",
            "Fetch -> Decode -> Execute -> Mem -> Wb",
            "written in C# and F#",
            "x86_64 with snake_case_fn",
            "dunder __init__ method",
            "_word_ and a*b*c and `tick` and ~tilde~",
            "# heading?",
            "> quoted?",
            "see [docs](url) here",
            "100% of a|b <tag> \\slash",
            "#4 in the list",
            "Sharp #",
        ];

        let options = Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
        // (prefix, spot) for each place the converter puts escaped text.
        let spots = [
            ("- ", Spot::BlockStart),
            ("", Spot::BlockStart),
            ("## ", Spot::Heading),
        ];

        for text in literal {
            for (prefix, spot) in spots {
                let esc = Escape {
                    block_start: spot == Spot::BlockStart,
                    heading_end: spot == Spot::Heading,
                    links: text.contains("](") || text.contains("]["),
                };
                let line = format!("{prefix}{}", escape_md(text, esc));
                let rendered: String = Parser::new_ext(&line, options)
                    .filter_map(|e| match e {
                        Event::Text(t) | Event::Code(t) => Some(t.to_string()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    rendered, text,
                    "{spot:?} {line:?} did not render back to {text:?}"
                );
            }
        }
    }

    #[test]
    fn render_emits_frontmatter_notes_and_separators() {
        let slides = vec![
            Slide {
                title: Some("Title".into()),
                kind: Some(Kind::Title),
                subtitle: vec!["sub".into()],
                ..Slide::default()
            },
            Slide {
                title: Some("Two".into()),
                body: vec![(0, "a".into()), (1, "b".into())],
                notes: Some("speak".into()),
                ..Slide::default()
            },
        ];
        let md = render(&slides, Some("Title"), "16:9", &no_links());
        assert!(md.contains("aspect: \"16:9\""));
        assert!(md.contains("<!-- slide: kind=title -->\n# Title"));
        assert!(md.contains("## Two"));
        assert!(md.contains("- a\n  - b"));
        assert!(md.contains("<!-- note: speak -->"));
        assert_eq!(md.lines().filter(|l| l.trim() == "---").count(), 4);
    }
}

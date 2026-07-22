//! OOXML part templates and shape builders for the editable `.pptx`
//! exporter. Deliberately hand-written like the importer's reader (and
//! preso-export's bitmap writer): the part set is small and fixed, and a
//! template beats a DOM library for auditability.

use super::blocks::Run;

pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ---------------------------------------------------------------- fonts --

/// Monospace typeface for code runs/blocks.
pub const MONO_FACE: &str = "Consolas";

// ------------------------------------------------------------ text bits --

/// Run properties: size in hundredths of a point; `link_rid` references a
/// hyperlink relationship in the slide's rels.
pub fn run_xml(run: &Run, size_hundredths: u32, link_rid: Option<usize>) -> String {
    let mut props = format!(r#"<a:rPr lang="en-US" sz="{size_hundredths}" dirty="0""#);
    if run.bold {
        props.push_str(r#" b="1""#);
    }
    if run.italic {
        props.push_str(r#" i="1""#);
    }
    props.push('>');
    if run.code {
        props.push_str(r#"<a:solidFill><a:srgbClr val="9A3412"/></a:solidFill>"#);
        props.push_str(&format!(r#"<a:latin typeface="{MONO_FACE}"/>"#));
    }
    if let Some(rid) = link_rid {
        props.push_str(&format!(r#"<a:hlinkClick xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" r:id="rId{rid}"/>"#));
    }
    props.push_str("</a:rPr>");
    format!("<a:r>{props}<a:t>{}</a:t></a:r>", xml_escape(&run.text))
}

/// Paragraph properties for a list item at `level` (0-based).
pub fn list_ppr(level: u8, ordered: bool) -> String {
    let mar = 342_900 * (u32::from(level) + 1);
    let bullet = if ordered {
        r#"<a:buFont typeface="+mj-lt"/><a:buAutoNum type="arabicPeriod"/>"#.to_string()
    } else {
        r#"<a:buFont typeface="Arial"/><a:buChar char="•"/>"#.to_string()
    };
    format!(r#"<a:pPr marL="{mar}" indent="-342900" lvl="{level}">{bullet}</a:pPr>"#)
}

/// A plain text-box shape at the given EMU rect containing paragraphs
/// (pre-built `<a:p>…</a:p>` strings). `autofit` shrinks text on overflow.
pub fn text_shape(id: usize, name: &str, rect: (i64, i64, i64, i64), paragraphs: &str) -> String {
    let (x, y, w, h) = rect;
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{w}" cy="{h}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr wrap="square" rtlCol="0"><a:normAutofit/></a:bodyPr><a:lstStyle/>{paragraphs}</p:txBody></p:sp>"#,
        xml_escape(name)
    )
}

/// A code text-box: light fill, monospace paragraphs supplied by caller.
pub fn code_shape(id: usize, rect: (i64, i64, i64, i64), paragraphs: &str) -> String {
    let (x, y, w, h) = rect;
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Code"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{w}" cy="{h}"/></a:xfrm><a:prstGeom prst="roundRect"><a:avLst><a:gd name="adj" fmla="val 4000"/></a:avLst></a:prstGeom><a:solidFill><a:srgbClr val="F4F4F6"/></a:solidFill></p:spPr><p:txBody><a:bodyPr wrap="square" rtlCol="0" lIns="137160" tIns="91440" rIns="137160" bIns="91440"><a:normAutofit/></a:bodyPr><a:lstStyle/>{paragraphs}</p:txBody></p:sp>"#
    )
}

/// A picture shape referencing `rIdN` in the slide rels.
pub fn picture_shape(id: usize, rid: usize, rect: (i64, i64, i64, i64)) -> String {
    let (x, y, w, h) = rect;
    format!(
        r#"<p:pic><p:nvPicPr><p:cNvPr id="{id}" name="Image"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId{rid}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{w}" cy="{h}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#
    )
}

/// A highlight callout: a rect/ellipse placed at `rect` (EMU) with a
/// semi-transparent `fill` (a full `<a:solidFill>…</a:solidFill>` or
/// `<a:noFill/>`) and an optional `line` (`<a:ln>…</a:ln>`, or empty).
pub fn highlight_shape(
    id: usize,
    name: &str,
    ellipse: bool,
    rect: (i64, i64, i64, i64),
    fill: &str,
    line: &str,
) -> String {
    let (x, y, w, h) = rect;
    let geom = if ellipse { "ellipse" } else { "rect" };
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{w}" cy="{h}"/></a:xfrm><a:prstGeom prst="{geom}"><a:avLst/></a:prstGeom>{fill}{line}</p:spPr></p:sp>"#,
        xml_escape(name)
    )
}

/// A spotlight scrim: a wash over the whole picture `rect` (EMU) with
/// rectangular `holes` punched out so the picked regions stay at full
/// fidelity. Holes are `(x, y, w, h)` in the path's local space (origin at
/// the picture's top-left); each is wound opposite the outer rectangle, so
/// the non-zero fill rule leaves it clear.
pub fn spotlight_scrim(
    id: usize,
    rect: (i64, i64, i64, i64),
    holes: &[(i64, i64, i64, i64)],
    fill: &str,
) -> String {
    let (x, y, w, h) = rect;
    // Outer rectangle, clockwise.
    let mut path = format!(
        r#"<a:moveTo><a:pt x="0" y="0"/></a:moveTo><a:lnTo><a:pt x="{w}" y="0"/></a:lnTo><a:lnTo><a:pt x="{w}" y="{h}"/></a:lnTo><a:lnTo><a:pt x="0" y="{h}"/></a:lnTo><a:close/>"#
    );
    for &(hx, hy, hw, hh) in holes {
        let (x0, y0, x1, y1) = (hx, hy, hx + hw, hy + hh);
        // Counter-clockwise, cancelling the outer winding → a hole.
        path.push_str(&format!(
            r#"<a:moveTo><a:pt x="{x0}" y="{y0}"/></a:moveTo><a:lnTo><a:pt x="{x0}" y="{y1}"/></a:lnTo><a:lnTo><a:pt x="{x1}" y="{y1}"/></a:lnTo><a:lnTo><a:pt x="{x1}" y="{y0}"/></a:lnTo><a:close/>"#
        ));
    }
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Spotlight"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{w}" cy="{h}"/></a:xfrm><a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l="0" t="0" r="{w}" b="{h}"/><a:pathLst><a:path w="{w}" h="{h}">{path}</a:path></a:pathLst></a:custGeom>{fill}</p:spPr></p:sp>"#
    )
}

/// A table graphicFrame. `cells[row][col]` are pre-built `<a:p>` strings;
/// the first row renders as the header (bold handled by caller's runs).
pub fn table_frame(
    id: usize,
    rect: (i64, i64, i64, i64),
    col_widths: &[i64],
    row_height: i64,
    cells: &[Vec<String>],
) -> String {
    let (x, y, w, h) = rect;
    let grid: String = col_widths
        .iter()
        .map(|w| format!(r#"<a:gridCol w="{w}"/>"#))
        .collect();
    let rows: String = cells
        .iter()
        .map(|row| {
            let tcs: String = row
                .iter()
                .map(|p| {
                    format!(
                        r#"<a:tc><a:txBody><a:bodyPr/><a:lstStyle/>{p}</a:txBody><a:tcPr/></a:tc>"#
                    )
                })
                .collect();
            format!(r#"<a:tr h="{row_height}">{tcs}</a:tr>"#)
        })
        .collect();
    format!(
        r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="{id}" name="Table"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp="1"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{w}" cy="{h}"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tblPr firstRow="1" bandRow="1"/><a:tblGrid>{grid}</a:tblGrid>{rows}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#
    )
}

// ---------------------------------------------------------------- parts --

pub fn slide_part(shapes: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>{shapes}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#
    )
}

/// Relationships part from `(rid, type-suffix, target, external?)` rows.
pub fn rels_part(rels: &[(usize, &str, String, bool)]) -> String {
    let body: String = rels
        .iter()
        .map(|(rid, rtype, target, external)| {
            let mode = if *external {
                r#" TargetMode="External""#
            } else {
                ""
            };
            format!(
                r#"<Relationship Id="rId{rid}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{rtype}" Target="{}"{mode}/>"#,
                xml_escape(target)
            )
        })
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{body}</Relationships>"#
    )
}

pub fn presentation_part(slides: usize, w: i64, h: i64) -> String {
    // rId1 = slideMaster, rId2 = notesMaster, slides from rId3.
    let ids: String = (0..slides)
        .map(|i| format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 256 + i, i + 3))
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:notesMasterIdLst><p:notesMasterId r:id="rId2"/></p:notesMasterIdLst><p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx="{w}" cy="{h}"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#
    )
}

pub fn notes_slide_part(text: &str) -> String {
    let paragraphs: String = text
        .split("\n\n")
        .map(|para| {
            format!(
                r#"<a:p><a:r><a:rPr lang="en-US" sz="1200" dirty="0"/><a:t>{}</a:t></a:r></a:p>"#,
                xml_escape(para)
            )
        })
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:notes xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr><p:sp><p:nvSpPr><p:cNvPr id="2" name="Notes"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>{paragraphs}</p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>"#
    )
}

pub fn core_props(title: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{}</dc:title></cp:coreProperties>"#,
        xml_escape(title)
    )
}

pub const APP_PROPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>preso-convert</Application></Properties>"#;

pub const PACKAGE_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#;

const EMPTY_TREE: &str = r#"<p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree>"#;

pub fn slide_master() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld>{EMPTY_TREE}</p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst></p:sldMaster>"#
    )
}

pub fn notes_master() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:notesMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld>{EMPTY_TREE}</p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/></p:notesMaster>"#
    )
}

pub fn slide_layout() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="blank"><p:cSld>{EMPTY_TREE}</p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#
    )
}

pub const MASTER_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/></Relationships>"#;

pub const NOTES_MASTER_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme2.xml"/></Relationships>"#;

pub const LAYOUT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"#;

/// Minimal but schema-complete theme, shared shape with the bitmap
/// exporter's: full clrScheme, fontScheme, and the three-entry lists
/// fmtScheme requires.
pub fn theme(name: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="{name}"><a:themeElements><a:clrScheme name="{name}"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="1F1F1F"/></a:dk2><a:lt2><a:srgbClr val="EEEEEE"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme><a:fontScheme name="{name}"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="{name}"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#
    )
}

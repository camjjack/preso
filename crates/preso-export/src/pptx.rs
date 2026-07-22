//! Bitmap-per-slide PowerPoint assembly: each captured frame becomes one
//! slide holding a single full-bleed picture. The mirror of the PDF path —
//! pixel-faithful (themes and all) but not editable text; it answers
//! "the organizers want a .pptx", not "hand off for editing".
//!
//! A `.pptx` is an OOXML zip. The minimal valid part set is written by
//! hand, like the importer reads it: content types, package rels, a
//! presentation with one blank master/layout/theme (required even though
//! every slide is just a picture), one slide + rels + media file per page,
//! and core/app doc properties carrying the title.

use crate::{ExportError, Page, rgba_to_rgb};
use std::io::Write;
use std::path::Path;
use zip::write::SimpleFileOptions;

/// One inch in English Metric Units, the OOXML coordinate space.
const EMU_PER_INCH: i64 = 914_400;
/// Slide height: 7.5in, PowerPoint's standard; width follows the pages'
/// aspect ratio (16:9 pages give the standard 12192000×6858000).
const SLIDE_HEIGHT_EMU: i64 = (7.5 * EMU_PER_INCH as f64) as i64;

/// Write `pages` as a picture-per-slide PowerPoint file at `out`.
/// `quality` (0..1) is the JPEG quality used where JPEG wins the
/// per-slide codec choice (flat slides stay lossless PNG).
pub fn write_pptx(
    title: &str,
    pages: &[Page],
    out: &Path,
    quality: f32,
) -> Result<(), ExportError> {
    if pages.is_empty() {
        return Err(ExportError::NoPages);
    }
    let io_err = |e: &dyn std::fmt::Display| ExportError::Io {
        path: out.display().to_string(),
        source: std::io::Error::other(e.to_string()),
    };

    // Slide geometry from the first page's aspect (pages share dimensions).
    let (w, h) = (pages[0].width as f64, pages[0].height.max(1) as f64);
    let slide_w = ((SLIDE_HEIGHT_EMU as f64) * w / h).round() as i64;
    let slide_h = SLIDE_HEIGHT_EMU;

    let file = std::fs::File::create(out).map_err(|source| ExportError::Io {
        path: out.display().to_string(),
        source,
    })?;
    let mut zip = zip::ZipWriter::new(file);
    let xml_opts =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    // Media bytes are already PNG/JPEG-compressed; recompressing wastes time.
    let media_opts =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    let put = |zip: &mut zip::ZipWriter<std::fs::File>,
               name: &str,
               bytes: &[u8],
               opts: SimpleFileOptions|
     -> Result<(), ExportError> {
        zip.start_file(name, opts).map_err(|e| io_err(&e))?;
        zip.write_all(bytes).map_err(|e| io_err(&e))
    };

    // Encode every slide up front so [Content_Types].xml knows which
    // image extensions occur.
    let media: Vec<(Vec<u8>, &'static str)> = pages
        .iter()
        .map(|page| encode_media(page, quality))
        .collect();

    put(
        &mut zip,
        "[Content_Types].xml",
        content_types(&media).as_bytes(),
        xml_opts,
    )?;
    put(&mut zip, "_rels/.rels", PACKAGE_RELS.as_bytes(), xml_opts)?;
    put(
        &mut zip,
        "docProps/core.xml",
        core_props(title).as_bytes(),
        xml_opts,
    )?;
    put(&mut zip, "docProps/app.xml", APP_PROPS.as_bytes(), xml_opts)?;
    put(
        &mut zip,
        "ppt/presentation.xml",
        presentation(pages.len(), slide_w, slide_h).as_bytes(),
        xml_opts,
    )?;
    put(
        &mut zip,
        "ppt/_rels/presentation.xml.rels",
        presentation_rels(pages.len()).as_bytes(),
        xml_opts,
    )?;
    put(
        &mut zip,
        "ppt/slideMasters/slideMaster1.xml",
        SLIDE_MASTER.as_bytes(),
        xml_opts,
    )?;
    put(
        &mut zip,
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        MASTER_RELS.as_bytes(),
        xml_opts,
    )?;
    put(
        &mut zip,
        "ppt/slideLayouts/slideLayout1.xml",
        SLIDE_LAYOUT.as_bytes(),
        xml_opts,
    )?;
    put(
        &mut zip,
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        LAYOUT_RELS.as_bytes(),
        xml_opts,
    )?;
    put(&mut zip, "ppt/theme/theme1.xml", THEME.as_bytes(), xml_opts)?;

    for (i, (bytes, ext)) in media.iter().enumerate() {
        let n = i + 1;
        put(
            &mut zip,
            &format!("ppt/slides/slide{n}.xml"),
            slide_xml(slide_w, slide_h).as_bytes(),
            xml_opts,
        )?;
        put(
            &mut zip,
            &format!("ppt/slides/_rels/slide{n}.xml.rels"),
            slide_rels(n, ext).as_bytes(),
            xml_opts,
        )?;
        put(
            &mut zip,
            &format!("ppt/media/image{n}.{ext}"),
            bytes,
            media_opts,
        )?;
    }

    zip.finish().map_err(|e| io_err(&e))?;
    Ok(())
}

/// Encode a slide as the smaller of lossless PNG or JPEG — the same
/// per-slide codec choice as the PDF exporter (Flate ↔ PNG both being the
/// lossless option that wins on flat/text slides).
fn encode_media(page: &Page, quality: f32) -> (Vec<u8>, &'static str) {
    use image::ImageEncoder;

    let rgb = rgba_to_rgb(&page.rgba);

    let mut jpeg = Vec::new();
    let q = (quality.clamp(0.05, 1.0) * 100.0).round().clamp(1.0, 100.0) as u8;
    let jpeg_ok = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, q)
        .encode(
            &rgb,
            page.width,
            page.height,
            image::ExtendedColorType::Rgb8,
        )
        .is_ok();

    let mut png = Vec::new();
    let png_ok = image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            &rgb,
            page.width,
            page.height,
            image::ExtendedColorType::Rgb8,
        )
        .is_ok();

    if jpeg_ok && !jpeg.is_empty() && (!png_ok || jpeg.len() <= png.len()) {
        (jpeg, "jpeg")
    } else {
        (png, "png")
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn content_types(media: &[(Vec<u8>, &'static str)]) -> String {
    let mut defaults = String::from(
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>"#,
    );
    if media.iter().any(|(_, e)| *e == "png") {
        defaults.push_str(r#"<Default Extension="png" ContentType="image/png"/>"#);
    }
    if media.iter().any(|(_, e)| *e == "jpeg") {
        defaults.push_str(r#"<Default Extension="jpeg" ContentType="image/jpeg"/>"#);
    }
    let slides: String = (1..=media.len())
        .map(|n| format!(
            r#"<Override PartName="/ppt/slides/slide{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#
        ))
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">{defaults}<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>{slides}</Types>"#
    )
}

const PACKAGE_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#;

fn core_props(title: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{}</dc:title></cp:coreProperties>"#,
        xml_escape(title)
    )
}

const APP_PROPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>preso</Application></Properties>"#;

fn presentation(slides: usize, w: i64, h: i64) -> String {
    // rId1 is the master; slides start at rId2. Slide ids must be >= 256.
    let ids: String = (0..slides)
        .map(|i| format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 256 + i, i + 2))
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx="{w}" cy="{h}"/><p:notesSz cx="{h}" cy="{w}"/></p:presentation>"#
    )
}

fn presentation_rels(slides: usize) -> String {
    let slide_rels: String = (1..=slides)
        .map(|n| format!(
            r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{n}.xml"/>"#,
            n + 1
        ))
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>{slide_rels}</Relationships>"#
    )
}

const SLIDE_MASTER: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst></p:sldMaster>"#;

const MASTER_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/></Relationships>"#;

const SLIDE_LAYOUT: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="blank"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#;

const LAYOUT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"#;

/// A minimal but schema-complete theme (clrScheme, fontScheme, and the
/// three-entry fill/line/effect lists fmtScheme requires). Nothing on the
/// slides references it — every slide is one picture — but PowerPoint
/// refuses decks whose master lacks a theme.
const THEME: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="preso"><a:themeElements><a:clrScheme name="preso"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="1F1F1F"/></a:dk2><a:lt2><a:srgbClr val="EEEEEE"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme><a:fontScheme name="preso"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="preso"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#;

fn slide_xml(w: i64, h: i64) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr><p:pic><p:nvPicPr><p:cNvPr id="2" name="Slide image"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId1"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{w}" cy="{h}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#
    )
}

fn slide_rels(n: usize, ext: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image{n}.{ext}"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn solid_page(w: u32, h: u32, color: [u8; 4]) -> Page {
        Page {
            width: w,
            height: h,
            rgba: color
                .iter()
                .copied()
                .cycle()
                .take((w * h * 4) as usize)
                .collect(),
        }
    }

    fn out_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("preso-export-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn writes_a_structurally_valid_pptx() {
        let out = out_path("deck.pptx");
        let pages = vec![
            solid_page(640, 360, [30, 30, 46, 255]),
            solid_page(640, 360, [239, 241, 245, 255]),
        ];
        write_pptx("test deck", &pages, &out, 0.7).unwrap();

        let mut zip = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        for part in [
            "[Content_Types].xml",
            "_rels/.rels",
            "ppt/presentation.xml",
            "ppt/_rels/presentation.xml.rels",
            "ppt/slideMasters/slideMaster1.xml",
            "ppt/slideLayouts/slideLayout1.xml",
            "ppt/theme/theme1.xml",
            "ppt/slides/slide1.xml",
            "ppt/slides/slide2.xml",
            "ppt/slides/_rels/slide2.xml.rels",
        ] {
            assert!(zip.by_name(part).is_ok(), "missing part {part}");
        }
        // Flat slides pick lossless PNG.
        assert!(zip.by_name("ppt/media/image1.png").is_ok());

        // 16:9 pages → PowerPoint's standard 12192000×6858000 slide size.
        let mut pres = String::new();
        zip.by_name("ppt/presentation.xml")
            .unwrap()
            .read_to_string(&mut pres)
            .unwrap();
        assert!(pres.contains(r#"<p:sldSz cx="12192000" cy="6858000"/>"#));
        assert_eq!(pres.matches("<p:sldId ").count(), 2);
    }

    #[test]
    fn round_trips_through_the_pptx_importer() {
        // The strongest headless validation available: preso's own .pptx
        // importer must read the file back — slide count intact, each
        // slide carrying its picture.
        let out = out_path("roundtrip.pptx");
        let pages = vec![
            solid_page(640, 360, [30, 30, 46, 255]),
            solid_page(640, 360, [200, 30, 46, 255]),
            solid_page(640, 360, [30, 200, 46, 255]),
        ];
        write_pptx("round trip", &pages, &out, 0.7).unwrap();

        let conv = preso_convert::convert_pptx(&out, Some("rt.assets")).unwrap();
        assert_eq!(conv.media.len(), 3, "one image per slide");
        // Parse the round-tripped markdown with preso itself: the slide
        // count must survive writer → importer → parser.
        let deck = preso_core::parser::parse(&conv.output).unwrap();
        assert_eq!(deck.slides.len(), 3);
    }

    #[test]
    fn title_is_escaped_and_empty_pages_error() {
        let out = out_path("escaped.pptx");
        write_pptx(
            "A <B> & \"C\"",
            &[solid_page(64, 36, [0, 0, 0, 255])],
            &out,
            0.7,
        )
        .unwrap();
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        let mut core = String::new();
        zip.by_name("docProps/core.xml")
            .unwrap()
            .read_to_string(&mut core)
            .unwrap();
        assert!(core.contains("A &lt;B&gt; &amp; &quot;C&quot;"));

        assert!(matches!(
            write_pptx("x", &[], &out, 0.7),
            Err(ExportError::NoPages)
        ));
    }
}

//! preso-export: assemble captured slide frames into a bitmap-per-slide PDF.
//!
//! Each slide becomes its own image XObject. The codec is chosen *per slide*:
//! a flat/text slide compresses smaller (and stays sharp) with lossless Flate,
//! while a photographic slide is smaller as JPEG — so we encode both and keep
//! whichever is smaller. The PDF is built directly with `lopdf` because
//! higher-level writers only allow one codec for the whole document.

mod pptx;
pub use pptx::write_pptx;

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use std::io::Write;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("no pages were captured")]
    NoPages,

    #[error("cannot write {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
}

/// One captured slide frame, straight RGBA.
pub struct Page {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Page layout for the produced PDF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    /// One slide per page; the page IS the slide (16:9).
    #[default]
    Slides,
    /// Handout: two slides stacked on an A4 portrait page.
    TwoUp,
}

/// Standard 16:9 slide page width in PDF points (13.33in × 72).
const SLIDE_PAGE_WIDTH_PT: f32 = 960.0;
/// A4 portrait, in points.
const A4_WIDTH_PT: f32 = 595.276;
const A4_HEIGHT_PT: f32 = 841.89;
/// Side margin for the two-up handout. Small, so the (width-constrained)
/// slides are as large as the page allows.
const TWO_UP_SIDE_MARGIN_PT: f32 = 28.0;

/// One image placed on a page: (xobject, width, height, x, y) in points,
/// anchored at its bottom-left corner.
type Placement = (ObjectId, f32, f32, f32, f32);

/// Write `pages` to a PDF at `out`. `quality` (0..1) is the JPEG quality used
/// where JPEG wins the per-slide codec choice.
pub fn write_pdf(
    title: &str,
    pages: &[Page],
    out: &std::path::Path,
    layout: Layout,
    quality: f32,
) -> Result<(), ExportError> {
    if pages.is_empty() {
        return Err(ExportError::NoPages);
    }

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();

    // One image XObject per slide (codec chosen per slide).
    let images: Vec<(ObjectId, &Page)> = pages
        .iter()
        .map(|page| (image_object(&mut doc, page, quality), page))
        .collect();

    let page_refs: Vec<Object> = match layout {
        Layout::Slides => images
            .iter()
            .map(|(img, page)| {
                // The page *is* the slide: fixed width, height from aspect.
                let h = SLIDE_PAGE_WIDTH_PT * page.height as f32 / page.width as f32;
                let placements = [(*img, SLIDE_PAGE_WIDTH_PT, h, 0.0, 0.0)];
                build_page(&mut doc, pages_id, SLIDE_PAGE_WIDTH_PT, h, &placements)
            })
            .collect(),
        Layout::TwoUp => images
            .chunks(2)
            .map(|pair| {
                let sw = A4_WIDTH_PT - 2.0 * TWO_UP_SIDE_MARGIN_PT;
                // Slides share dimensions; derive the (width-constrained) height
                // from the first, then distribute the two with three equal
                // vertical gaps (top / middle / bottom) to fill the page.
                let sh = sw * pair[0].1.height as f32 / pair[0].1.width as f32;
                let gap = ((A4_HEIGHT_PT - 2.0 * sh) / 3.0).max(0.0);
                let placements: Vec<Placement> = pair
                    .iter()
                    .enumerate()
                    .map(|(i, (img, _))| {
                        let y = if i == 0 { A4_HEIGHT_PT - gap - sh } else { gap };
                        (*img, sw, sh, TWO_UP_SIDE_MARGIN_PT, y)
                    })
                    .collect();
                build_page(&mut doc, pages_id, A4_WIDTH_PT, A4_HEIGHT_PT, &placements)
            })
            .collect(),
    };

    let count = page_refs.len() as i64;
    let mut pages_dict = Dictionary::new();
    pages_dict.set("Type", Object::Name(b"Pages".to_vec()));
    pages_dict.set("Kids", Object::Array(page_refs));
    pages_dict.set("Count", Object::Integer(count));
    doc.objects.insert(pages_id, Object::Dictionary(pages_dict));

    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", Object::Reference(pages_id));
    let catalog_id = doc.add_object(catalog);
    doc.trailer.set("Root", catalog_id);

    let mut info = Dictionary::new();
    info.set("Title", pdf_text_string(title));
    let info_id = doc.add_object(info);
    doc.trailer.set("Info", info_id);

    let mut buf = Vec::new();
    doc.save_to(&mut buf).map_err(|e| ExportError::Io {
        path: out.display().to_string(),
        source: std::io::Error::other(e.to_string()),
    })?;
    std::fs::write(out, buf).map_err(|source| ExportError::Io {
        path: out.display().to_string(),
        source,
    })
}

/// Encode a PDF *text string* (e.g. the Info `Title`). ASCII passes through
/// as a literal string (ASCII is a subset of PDFDocEncoding); anything else
/// is UTF-16BE with a BOM, per the PDF spec — writing raw UTF-8 bytes would
/// show as mojibake in viewers.
fn pdf_text_string(s: &str) -> Object {
    if s.is_ascii() {
        return Object::String(s.as_bytes().to_vec(), StringFormat::Literal);
    }
    let mut bytes = vec![0xfe, 0xff];
    for unit in s.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    Object::String(bytes, StringFormat::Hexadecimal)
}

/// Add an image XObject for `page`, encoding it as whichever of lossless Flate
/// or JPEG is smaller, and return its object id.
fn image_object(doc: &mut Document, page: &Page, quality: f32) -> ObjectId {
    let (data, filter) = encode_image(page, quality);
    let mut dict = Dictionary::new();
    dict.set("Type", Object::Name(b"XObject".to_vec()));
    dict.set("Subtype", Object::Name(b"Image".to_vec()));
    dict.set("Width", Object::Integer(page.width as i64));
    dict.set("Height", Object::Integer(page.height as i64));
    dict.set("ColorSpace", Object::Name(b"DeviceRGB".to_vec()));
    dict.set("BitsPerComponent", Object::Integer(8));
    dict.set("Filter", Object::Name(filter.as_bytes().to_vec()));
    // The data is already encoded with `filter`; don't let lopdf re-compress.
    let stream = Stream::new(dict, data).with_compression(false);
    doc.add_object(stream)
}

/// Encode a slide as the smaller of lossless Flate (FlateDecode) or JPEG
/// (DCTDecode). Returns the encoded bytes and the PDF filter name.
fn encode_image(page: &Page, quality: f32) -> (Vec<u8>, &'static str) {
    let rgb = rgba_to_rgb(&page.rgba);

    // JPEG candidate.
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

    // Lossless Flate candidate.
    let flate = {
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(8));
        enc.write_all(&rgb).ok();
        enc.finish().unwrap_or_default()
    };

    if jpeg_ok && !jpeg.is_empty() && jpeg.len() <= flate.len() {
        (jpeg, "DCTDecode")
    } else {
        (flate, "FlateDecode")
    }
}

/// Drop the (opaque) alpha channel: PDF DeviceRGB images are 3 bytes/pixel.
pub(crate) fn rgba_to_rgb(rgba: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for px in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&px[..3]);
    }
    rgb
}

/// Build one PDF page of `page_w`×`page_h` points placing `placements`, and
/// return a reference to it for the page tree.
fn build_page(
    doc: &mut Document,
    parent: ObjectId,
    page_w: f32,
    page_h: f32,
    placements: &[Placement],
) -> Object {
    let mut ops = Vec::new();
    let mut xobjects = Dictionary::new();
    for (i, (img, w, h, x, y)) in placements.iter().enumerate() {
        let name = format!("Im{i}");
        xobjects.set(name.clone(), Object::Reference(*img));
        // Images draw into the unit square; the matrix scales/places them.
        ops.push(Operation::new("q", vec![]));
        ops.push(Operation::new(
            "cm",
            vec![
                Object::Real(*w),
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(*h),
                Object::Real(*x),
                Object::Real(*y),
            ],
        ));
        ops.push(Operation::new("Do", vec![Object::Name(name.into_bytes())]));
        ops.push(Operation::new("Q", vec![]));
    }
    let content = Content { operations: ops };
    let content_id = doc.add_object(Stream::new(
        Dictionary::new(),
        content.encode().unwrap_or_default(),
    ));

    let mut resources = Dictionary::new();
    resources.set("XObject", Object::Dictionary(xobjects));

    let mut page = Dictionary::new();
    page.set("Type", Object::Name(b"Page".to_vec()));
    page.set("Parent", Object::Reference(parent));
    page.set(
        "MediaBox",
        Object::Array(vec![
            Object::Real(0.0),
            Object::Real(0.0),
            Object::Real(page_w),
            Object::Real(page_h),
        ]),
    );
    page.set("Resources", Object::Dictionary(resources));
    page.set("Contents", Object::Reference(content_id));
    Object::Reference(doc.add_object(page))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn writes_a_parseable_pdf() {
        let dir = std::env::temp_dir().join(format!("preso-export-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("out.pdf");
        let pages = vec![
            solid_page(640, 360, [30, 30, 46, 255]),
            solid_page(640, 360, [239, 241, 245, 255]),
        ];
        write_pdf("test deck", &pages, &out, Layout::Slides, 0.7).unwrap();
        assert!(std::fs::read(&out).unwrap().starts_with(b"%PDF"));
        let doc = Document::load(&out).unwrap();
        assert_eq!(doc.get_pages().len(), 2);
    }

    #[test]
    fn two_up_packs_pairs_onto_a4_pages() {
        let dir = std::env::temp_dir().join(format!("preso-export-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("handout.pdf");
        let pages = vec![
            solid_page(640, 360, [30, 30, 46, 255]),
            solid_page(640, 360, [239, 241, 245, 255]),
            solid_page(640, 360, [137, 180, 250, 255]),
        ];
        write_pdf("handout", &pages, &out, Layout::TwoUp, 0.7).unwrap();
        // 3 slides → 2 A4 pages.
        let doc = Document::load(&out).unwrap();
        assert_eq!(doc.get_pages().len(), 2);
    }

    #[test]
    fn non_ascii_title_round_trips_as_utf16() {
        let dir = std::env::temp_dir().join(format!("preso-export-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("title.pdf");
        let title = "Über—Vortrag ✓";
        let pages = vec![solid_page(64, 36, [0, 0, 0, 255])];
        write_pdf(title, &pages, &out, Layout::Slides, 0.7).unwrap();

        let doc = Document::load(&out).unwrap();
        let info_id = doc.trailer.get(b"Info").unwrap().as_reference().unwrap();
        let Object::String(bytes, _) = doc.get_dictionary(info_id).unwrap().get(b"Title").unwrap()
        else {
            panic!("Title is not a string");
        };
        // UTF-16BE with BOM, decoding back to the original title.
        assert_eq!(bytes[..2], [0xfe, 0xff]);
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(String::from_utf16(&units).unwrap(), title);

        // ASCII titles stay plain literal strings (no BOM).
        let Object::String(ascii, _) = pdf_text_string("plain title") else {
            panic!("not a string");
        };
        assert_eq!(ascii, b"plain title");
    }

    #[test]
    fn flat_slides_pick_lossless() {
        // A solid-colour slide is far smaller (and sharper) as lossless Flate
        // than JPEG, so the per-slide chooser must pick it.
        let page = solid_page(640, 360, [30, 30, 46, 255]);
        let (_, filter) = encode_image(&page, 0.7);
        assert_eq!(filter, "FlateDecode");
    }

    #[test]
    fn empty_pages_is_an_error() {
        let out = std::env::temp_dir().join("never.pdf");
        assert!(matches!(
            write_pdf("x", &[], &out, Layout::Slides, 0.7),
            Err(ExportError::NoPages)
        ));
    }
}

//! Rendered-media cache: math images, Mermaid diagrams, and slide images.
//!
//! Lives on `App` and is consulted from `view` code, so lookups use
//! interior mutability. Renders happen lazily on first use and are cached
//! by content + size; hot reload keeps the cache (content-keyed entries
//! stay valid).

use iced::Size;
use iced::widget::image;
use preso_diagram::Renderer;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

/// Supersampling factor for rasterized vector content (crisp on hidpi).
const OVERSAMPLE: f32 = 2.0;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    Math {
        latex: String,
        px: u32,
        color: (u8, u8, u8),
    },
    Mermaid {
        source: String,
        px: u32,
        transparent: bool,
    },
    Graphviz {
        source: String,
        px: u32,
    },
    Image {
        path: String,
        px: u32,
    },
    /// A slide image with highlight washes composited into its pixels
    /// (see [`Media::masked_image`]); `ops` is a hash of the applied ops.
    MaskedImage {
        path: String,
        px: u32,
        ops: u64,
    },
    Gradient {
        from: (u8, u8, u8, u8),
        to: (u8, u8, u8, u8),
        /// Millidegrees, so the key stays `Eq`/`Hash`.
        angle_mdeg: i32,
        w: u32,
        h: u32,
    },
}

type Entry = Option<(image::Handle, Size)>;

/// A decoded, animated GIF: frames with per-frame delays.
pub struct Gif {
    pub frames: Vec<(image::Handle, std::time::Duration)>,
    pub size: Size,
    pub total: std::time::Duration,
}

impl Gif {
    /// The frame to show at `t` (loops forever).
    pub fn frame_at(&self, t: std::time::Duration) -> &image::Handle {
        let mut remaining =
            std::time::Duration::from_nanos((t.as_nanos() % self.total.as_nanos().max(1)) as u64);
        for (handle, delay) in &self.frames {
            if remaining < *delay {
                return handle;
            }
            remaining -= *delay;
        }
        &self.frames[0].0
    }
}

pub struct Media {
    renderer: Renderer,
    base_dir: RefCell<PathBuf>,
    cache: RefCell<HashMap<Key, Entry>>,
    gifs: RefCell<HashMap<String, Option<std::rc::Rc<Gif>>>>,
}

impl Media {
    pub fn new(deck_path: &std::path::Path) -> Self {
        let renderer = Renderer::new(
            &[
                crate::render::INTER_REGULAR,
                crate::render::INTER_BOLD,
                crate::render::INTER_ITALIC,
                crate::render::JETBRAINS_MONO,
            ],
            "Inter",
            "JetBrains Mono",
        );
        Self {
            renderer,
            base_dir: RefCell::new(base_dir_of(deck_path)),
            cache: RefCell::new(HashMap::new()),
            gifs: RefCell::new(HashMap::new()),
        }
    }

    /// Decode an animated GIF (deck-relative path), cached.
    pub fn gif(&self, url: &str) -> Option<std::rc::Rc<Gif>> {
        if let Some(entry) = self.gifs.borrow().get(url) {
            return entry.clone();
        }
        let decoded = self.decode_gif(url);
        self.gifs
            .borrow_mut()
            .insert(url.to_string(), decoded.clone());
        decoded
    }

    fn decode_gif(&self, url: &str) -> Option<std::rc::Rc<Gif>> {
        use ::image::AnimationDecoder;

        let path = self.base_dir.borrow().join(url);
        let file = std::fs::File::open(&path)
            .map_err(|e| tracing::warn!(path = %path.display(), error = %e, "gif not found"))
            .ok()?;
        let decoder = ::image::codecs::gif::GifDecoder::new(std::io::BufReader::new(file))
            .map_err(|e| tracing::warn!(error = %e, "gif decode failed"))
            .ok()?;
        let raw_frames = decoder
            .into_frames()
            .collect_frames()
            .map_err(|e| tracing::warn!(error = %e, "gif frames failed"))
            .ok()?;
        if raw_frames.is_empty() {
            return None;
        }

        let mut size = Size::ZERO;
        let mut total = std::time::Duration::ZERO;
        let frames: Vec<(image::Handle, std::time::Duration)> = raw_frames
            .into_iter()
            .map(|frame| {
                let delay = std::time::Duration::from(frame.delay())
                    .max(std::time::Duration::from_millis(20));
                let buffer = frame.into_buffer();
                size = Size::new(buffer.width() as f32, buffer.height() as f32);
                total += delay;
                (
                    image::Handle::from_rgba(buffer.width(), buffer.height(), buffer.into_raw()),
                    delay,
                )
            })
            .collect();

        Some(std::rc::Rc::new(Gif {
            frames,
            size,
            total,
        }))
    }

    /// Called when a different deck file is opened.
    pub fn set_deck_path(&self, deck_path: &std::path::Path) {
        *self.base_dir.borrow_mut() = base_dir_of(deck_path);
    }

    /// Display math rendered at `font_px` logical pixels of font size, in
    /// the given text color (kind themes can change it per slide).
    pub fn math(&self, latex: &str, font_px: f32, text: preso_style::Color) -> Entry {
        let px = font_px.round().max(1.0) as u32;
        // RGB only: RaTeX output has no alpha channel (see `math_svg`).
        let color = (text.r, text.g, text.b);
        let key = Key::Math {
            latex: latex.to_string(),
            px,
            color,
        };
        self.lookup(key, || {
            let svg = self
                .renderer
                .math_svg(latex, true, color, px as f32 * OVERSAMPLE)
                .map_err(|e| tracing::warn!(error = %e, latex, "math render failed"))
                .ok()?;
            self.to_handle(&svg, 1.0)
        })
    }

    /// Mermaid diagram at a logical scale factor. `target_width` (logical
    /// pixels, from a `{width=NN%}` annotation) overrides the intrinsic
    /// size; the raster is produced at exactly that size for crispness.
    pub fn mermaid(
        &self,
        source: &str,
        scale: f32,
        target_width: Option<f32>,
        transparent: bool,
    ) -> Entry {
        let px = target_width
            .map(|w| w.round() as u32)
            .unwrap_or_else(|| (scale * 100.0).round() as u32);
        let key = Key::Mermaid {
            source: source.to_string(),
            px,
            transparent,
        };
        self.lookup(key, || {
            let svg = self
                .renderer
                .mermaid_svg(source, transparent)
                .map_err(|e| tracing::warn!(error = %e, "mermaid render failed"))
                .ok()?;
            self.svg_entry(&svg, target_width, scale)
        })
    }

    /// Graphviz DOT diagram; same sizing contract as [`Self::mermaid`].
    pub fn graphviz(&self, source: &str, scale: f32, target_width: Option<f32>) -> Entry {
        let px = target_width
            .map(|w| w.round() as u32)
            .unwrap_or_else(|| (scale * 100.0).round() as u32);
        let key = Key::Graphviz {
            source: source.to_string(),
            px,
        };
        self.lookup(key, || {
            let svg = self
                .renderer
                .graphviz_svg(source)
                .map_err(|e| tracing::warn!(error = %e, "graphviz render failed"))
                .ok()?;
            self.svg_entry(&svg, target_width, scale)
        })
    }

    /// Slide image (`![alt](path)`), resolved relative to the deck file.
    /// SVG files are rasterized (at `target_width` logical pixels when a
    /// `{width=NN%}` attribute was given); bitmaps load via iced directly.
    pub fn slide_image(&self, url: &str, target_width: Option<f32>) -> Entry {
        let key = Key::Image {
            path: url.to_string(),
            px: target_width.map_or(0, |w| w.round() as u32),
        };
        self.lookup(key, || {
            if url.contains("://") {
                tracing::debug!(url, "remote images are not supported yet");
                return None;
            }
            let path = self.base_dir.borrow().join(url);
            if !path.is_file() {
                tracing::warn!(path = %path.display(), "image not found");
                return None;
            }
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
            {
                let svg = std::fs::read_to_string(&path).ok()?;
                self.svg_entry(&svg, target_width, 1.0)
            } else {
                load_bitmap(&path)
            }
        })
    }

    /// Like [`Self::slide_image`], but with the highlight `ops` composited
    /// into the pixels — respecting the image's alpha, so a transparent
    /// image's see-through areas stay untouched (the slide background
    /// showing through them is unaffected). This is what a `clip` highlight
    /// needs: iced's canvas can't mask a scrim by an arbitrary alpha
    /// channel, so we bake the wash into the pixels instead. Cached by
    /// `(path, px, ops)`; unsupported for remote and animated-GIF images
    /// (returns `None`, so the caller falls back to the canvas overlay).
    pub fn masked_image(&self, url: &str, target_width: Option<f32>, ops: &[MaskOp]) -> Entry {
        use std::hash::{Hash, Hasher};
        if ops.is_empty() {
            return None;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for op in ops {
            op.hash(&mut hasher);
        }
        let key = Key::MaskedImage {
            path: url.to_string(),
            px: target_width.map_or(0, |w| w.round() as u32),
            ops: hasher.finish(),
        };
        self.lookup(key, || {
            let (mut rgba, w, h, logical) = self.decode_rgba(url, target_width)?;
            composite_mask(&mut rgba, w, h, ops);
            Some((image::Handle::from_rgba(w, h, rgba), logical))
        })
    }

    /// Decode a slide image to raw RGBA at the resolution [`Self::slide_image`]
    /// would use, plus the logical display size. SVGs rasterize (oversampled
    /// for hidpi); bitmaps decode to natural pixels. `None` for remote,
    /// missing, or undecodable files.
    fn decode_rgba(
        &self,
        url: &str,
        target_width: Option<f32>,
    ) -> Option<(Vec<u8>, u32, u32, Size)> {
        if url.contains("://") {
            return None;
        }
        let path = self.base_dir.borrow().join(url);
        if !path.is_file() {
            return None;
        }
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
        {
            let svg = std::fs::read_to_string(&path).ok()?;
            let scale = match target_width {
                Some(width) => {
                    let (intrinsic_w, _) = self.renderer.svg_size(&svg).ok()?;
                    (width / intrinsic_w) * OVERSAMPLE
                }
                None => OVERSAMPLE,
            };
            let raster = self.renderer.rasterize(&svg, scale).ok()?;
            let logical = Size::new(
                raster.width as f32 / OVERSAMPLE,
                raster.height as f32 / OVERSAMPLE,
            );
            Some((raster.rgba, raster.width, raster.height, logical))
        } else {
            let bytes = std::fs::read(&path).ok()?;
            let img = ::image::load_from_memory(&bytes).ok()?;
            let rgba = img.to_rgba8();
            let (w, h) = (rgba.width(), rgba.height());
            Some((rgba.into_raw(), w, h, Size::new(w as f32, h as f32)))
        }
    }

    /// Soft cap on cached entries. Renders are content-keyed, so a talk
    /// rarely exceeds a few dozen; the cap only matters during long
    /// hot-reload editing sessions with churning content.
    const MAX_ENTRIES: usize = 256;

    /// Slide background gradient, rendered with dithering. iced's
    /// gradients quantize straight to 8-bit, which shows as banding on
    /// slow dark gradients; rendering the ramp ourselves with triangular
    /// noise (±1 LSB) pushes the steps below visibility.
    pub fn gradient(&self, gradient: preso_style::Gradient, size: Size) -> Entry {
        let (w, h) = (size.width.round() as u32, size.height.round() as u32);
        if w == 0 || h == 0 {
            return None;
        }
        let c = |c: preso_style::Color| (c.r, c.g, c.b, c.a);
        let key = Key::Gradient {
            from: c(gradient.from),
            to: c(gradient.to),
            angle_mdeg: (gradient.angle * 1000.0).round() as i32,
            w,
            h,
        };
        self.lookup(key, || {
            let rgba = dithered_gradient_rgba(gradient, w, h);
            Some((
                image::Handle::from_rgba(w, h, rgba),
                Size::new(w as f32, h as f32),
            ))
        })
    }

    fn lookup(&self, key: Key, render: impl FnOnce() -> Entry) -> Entry {
        if let Some(entry) = self.cache.borrow().get(&key) {
            return entry.clone();
        }
        let entry = render();
        let mut cache = self.cache.borrow_mut();
        if cache.len() >= Self::MAX_ENTRIES {
            tracing::debug!("media cache full; clearing");
            cache.clear();
        }
        cache.insert(key, entry.clone());
        entry
    }

    /// Rasterize an SVG into a cache entry: at exactly `target_width`
    /// logical pixels when given (a `{width=NN%}` annotation — sizing the
    /// raster to the displayed width keeps vector content crisp), else at
    /// `fallback_scale`. Both paths oversample for hidpi.
    fn svg_entry(&self, svg: &str, target_width: Option<f32>, fallback_scale: f32) -> Entry {
        let raster_scale = match target_width {
            Some(width) => {
                let (intrinsic_w, _) = self.renderer.svg_size(svg).ok()?;
                (width / intrinsic_w) * OVERSAMPLE
            }
            None => fallback_scale * OVERSAMPLE,
        };
        self.to_handle(svg, raster_scale)
    }

    fn to_handle(&self, svg: &str, raster_scale: f32) -> Entry {
        let raster = self
            .renderer
            .rasterize(svg, raster_scale)
            .map_err(|e| tracing::warn!(error = %e, "rasterize failed"))
            .ok()?;
        let logical = Size::new(
            raster.width as f32 / OVERSAMPLE,
            raster.height as f32 / OVERSAMPLE,
        );
        Some((
            image::Handle::from_rgba(raster.width, raster.height, raster.rgba),
            logical,
        ))
    }
}

fn base_dir_of(deck_path: &std::path::Path) -> PathBuf {
    deck_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Load a bitmap image, guarding against the renderer aborting on a file it
/// can't decode.
///
/// iced selects its decoder by file *extension*, so a file whose bytes don't
/// match its extension (e.g. a JPEG saved as `.png`) decodes to nothing and
/// panics deep in `iced_tiny_skia` ("Image should be allocated"). We compare
/// the real (content) format to the extension: when they agree, hand iced the
/// path and let it decode lazily (cheap, low-memory); otherwise decode here by
/// content into RGBA, which always rasterises. A file that isn't a decodable
/// image is skipped (the caller shows its alt text) rather than crashing.
/// One highlight wash to composite into an image's pixels (see
/// [`Media::masked_image`]). Coordinates are fractions of the image.
#[derive(Debug, Clone, Copy)]
pub struct MaskOp {
    pub ellipse: bool,
    /// `true` = spotlight scrim (dim *outside* the region); `false` = fill
    /// (tint *inside* the region).
    pub spotlight: bool,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: (u8, u8, u8),
    /// Blend strength, `0.0`–`1.0`.
    pub alpha: f32,
}

impl std::hash::Hash for MaskOp {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Quantize the floats so the op is `Hash` (for the cache key) and
        // sub-pixel jitter doesn't mint new cache entries.
        self.ellipse.hash(state);
        self.spotlight.hash(state);
        for f in [self.x, self.y, self.w, self.h, self.alpha] {
            ((f * 4096.0).round() as i32).hash(state);
        }
        self.color.hash(state);
    }
}

impl MaskOp {
    /// Whether the region contains the point at image-fraction `(fx, fy)`.
    fn contains(&self, fx: f32, fy: f32) -> bool {
        if self.ellipse {
            let (cx, cy) = (self.x + self.w / 2.0, self.y + self.h / 2.0);
            let (rx, ry) = (self.w / 2.0, self.h / 2.0);
            if rx <= 0.0 || ry <= 0.0 {
                return false;
            }
            let (dx, dy) = ((fx - cx) / rx, (fy - cy) / ry);
            dx * dx + dy * dy <= 1.0
        } else {
            fx >= self.x && fx < self.x + self.w && fy >= self.y && fy < self.y + self.h
        }
    }
}

/// Blend the highlight `ops` into `rgba` (`w`×`h`, straight-alpha RGBA8),
/// leaving each pixel's alpha untouched — so fully transparent areas stay
/// transparent and the slide background behind them is unaffected.
/// Spotlight ops dim everything outside the union of their regions (styled
/// by the first one); fill ops tint their region. Spotlight is applied
/// first, then fills paint on top.
fn composite_mask(rgba: &mut [u8], w: u32, h: u32, ops: &[MaskOp]) {
    let spots: Vec<&MaskOp> = ops.iter().filter(|o| o.spotlight).collect();
    let fills: Vec<&MaskOp> = ops.iter().filter(|o| !o.spotlight).collect();
    let scrim = spots.first();
    let blend = |chan: &mut u8, target: u8, a: f32| {
        *chan = (f32::from(*chan) * (1.0 - a) + f32::from(target) * a).round() as u8;
    };
    for py in 0..h {
        for px in 0..w {
            let idx = ((py * w + px) * 4) as usize;
            if rgba[idx + 3] == 0 {
                continue; // transparent: leave it (background shows through)
            }
            let fx = (px as f32 + 0.5) / w as f32;
            let fy = (py as f32 + 0.5) / h as f32;
            if let Some(s) = scrim
                && !spots.iter().any(|o| o.contains(fx, fy))
            {
                blend(&mut rgba[idx], s.color.0, s.alpha);
                blend(&mut rgba[idx + 1], s.color.1, s.alpha);
                blend(&mut rgba[idx + 2], s.color.2, s.alpha);
            }
            for o in &fills {
                if o.contains(fx, fy) {
                    blend(&mut rgba[idx], o.color.0, o.alpha);
                    blend(&mut rgba[idx + 1], o.color.1, o.alpha);
                    blend(&mut rgba[idx + 2], o.color.2, o.alpha);
                }
            }
        }
    }
}

fn load_bitmap(path: &std::path::Path) -> Entry {
    let content_format = image_header(path).and_then(|h| ::image::guess_format(&h).ok());
    let ext_format = ::image::ImageFormat::from_path(path).ok();
    match content_format {
        None => {
            tracing::warn!(path = %path.display(), "not a recognised image; skipping");
            None
        }
        // Extension matches the bytes: iced can decode it lazily from the path.
        Some(fmt) if Some(fmt) == ext_format => {
            let size = ::image::image_dimensions(path)
                .map(|(w, h)| Size::new(w as f32, h as f32))
                .unwrap_or(Size::ZERO);
            Some((image::Handle::from_path(path.to_path_buf()), size))
        }
        // Mislabeled (or no/odd extension): decode by content ourselves.
        Some(_) => match std::fs::read(path)
            .ok()
            .and_then(|b| ::image::load_from_memory(&b).ok())
        {
            Some(img) => {
                let rgba = img.to_rgba8();
                let (w, h) = (rgba.width(), rgba.height());
                Some((
                    image::Handle::from_rgba(w, h, rgba.into_raw()),
                    Size::new(w as f32, h as f32),
                ))
            }
            None => {
                tracing::warn!(path = %path.display(), "image decode failed; skipping");
                None
            }
        },
    }
}

/// First bytes of a file, enough for `image::guess_format` to sniff the format.
fn image_header(path: &std::path::Path) -> Option<Vec<u8>> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 32];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(buf)
}

/// Render a linear gradient to RGBA with triangular dithering.
///
/// Geometry replicates `iced::Radians::to_distance` (angle 0 points the
/// `to` color up, clockwise; span from the rect center by the larger
/// axis extent), so themes look the same as with iced's own gradients —
/// minus the banding: interpolation happens in f32 and each channel
/// adds ±1 LSB of triangular noise before quantizing to 8-bit.
fn dithered_gradient_rgba(gradient: preso_style::Gradient, w: u32, h: u32) -> Vec<u8> {
    let theta = gradient.angle.to_radians();
    let (rx, ry) = (theta.sin(), -theta.cos());
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let extent = f32::max((rx * w as f32 / 2.0).abs(), (ry * h as f32 / 2.0).abs());
    // Projection of the start point and the start→end length, along r.
    let start_dot = (cx - rx * extent) * rx + (cy - ry * extent) * ry;
    let span = (2.0 * extent).max(f32::EPSILON);

    let from = [
        f32::from(gradient.from.r),
        f32::from(gradient.from.g),
        f32::from(gradient.from.b),
        f32::from(gradient.from.a),
    ];
    let to = [
        f32::from(gradient.to.r),
        f32::from(gradient.to.g),
        f32::from(gradient.to.b),
        f32::from(gradient.to.a),
    ];

    // Deterministic per-pixel hash noise (no rand dependency; stable
    // output keeps snapshots and exports reproducible).
    let hash = |x: u32, y: u32, salt: u32| -> f32 {
        let mut v = x
            .wrapping_mul(0x9e37_79b9)
            .wrapping_add(y.wrapping_mul(0x85eb_ca6b))
            .wrapping_add(salt.wrapping_mul(0xc2b2_ae35));
        v ^= v >> 16;
        v = v.wrapping_mul(0x7feb_352d);
        v ^= v >> 15;
        v = v.wrapping_mul(0x846c_a68b);
        v ^= v >> 16;
        v as f32 / u32::MAX as f32
    };

    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let dot = (x as f32 + 0.5) * rx + (y as f32 + 0.5) * ry;
            let t = ((dot - start_dot) / span).clamp(0.0, 1.0);
            // ONE triangular noise value in [-1, 1) LSB, shared by R, G,
            // and B: banding is a luminance artifact, so luma-only dither
            // breaks it just as well — and unlike independent per-channel
            // noise it leaves JPEG/PNG chroma smooth, which keeps PDF
            // exports compressible (~14x smaller pages).
            let noise = hash(x, y, 0) + hash(x, y, 1) - 1.0;
            for c in 0..3usize {
                let v = from[c] + (to[c] - from[c]) * t;
                rgba.push((v + noise).round().clamp(0.0, 255.0) as u8);
            }
            let alpha = from[3] + (to[3] - from[3]) * t;
            rgba.push(alpha.round().clamp(0.0, 255.0) as u8);
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(angle: f32) -> preso_style::Gradient {
        preso_style::Gradient {
            from: preso_style::Color::rgb(0x0f, 0x20, 0x27),
            to: preso_style::Color::rgb(0x2c, 0x53, 0x64),
            angle,
        }
    }

    #[test]
    fn composite_mask_preserves_alpha_and_dims_only_outside() {
        // 2x1 image: left pixel opaque red, right pixel transparent.
        let mut rgba = vec![255, 0, 0, 255, 0, 0, 0, 0];
        // Spotlight the right half with an opaque black scrim: the left
        // (opaque) pixel is outside → dims to black; the right (transparent)
        // pixel keeps alpha 0, so the slide background stays untouched.
        let op = MaskOp {
            ellipse: false,
            spotlight: true,
            x: 0.5,
            y: 0.0,
            w: 0.5,
            h: 1.0,
            color: (0, 0, 0),
            alpha: 1.0,
        };
        composite_mask(&mut rgba, 2, 1, &[op]);
        assert_eq!(rgba, vec![0, 0, 0, 255, 0, 0, 0, 0]);

        // A fill over the whole width tints only the opaque pixel; the
        // transparent one is skipped (alpha stays 0).
        let mut rgba = vec![200, 200, 200, 255, 9, 9, 9, 0];
        let op = MaskOp {
            ellipse: false,
            spotlight: false,
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
            color: (0, 0, 255),
            alpha: 0.5,
        };
        composite_mask(&mut rgba, 2, 1, &[op]);
        assert_eq!(rgba, vec![100, 100, 228, 255, 9, 9, 9, 0]);
    }

    #[test]
    fn load_bitmap_handles_mislabeled_and_broken_files() {
        let dir = std::env::temp_dir().join(format!("preso-media-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // A JPEG whose name says `.png` — iced would decode-by-extension and
        // panic; load_bitmap must decode by content and report real dims.
        let img = ::image::RgbImage::from_pixel(7, 5, ::image::Rgb([10, 20, 30]));
        let mislabeled = dir.join("photo.png");
        ::image::DynamicImage::ImageRgb8(img)
            .save_with_format(&mislabeled, ::image::ImageFormat::Jpeg)
            .unwrap();
        let (_, size) = load_bitmap(&mislabeled).expect("mislabeled jpeg should load");
        assert_eq!(size, Size::new(7.0, 5.0));

        // Garbage with an image extension → skipped, not a panic.
        let broken = dir.join("broken.png");
        std::fs::write(&broken, b"not an image").unwrap();
        assert!(load_bitmap(&broken).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gradient_endpoints_match_stops() {
        // angle 0: `to` at the top, `from` at the bottom.
        let px = dithered_gradient_rgba(gradient(0.0), 64, 256);
        let top = &px[0..3];
        let bottom = &px[px.len() - 4 * 64..px.len() - 4 * 64 + 3];
        // Within dither noise of the exact stop colors.
        assert!((i32::from(top[0]) - 0x2c).abs() <= 1, "top red {}", top[0]);
        assert!((i32::from(top[2]) - 0x64).abs() <= 1, "top blue {}", top[2]);
        assert!((i32::from(bottom[0]) - 0x0f).abs() <= 1);
        assert!((i32::from(bottom[2]) - 0x27).abs() <= 1);
    }

    #[test]
    fn gradient_is_dithered_not_banded() {
        // On a slow ramp, plain quantization makes long runs of identical
        // rows; dithering must vary pixels within a single row.
        let w = 64u32;
        let px = dithered_gradient_rgba(gradient(0.0), w, 1024);
        let mid_row = 512usize * w as usize * 4;
        let row = &px[mid_row..mid_row + w as usize * 4];
        let first = row[0];
        assert!(
            (0..w as usize).any(|x| row[x * 4] != first),
            "row is uniform: dithering not applied"
        );
    }

    #[test]
    fn gradient_is_deterministic() {
        let a = dithered_gradient_rgba(gradient(160.0), 128, 72);
        let b = dithered_gradient_rgba(gradient(160.0), 128, 72);
        assert_eq!(a, b);
    }
}

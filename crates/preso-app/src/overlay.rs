//! Laser pointer and freehand pen strokes, shared between windows.
//!
//! All positions are stored in design-space (the 1920×1080 virtual
//! canvas), so a stroke drawn on the presenter's miniature renders
//! identically on the audience window and vice versa. Each window's
//! overlay converts with its own scale.

use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Theme, mouse};

/// Stroke width and laser radii in design units.
const STROKE_WIDTH: f32 = 5.0;
const LASER_HALO: f32 = 22.0;
const LASER_DOT: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PointerMode {
    #[default]
    None,
    /// `L`: glowing dot follows the cursor.
    Laser,
    /// `P`: dragging draws strokes; `C` clears.
    Pen,
}

/// Pointer state owned by `App`, in design-space coordinates.
/// Strokes clear on slide change.
#[derive(Debug, Default)]
pub struct Pointer {
    pub mode: PointerMode,
    pub position: Option<Point>,
    pub strokes: Vec<Vec<Point>>,
    pub drawing: bool,
}

impl Pointer {
    pub fn active(&self) -> bool {
        self.mode != PointerMode::None
    }

    /// Whether any window needs the overlay drawn.
    pub fn visible(&self) -> bool {
        self.active() || !self.strokes.is_empty()
    }

    pub fn toggle(&mut self, mode: PointerMode) {
        self.mode = if self.mode == mode {
            PointerMode::None
        } else {
            mode
        };
        self.drawing = false;
    }

    /// `position` in design-space units.
    pub fn moved(&mut self, position: Point) {
        self.position = Some(position);
        if self.drawing
            && let Some(stroke) = self.strokes.last_mut()
        {
            stroke.push(position);
        }
    }

    pub fn pressed(&mut self) {
        if self.mode == PointerMode::Pen {
            self.drawing = true;
            let start = self.position.map(|p| vec![p]).unwrap_or_default();
            self.strokes.push(start);
        }
    }

    pub fn released(&mut self) {
        self.drawing = false;
    }

    pub fn clear_strokes(&mut self) {
        self.strokes.clear();
        self.drawing = false;
    }
}

/// Whether the active iced backend is tiny-skia (`main` always sets the
/// variable, to "wgpu" under `--gpu`).
fn software_renderer() -> bool {
    static SOFTWARE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SOFTWARE
        .get_or_init(|| std::env::var("ICED_BACKEND").is_ok_and(|backend| backend == "tiny-skia"))
}

/// Canvas program drawing the overlay at one window's scale.
pub struct Overlay<'a> {
    pub pointer: &'a Pointer,
    pub accent: Color,
    /// Design-space → this window's logical pixels.
    pub scale: f32,
    /// The window's DPI scale factor (`App::window_scale_factor`).
    pub scale_factor: f32,
}

impl canvas::Program<crate::app::Message> for Overlay<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        // Second tiny-skia transform bug: the frame's clip rectangle is
        // stored already translated to the widget bounds (layer.rs
        // `draw_primitive_group`) and then translated AGAIN at present
        // time (lib.rs composes `clip_bounds * transformation`), so the
        // clip lands at 2x the widget offset, cutting everything left of
        // and above it. Start the clip rect at -bounds so the double
        // translation cancels to exactly the canvas rect.
        let frame_bounds = if software_renderer() {
            Rectangle::new(Point::new(-bounds.x, -bounds.y), bounds.size())
        } else {
            Rectangle::with_size(bounds.size())
        };
        let mut frame = canvas::Frame::with_bounds(renderer, frame_bounds);
        let s = self.scale;
        // Frame coordinates are canvas-local and the renderer translates
        // the geometry to the widget's bounds — but iced 0.14's tiny-skia
        // backend composes that translation in physical pixels instead of
        // logical ones (lib.rs draws primitive groups with
        // `transformation * scale(scale_factor)`, so the offset misses the
        // DPI scale): geometry lands at bounds/scale_factor. Add the
        // missing share of the offset here, in frame coordinates, which DO
        // scale. No-op on 1x displays and under wgpu, which composes the
        // transform correctly. Verified by calibration screenshots on 1x
        // and 2x displays.
        let missing = if software_renderer() {
            1.0 - 1.0 / self.scale_factor.max(1.0)
        } else {
            0.0
        };
        let origin = Point::new(bounds.x * missing, bounds.y * missing);
        let at = |p: Point| Point::new(origin.x + p.x * s, origin.y + p.y * s);

        for stroke in &self.pointer.strokes {
            if stroke.len() < 2 {
                continue;
            }
            let path = canvas::Path::new(|builder| {
                builder.move_to(at(stroke[0]));
                for point in &stroke[1..] {
                    builder.line_to(at(*point));
                }
            });
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_color(self.accent)
                    .with_width((STROKE_WIDTH * s).max(1.5))
                    .with_line_cap(canvas::LineCap::Round)
                    .with_line_join(canvas::LineJoin::Round),
            );
        }

        if self.pointer.mode == PointerMode::Laser
            && let Some(position) = self.pointer.position
        {
            let halo = canvas::Path::circle(at(position), (LASER_HALO * s).max(4.0));
            frame.fill(
                &halo,
                Color {
                    a: 0.25,
                    ..self.accent
                },
            );
            let dot = canvas::Path::circle(at(position), (LASER_DOT * s).max(2.0));
            frame.fill(&dot, self.accent);
        }

        vec![frame.into_geometry()]
    }
}

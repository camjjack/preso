//! Camera zoom: a box that fills the room it's given, with its child laid
//! out at its natural size along the top. The camera magnifies a focus —
//! some code lines, a diagram's node, or a point — to fill the box, so a
//! small block has the rest of the slide to zoom into. Between reveal steps
//! the camera eases from the previous step's focus to the new one.
//!
//! The magnification is a renderer transformation, not a re-layout, so the
//! child keeps its shape and nothing around it moves. Both renderers
//! rasterize text at the transformed size, so zoomed code lands sharp.
//! Pictures don't re-rasterize, so a zoomable diagram keeps rasters at a few
//! depths and [`by_depth`] draws whichever suits the magnification the
//! enclosing zooms are drawing at — sharp at every zoom, and never shrunk so
//! far that it aliases (the wgpu renderer has no mipmaps).

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{Clipboard, Shell, mouse, renderer};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Transformation};
use std::cell::Cell;
use std::time::Duration;

thread_local! {
    /// The magnification the enclosing zooms are currently drawing at: set
    /// by [`Zoom`] around its child's draw, read by [`ByDepth`].
    static DRAWING_SCALE: Cell<f32> = const { Cell::new(1.0) };
}

/// Deepest magnification a zoom will fit to. Past this a stage selecting a
/// single short line would blow it up to fill the block, which reads as a
/// glitch rather than emphasis.
pub const MAX_ZOOM: f32 = 4.0;

/// How long the camera takes to move between reveal steps.
pub const DURATION: Duration = Duration::from_millis(500);

/// Breathing room around a fitted region, as a fraction of the view on each
/// side, so zoomed-onto lines or nodes don't touch the block's edges.
const MARGIN: f32 = 0.04;

/// Where the camera looks: the centre of the view, as fractions of the
/// zoomed box, and the magnification (`1.0` = the whole box).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub x: f32,
    pub y: f32,
    pub scale: f32,
}

impl Camera {
    pub const WHOLE: Camera = Camera {
        x: 0.5,
        y: 0.5,
        scale: 1.0,
    };

    /// The camera centred on `region` (fractions of the box) that fits it
    /// with a margin, no deeper than [`MAX_ZOOM`].
    pub fn fit(region: Rectangle) -> Camera {
        let w = (region.width + 2.0 * MARGIN).max(1e-3);
        let h = (region.height + 2.0 * MARGIN).max(1e-3);
        Camera {
            x: region.x + region.width / 2.0,
            y: region.y + region.height / 2.0,
            scale: (1.0 / w).min(1.0 / h).clamp(1.0, MAX_ZOOM),
        }
    }

    /// This camera, framed on a child occupying `child` (fractions of the
    /// box): magnification within `1..=MAX_ZOOM`, and per axis —
    ///
    /// - a child at least as big as the view keeps the view inside it, so
    ///   no empty margin shows past its edges;
    /// - a smaller child sits in the view where it sat in the box: a block
    ///   at the top left grows from the top left rather than jumping to the
    ///   middle.
    ///
    /// A slide zoom's child is the whole box, which this keeps the view in.
    pub fn framed(self, child: Rectangle) -> Camera {
        let scale = if self.scale.is_finite() {
            self.scale.clamp(1.0, MAX_ZOOM)
        } else {
            1.0
        };
        Camera {
            x: frame_axis(self.x, scale, child.x, child.x + child.width),
            y: frame_axis(self.y, scale, child.y, child.y + child.height),
            scale,
        }
    }

    /// The camera `t` (0→1) of the way from `self` to `to`. Magnification
    /// interpolates geometrically, so a 1×→4× zoom doesn't rush its first
    /// half and crawl its second.
    pub fn lerp(self, to: Camera, t: f32) -> Camera {
        let t = t.clamp(0.0, 1.0);
        Camera {
            x: self.x + (to.x - self.x) * t,
            y: self.y + (to.y - self.y) * t,
            scale: self.scale.powf(1.0 - t) * to.scale.powf(t),
        }
    }

    /// The part of `bounds` in view, in the same (unzoomed) coordinates.
    fn view(self, bounds: Rectangle) -> Rectangle {
        let (w, h) = (bounds.width / self.scale, bounds.height / self.scale);
        Rectangle {
            x: bounds.x + self.x * bounds.width - w / 2.0,
            y: bounds.y + self.y * bounds.height - h / 2.0,
            width: w,
            height: h,
        }
    }
}

/// One axis of [`Camera::framed`]: the view's centre, for a camera centred
/// on `centre` at `scale` over a child spanning `c0..c1` (box fractions).
fn frame_axis(centre: f32, scale: f32, c0: f32, c1: f32) -> f32 {
    let view = 1.0 / scale;
    let span = c1 - c0;
    if span >= view {
        centre.clamp(c0 + view / 2.0, c1 - view / 2.0)
    } else {
        // Where the child sat in the box's free space (0 = at the start),
        // kept in the view's free space.
        let free = 1.0 - span;
        let at = if free > 1e-6 {
            (c0 / free).clamp(0.0, 1.0)
        } else {
            0.5
        };
        c0 - at * (view - span) + view / 2.0
    }
}

/// What a zoom looks at.
///
/// The region kinds name a node `depth` single-child wrappers down from the
/// zoomed child (`0` = the child itself), so a focus can point inside a
/// block's frame — a code panel's padding, a diagram's card — while the
/// frame zooms along with it.
#[derive(Debug, Clone, PartialEq)]
pub enum Focus {
    /// The whole box, unmagnified.
    Whole,
    /// An explicit camera on the box (a slide zoom's `x%,y%,Nx`).
    Camera(Camera),
    /// A region of the node, as fractions of its size (a diagram's nodes).
    Region { rect: Rectangle, depth: usize },
    /// Rows `first..=last` (0-based) of a node that is a column of rows each
    /// wrapping one line of text — the code block's line layout. Fits their
    /// full height, from the column's left edge to the end of the longest
    /// line's text.
    Rows {
        first: usize,
        last: usize,
        depth: usize,
    },
}

impl Focus {
    fn camera(&self, bounds: Rectangle, child: Layout<'_>) -> Camera {
        let target = match self {
            Focus::Whole => return Camera::WHOLE,
            Focus::Camera(camera) => return *camera,
            Focus::Region { rect, depth } => descend(child, *depth).map(|node| {
                let b = node.bounds();
                Rectangle {
                    x: b.x + rect.x * b.width,
                    y: b.y + rect.y * b.height,
                    width: rect.width * b.width,
                    height: rect.height * b.height,
                }
            }),
            Focus::Rows { first, last, depth } => {
                descend(child, *depth).and_then(|column| rows_bounds(column, *first, *last))
            }
        };
        target
            .and_then(|t| fractions_of(t, bounds))
            .map_or(Camera::WHOLE, Camera::fit)
    }
}

/// The node `depth` first-children down from `node`.
fn descend(node: Layout<'_>, depth: usize) -> Option<Layout<'_>> {
    let mut node = node;
    for _ in 0..depth {
        node = node.children().next()?;
    }
    Some(node)
}

/// `rect` (absolute) as fractions of `bounds`.
fn fractions_of(rect: Rectangle, bounds: Rectangle) -> Option<Rectangle> {
    (bounds.width > 0.0 && bounds.height > 0.0).then(|| Rectangle {
        x: (rect.x - bounds.x) / bounds.width,
        y: (rect.y - bounds.y) / bounds.height,
        width: rect.width / bounds.width,
        height: rect.height / bounds.height,
    })
}

/// The absolute box of rows `first..=last` of a column layout: their full
/// height, and from the column's left edge to the end of the widest line of
/// text among them. `None` if there are no such rows.
fn rows_bounds(column: Layout<'_>, first: usize, last: usize) -> Option<Rectangle> {
    let rows: Vec<Layout<'_>> = column
        .children()
        .skip(first)
        .take(last.saturating_sub(first) + 1)
        .collect();
    let (top, bottom) = (rows.first()?.bounds(), rows.last()?.bounds());
    let left = column.bounds().x;
    let right = rows
        .iter()
        .map(|row| {
            row.children()
                .next()
                .map_or(row.bounds(), |text| text.bounds())
        })
        .map(|b| b.x + b.width)
        .fold(left, f32::max);
    Some(Rectangle {
        x: left,
        y: top.y,
        width: right - left,
        height: bottom.y + bottom.height - top.y,
    })
}

pub struct Zoom<'a, Message> {
    content: Element<'a, Message>,
    from: Focus,
    to: Focus,
    /// Eased progress from `from` to `to`; `1.0` at rest.
    t: f32,
    /// Where the child sits across the box.
    align_x: iced::alignment::Horizontal,
}

/// Show `content` zoomed onto `focus`, in a box filling the room given.
pub fn zoom<'a, Message>(
    content: impl Into<Element<'a, Message>>,
    focus: Focus,
) -> Zoom<'a, Message> {
    Zoom {
        content: content.into(),
        from: Focus::Whole,
        to: focus,
        t: 1.0,
        align_x: iced::alignment::Horizontal::Left,
    }
}

impl<Message> Zoom<'_, Message> {
    /// Animate from `from`, `t` (0→1) of the way to the focus.
    pub fn from(mut self, from: Focus, t: f32) -> Self {
        self.from = from;
        self.t = t;
        self
    }

    /// Where the child sits across the box (left by default).
    pub fn align_x(mut self, align: iced::alignment::Horizontal) -> Self {
        self.align_x = align;
        self
    }

    fn camera(&self, bounds: Rectangle, child: Layout<'_>) -> Camera {
        let to = self.to.camera(bounds, child);
        let camera = if self.t >= 1.0 {
            to
        } else {
            let camera = self.from.camera(bounds, child).lerp(to, self.t);
            // Every magnification rasterizes the text afresh; snap mid-flight
            // values to the same coarse ladder a window resize uses, so a
            // zoom mints a small set of glyph sizes rather than one a frame.
            Camera {
                scale: crate::render::quantize_scale(camera.scale),
                ..camera
            }
        };
        let whole = Rectangle::new(iced::Point::ORIGIN, Size::new(1.0, 1.0));
        camera.framed(fractions_of(child.bounds(), bounds).unwrap_or(whole))
    }
}

impl<Message> Widget<Message, Theme, Renderer> for Zoom<'_, Message> {
    fn tag(&self) -> widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> widget::tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits.width(Length::Fill).height(Length::Fill);
        let child = self
            .content
            .as_widget_mut()
            .layout(tree, renderer, &limits.loose());
        let size = limits.resolve(Length::Fill, Length::Fill, child.size());
        let x = match self.align_x {
            iced::alignment::Horizontal::Left => 0.0,
            iced::alignment::Horizontal::Center => (size.width - child.size().width) / 2.0,
            iced::alignment::Horizontal::Right => size.width - child.size().width,
        };
        layout::Node::with_children(size, vec![child.move_to(iced::Point::new(x.max(0.0), 0.0))])
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content.as_widget_mut().operate(
            tree,
            layout.children().next().unwrap(),
            renderer,
            operation,
        );
    }

    // Events pass through unzoomed: a zoomed block is something to look
    // at (links on the audience slide are inert anyway).
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            tree,
            event,
            layout.children().next().unwrap(),
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            tree,
            layout.children().next().unwrap(),
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let child = layout.children().next().unwrap();
        let camera = self.camera(bounds, child);
        if camera.scale <= 1.0 {
            self.content
                .as_widget()
                .draw(tree, renderer, theme, style, child, cursor, viewport);
            return;
        }
        let Some(clip) = bounds.intersection(viewport) else {
            return;
        };
        // Map the view onto the whole box: p ↦ s·p + (origin − s·view).
        let view = camera.view(bounds);
        let s = camera.scale;
        let transformation =
            Transformation::translate(bounds.x - s * view.x, bounds.y - s * view.y)
                * Transformation::scale(s);
        use iced::advanced::Renderer as _;
        let outer = DRAWING_SCALE.get();
        DRAWING_SCALE.set(outer * s);
        renderer.with_layer(clip, |renderer| {
            renderer.with_transformation(transformation, |renderer| {
                self.content
                    .as_widget()
                    .draw(tree, renderer, theme, style, child, cursor, &view);
            });
        });
        DRAWING_SCALE.set(outer);
    }
}

/// One of several same-sized renditions of a picture, rasterized `depth`
/// times deeper, drawn according to the magnification it's seen at: the
/// shallowest at least as deep as the zoom.
///
/// Each level also carries a `warm` copy (the same picture at zero opacity)
/// that is drawn, clipped to a single pixel, whenever its level isn't the
/// one on show. The wgpu renderer uploads a large image on a worker thread,
/// leaving it blank for the frames until that finishes; drawing every level
/// all along means a zoom switching levels mid-flight finds them uploaded.
pub struct ByDepth<'a, Message> {
    /// Shallowest first.
    levels: Vec<Level<'a, Message>>,
}

pub struct Level<'a, Message> {
    pub depth: f32,
    pub shown: Element<'a, Message>,
    pub warm: Element<'a, Message>,
}

/// See [`ByDepth`]. `levels` must be non-empty.
pub fn by_depth<'a, Message>(mut levels: Vec<Level<'a, Message>>) -> ByDepth<'a, Message> {
    levels.sort_by(|a, b| a.depth.total_cmp(&b.depth));
    ByDepth { levels }
}

/// Index of the level to draw at magnification `scale`: the shallowest
/// with `depth >= scale`, else the deepest.
fn level_for(depths: impl Iterator<Item = f32>, scale: f32) -> usize {
    let depths: Vec<f32> = depths.collect();
    depths
        .iter()
        .position(|&d| d >= scale - 1e-3)
        .unwrap_or(depths.len().saturating_sub(1))
}

impl<Message> ByDepth<'_, Message> {
    /// Every level's elements, shown then warm: tree and layout children
    /// `2i` and `2i + 1` belong to level `i`.
    fn elements(&self) -> impl Iterator<Item = &Element<'_, Message>> {
        self.levels.iter().flat_map(|l| [&l.shown, &l.warm])
    }
}

impl<Message> Widget<Message, Theme, Renderer> for ByDepth<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        self.elements().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        let elements: Vec<_> = self.elements().collect();
        tree.diff_children(&elements);
    }

    fn size(&self) -> Size<Length> {
        self.levels[0].shown.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let nodes: Vec<layout::Node> = self
            .levels
            .iter_mut()
            .flat_map(|l| [&mut l.shown, &mut l.warm])
            .zip(&mut tree.children)
            .map(|(e, t)| e.as_widget_mut().layout(t, renderer, limits))
            .collect();
        layout::Node::with_children(nodes[0].size(), nodes)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        let shown = level_for(self.levels.iter().map(|l| l.depth), DRAWING_SCALE.get());
        let nodes: Vec<Layout<'_>> = layout.children().collect();
        for (i, level) in self.levels.iter().enumerate() {
            let (Some(&shown_node), Some(&warm_node)) = (nodes.get(2 * i), nodes.get(2 * i + 1))
            else {
                continue;
            };
            if i == shown {
                level.shown.as_widget().draw(
                    &tree.children[2 * i],
                    renderer,
                    theme,
                    style,
                    shown_node,
                    cursor,
                    viewport,
                );
            } else {
                let b = warm_node.bounds();
                let pixel = Rectangle::new(b.position(), Size::new(1.0, 1.0));
                renderer.with_layer(pixel, |renderer| {
                    level.warm.as_widget().draw(
                        &tree.children[2 * i + 1],
                        renderer,
                        theme,
                        style,
                        warm_node,
                        cursor,
                        &pixel,
                    );
                });
            }
        }
    }
}

impl<'a, Message: 'a> From<ByDepth<'a, Message>> for Element<'a, Message> {
    fn from(by_depth: ByDepth<'a, Message>) -> Self {
        Element::new(by_depth)
    }
}

impl<'a, Message: 'a> From<Zoom<'a, Message>> for Element<'a, Message> {
    fn from(zoom: Zoom<'a, Message>) -> Self {
        Element::new(zoom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn fit_centres_on_the_region_within_the_limits() {
        // A quarter-size region in the middle: 1 / (0.25 + margins).
        let c = Camera::fit(Rectangle::new(
            iced::Point::new(0.375, 0.375),
            Size::new(0.25, 0.25),
        ));
        assert!(close(c.x, 0.5) && close(c.y, 0.5));
        assert!(close(c.scale, 1.0 / (0.25 + 2.0 * MARGIN)), "{c:?}");

        // A sliver would zoom absurdly deep; it stops at MAX_ZOOM.
        let sliver = Camera::fit(Rectangle::new(
            iced::Point::new(0.5, 0.5),
            Size::new(0.01, 0.01),
        ));
        assert_eq!(sliver.scale, MAX_ZOOM);

        // The fitted scale follows the tighter axis: a wide, short strip is
        // limited by its width.
        let strip = Camera::fit(Rectangle::new(
            iced::Point::new(0.0, 0.4),
            Size::new(0.5, 0.05),
        ));
        assert!(close(strip.scale, 1.0 / (0.5 + 2.0 * MARGIN)), "{strip:?}");
    }

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rectangle {
        Rectangle::new(iced::Point::new(x, y), Size::new(w, h))
    }

    #[test]
    fn framing_on_the_whole_box_keeps_the_view_inside_it() {
        let whole = rect(0.0, 0.0, 1.0, 1.0);
        // Centred on the corner at 2×: the view slides in to show the
        // corner quadrant rather than anything past the edge.
        let c = Camera {
            x: 0.0,
            y: 1.0,
            scale: 2.0,
        }
        .framed(whole);
        assert_eq!((c.x, c.y), (0.25, 0.75));
        // Below 1× would show past every edge.
        let out = Camera {
            x: 0.1,
            y: 0.1,
            scale: 0.5,
        };
        assert_eq!(out.framed(whole), Camera::WHOLE);
    }

    #[test]
    fn a_big_child_is_framed_on_its_target_within_its_edges() {
        // A child filling the left 60% of the box, zoomed 4× (a view a
        // quarter wide): a target at 30% is centred, but ones near the
        // child's edges are held off so the view doesn't show past them.
        let child = rect(0.0, 0.0, 0.6, 1.0);
        let at = |x| {
            Camera {
                x,
                y: 0.5,
                scale: 4.0,
            }
            .framed(child)
            .x
        };
        assert!(close(at(0.3), 0.3));
        assert!(close(at(0.05), 0.125));
        assert!(close(at(0.59), 0.6 - 0.125));
    }

    #[test]
    fn a_small_child_keeps_its_place_as_it_grows() {
        // A block at the top left, a quarter of the box wide, at 2×: the
        // view (half the box) starts at the box's left, so the block grows
        // from the top left whatever the target.
        let top_left = rect(0.0, 0.0, 0.25, 0.1);
        let c = Camera {
            x: 0.2,
            y: 0.05,
            scale: 2.0,
        }
        .framed(top_left);
        assert!(close(c.x, 0.25) && close(c.y, 0.25), "{c:?}");
        // A centred one stays centred.
        let centred = rect(0.375, 0.45, 0.25, 0.1);
        let c = Camera {
            x: 0.4,
            y: 0.5,
            scale: 2.0,
        }
        .framed(centred);
        assert!(close(c.x, 0.5) && close(c.y, 0.5), "{c:?}");
        // At 1× framing is the identity, so an unzoomed block doesn't move.
        assert_eq!(Camera::WHOLE.framed(top_left), Camera::WHOLE);
    }

    #[test]
    fn lerp_runs_between_the_cameras_geometrically() {
        let far = Camera {
            x: 0.75,
            y: 0.25,
            scale: 4.0,
        };
        assert_eq!(Camera::WHOLE.lerp(far, 0.0), Camera::WHOLE);
        assert_eq!(Camera::WHOLE.lerp(far, 1.0), far);
        // Halfway through a 1×→4× zoom is 2×, not 2.5×.
        assert!(close(Camera::WHOLE.lerp(far, 0.5).scale, 2.0));
    }

    #[test]
    fn the_shallowest_deep_enough_level_is_drawn() {
        let depths = || [1.0, 2.0, 4.0].into_iter();
        assert_eq!(level_for(depths(), 1.0), 0);
        assert_eq!(level_for(depths(), 1.5), 1);
        assert_eq!(level_for(depths(), 2.0), 1);
        assert_eq!(level_for(depths(), 3.2), 2);
        // Past the deepest, the deepest it is.
        assert_eq!(level_for(depths(), 9.0), 2);
    }

    #[test]
    fn view_is_the_visible_part_of_the_box() {
        let bounds = Rectangle::new(iced::Point::new(100.0, 50.0), Size::new(200.0, 100.0));
        let camera = Camera {
            x: 0.75,
            y: 0.25,
            scale: 2.0,
        };
        assert_eq!(
            camera.view(bounds),
            Rectangle::new(iced::Point::new(200.0, 50.0), Size::new(100.0, 50.0))
        );
    }

    // --- Rendered: the transformation maps the focus onto the box. ---

    use iced::Color;
    use iced::widget::{column, container, row, space};

    fn render(element: Element<'_, ()>) -> impl Fn(f32, f32) -> [u8; 4] {
        crate::export::sampler(crate::export::offscreen(
            element,
            Size::new(100.0, 100.0),
            iced::Settings::default(),
            &Theme::Dark,
            0,
        ))
    }

    fn fill<'a>(r: f32, g: f32, b: f32) -> Element<'a, ()> {
        container(space())
            .width(iced::Fill)
            .height(iced::Fill)
            .style(move |_| container::background(Color::from_rgb(r, g, b)))
            .into()
    }

    /// Red | green over blue | white.
    fn quadrants<'a>() -> Element<'a, ()> {
        column![
            row![fill(1.0, 0.0, 0.0), fill(0.0, 1.0, 0.0)],
            row![fill(0.0, 0.0, 1.0), fill(1.0, 1.0, 1.0)],
        ]
        .into()
    }

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const WHITE: [u8; 4] = [255, 255, 255, 255];

    #[test]
    fn a_camera_zoom_fills_the_box_with_its_focus() {
        // 2× on the top-right quadrant's centre: all green.
        let camera = Focus::Camera(Camera {
            x: 0.75,
            y: 0.25,
            scale: 2.0,
        });
        let pixel = render(zoom(quadrants(), camera).into());
        for (x, y) in [(0.05, 0.05), (0.5, 0.5), (0.95, 0.95)] {
            assert_eq!(pixel(x, y), GREEN, "at ({x}, {y})");
        }
    }

    #[test]
    fn at_rest_unzoomed_draws_the_child_as_is() {
        let pixel = render(zoom(quadrants(), Focus::Whole).into());
        assert_eq!(pixel(0.25, 0.25), RED);
        assert_eq!(pixel(0.75, 0.75), WHITE);
    }

    #[test]
    fn mid_flight_the_camera_is_between_its_foci() {
        // Halfway from the whole box to 2× on white: ~1.41×, off-centre
        // towards white, so the centre is white and red still shows top-left.
        let to = Focus::Camera(Camera {
            x: 0.75,
            y: 0.75,
            scale: 2.0,
        });
        let pixel = render(zoom(quadrants(), to).from(Focus::Whole, 0.5).into());
        assert_eq!(pixel(0.5, 0.5), WHITE);
        assert_eq!(pixel(0.02, 0.02), RED);
    }

    #[test]
    fn rows_focus_fits_the_lines_it_names() {
        // Four rows, each a coloured "line" half the box wide. Zooming onto
        // row 2 (0-based) fits its height and that half width — so the
        // middle of the box is row 2's colour.
        let line = |r, g, b| -> Element<'_, ()> {
            container(
                container(space())
                    .width(50.0)
                    .height(25.0)
                    .style(move |_| container::background(Color::from_rgb(r, g, b))),
            )
            .width(iced::Fill)
            .into()
        };
        let lines = column![
            line(1.0, 0.0, 0.0),
            line(0.0, 0.0, 1.0),
            line(0.0, 1.0, 0.0),
            line(1.0, 1.0, 1.0),
        ];
        let pixel = render(
            zoom(
                lines,
                Focus::Rows {
                    first: 2,
                    last: 2,
                    depth: 0,
                },
            )
            .into(),
        );
        assert_eq!(pixel(0.25, 0.5), GREEN);
        assert_eq!(
            pixel(0.5, 0.5),
            GREEN,
            "the line fills the width it's fitted to"
        );
    }
}

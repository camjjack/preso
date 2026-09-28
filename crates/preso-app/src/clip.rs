//! A clip band: a wrapper that lays its child out at full size but draws only
//! the vertical strip between two fractions of its width.
//!
//! The wipe transitions need this. A `container` can't do it: a fixed-width
//! container clamps its child's layout to that width (an image squashes, text
//! re-wraps) rather than cropping it, and its `clip` only narrows the viewport,
//! which backgrounds ignore. A renderer layer clips everything drawn inside it.
//!
//! A band can also draw its child shifted within that clip — the pan
//! transition slides both slides' content along this way.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{Clipboard, Shell, mouse, renderer};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

pub struct Band<'a, Message> {
    content: Element<'a, Message>,
    from: f32,
    to: f32,
    /// How far the child is drawn moved right and down, as fractions of the
    /// width and height.
    shift: (f32, f32),
}

/// Show only the part of `content` between `from` and `to` (fractions of its
/// width, 0 = left edge, 1 = right edge). An empty band draws nothing.
pub fn band<'a, Message>(
    content: impl Into<Element<'a, Message>>,
    from: f32,
    to: f32,
) -> Band<'a, Message> {
    Band {
        content: content.into(),
        from: from.clamp(0.0, 1.0),
        to: to.clamp(0.0, 1.0),
        shift: (0.0, 0.0),
    }
}

impl<Message> Band<'_, Message> {
    /// Draw the child moved `dx` × its width to the right and `dy` × its
    /// height down (negative for left/up) — still clipped to the band, so
    /// what slides out of it is cut off at its edge. Layout is unchanged.
    pub fn shifted(mut self, dx: f32, dy: f32) -> Self {
        self.shift = (dx, dy);
        self
    }
}

impl<Message> Widget<Message, Theme, Renderer> for Band<'_, Message> {
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
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let child = self.content.as_widget_mut().layout(tree, renderer, limits);
        layout::Node::with_children(child.size(), vec![child])
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
        let Some(clip) = strip(layout.bounds(), self.from, self.to).intersection(viewport) else {
            return;
        };
        if clip.width <= 0.0 {
            return;
        }
        use iced::advanced::Renderer as _;
        let bounds = layout.bounds();
        let (dx, dy) = (self.shift.0 * bounds.width, self.shift.1 * bounds.height);
        renderer.with_layer(clip, |renderer| {
            renderer.with_translation(Vector::new(dx, dy), |renderer| {
                // The child sees the clip in its own (unshifted) terms, so
                // it culls against what actually shows.
                let visible = Rectangle {
                    x: clip.x - dx,
                    y: clip.y - dy,
                    ..clip
                };
                self.content.as_widget().draw(
                    tree,
                    renderer,
                    theme,
                    style,
                    layout.children().next().unwrap(),
                    cursor,
                    &visible,
                );
            });
        });
    }
}

impl<'a, Message: 'a> From<Band<'a, Message>> for Element<'a, Message> {
    fn from(band: Band<'a, Message>) -> Self {
        Element::new(band)
    }
}

/// The part of `bounds` between fractions `from` and `to` of its width.
fn strip(bounds: Rectangle, from: f32, to: f32) -> Rectangle {
    let x0 = bounds.x + bounds.width * from;
    let x1 = bounds.x + bounds.width * to;
    Rectangle {
        x: x0,
        width: (x1 - x0).max(0.0),
        ..bounds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_spans_the_fractions() {
        let b = Rectangle::new(iced::Point::new(10.0, 5.0), Size::new(200.0, 100.0));
        let left = strip(b, 0.0, 0.25);
        assert_eq!(
            (left.x, left.width, left.y, left.height),
            (10.0, 50.0, 5.0, 100.0)
        );
        let right = strip(b, 0.25, 1.0);
        assert_eq!((right.x, right.width), (60.0, 150.0));
    }

    /// Render `element` offscreen at 100×50 and return a pixel sampler over
    /// the snapshot (x, y as fractions of the size).
    fn render(element: Element<'_, ()>) -> impl Fn(f32, f32) -> [u8; 4] {
        crate::export::sampler(crate::export::offscreen(
            element,
            Size::new(100.0, 50.0),
            iced::Settings::default(),
            &Theme::Dark,
            0,
        ))
    }

    fn fill<'a>(c: iced::Color) -> Element<'a, ()> {
        use iced::widget::{container, space};
        container(space())
            .width(iced::Fill)
            .height(iced::Fill)
            .style(move |_| container::background(c))
            .into()
    }

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];

    #[test]
    fn a_shifted_band_slides_its_child_along() {
        use iced::Color;
        use iced::widget::{row, stack};
        // Blue|green moved a quarter to the right over red: red shows in the
        // first quarter, blue runs on to 75%, and green's right half has
        // slid out past the edge.
        let split = row![
            fill(Color::from_rgb(0.0, 0.0, 1.0)),
            fill(Color::from_rgb(0.0, 1.0, 0.0))
        ];
        let pixel = render(
            stack![
                fill(Color::from_rgb(1.0, 0.0, 0.0)),
                band(split, 0.0, 1.0).shifted(0.25, 0.0)
            ]
            .into(),
        );
        assert_eq!(pixel(0.1, 0.5), RED);
        assert_eq!(pixel(0.3, 0.5), BLUE);
        assert_eq!(pixel(0.7, 0.5), BLUE, "blue now ends at 75%");
        assert_eq!(pixel(0.9, 0.5), GREEN);
    }

    #[test]
    fn band_crops_its_child_rather_than_squashing_it() {
        use iced::Color;
        use iced::widget::{row, stack};
        // A red backdrop under a blue|green half-and-half split, of which
        // only the right three quarters are shown.
        let split = row![
            fill(Color::from_rgb(0.0, 0.0, 1.0)),
            fill(Color::from_rgb(0.0, 1.0, 0.0))
        ];
        let pixel =
            render(stack![fill(Color::from_rgb(1.0, 0.0, 0.0)), band(split, 0.25, 1.0)].into());
        assert_eq!(
            pixel(0.1, 0.5),
            RED,
            "left of the band shows what's under it"
        );
        assert_eq!(pixel(0.4, 0.5), BLUE);
        // A squashed split would still be blue here (it'd end at 62.5%).
        assert_eq!(pixel(0.55, 0.5), GREEN, "child keeps its full-width layout");
        assert_eq!(pixel(0.9, 0.5), GREEN);
    }

    #[test]
    fn empty_band_draws_nothing() {
        use iced::Color;
        use iced::widget::stack;
        let pixel = render(
            stack![
                fill(Color::from_rgb(1.0, 0.0, 0.0)),
                band(fill(Color::from_rgb(0.0, 0.0, 1.0)), 0.6, 0.6)
            ]
            .into(),
        );
        assert_eq!(pixel(0.6, 0.5), RED);
        assert_eq!(pixel(0.1, 0.5), RED);
    }

    #[test]
    fn inverted_strip_is_empty() {
        let b = Rectangle::new(iced::Point::ORIGIN, Size::new(100.0, 100.0));
        assert_eq!(strip(b, 0.8, 0.2).width, 0.0);
    }
}

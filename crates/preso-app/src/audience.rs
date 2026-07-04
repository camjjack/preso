//! The audience window: just the slide, on the virtual canvas, letterboxed.
//! When the laser pointer or pen is active, a canvas overlay stacks on top
//! of the slide and tracks the mouse.

use crate::app::{App, Message};
use crate::overlay::Overlay;
use crate::render;
use crate::transition;
use iced::widget::{canvas, center, container, image, mouse_area, stack};
use iced::{Color, Element, Fill, window};

pub fn view(app: &App, window: window::Id) -> Element<'_, Message> {
    let size = app.window_size(window);
    let raw = (size.width / render::DESIGN_WIDTH).min(size.height / render::DESIGN_HEIGHT);
    // Quantize to 1/16 steps: a live resize must not mint a fresh set of
    // glyph sizes every frame, or the shared text atlas churns (visible as
    // overlapping/mis-sized glyphs in BOTH windows).
    let scale = render::quantize_scale(raw);

    // Show the ▶ poster badge when the slide has a video but it isn't playing
    // inline — i.e. always in non-`video` builds (external player), and when an
    // embedded player failed to load. When the embedded player is up, it covers
    // the slide instead, so the badge is suppressed.
    let show_badge = app.deck.current_slide().video.is_some();
    #[cfg(feature = "video")]
    let show_badge = show_badge && app.embedded_video().is_none();

    // The slide canvas: fixed aspect, themed surface (background/gradient,
    // accent bar, logo) with the content on top. Slide transitions overlay the
    // outgoing frame on top of this live surface (see the end of this fn).
    let surface = render::slide_surface(
        app.current_slide_element(scale, true),
        &app.media,
        app.slide_theme(app.deck.current_slide()),
        &app.deck.current_slide().overrides,
        render::SurfaceOptions {
            scale,
            size: iced::Size::new(render::DESIGN_WIDTH * scale, render::DESIGN_HEIGHT * scale),
            number: Some((
                app.deck.display_number(app.deck.current_index()),
                app.deck.display_total(),
            )),
            footnote: app.deck.current_slide().footnote.clone(),
            layer_images: app.deck.current_slide().layer_images.clone(),
            video: show_badge,
            dither: true,
        },
    );

    // Embedded video (the `video` feature, wgpu live): the player draws over
    // the slide content, sized to the canvas. Without the feature it's just
    // the surface.
    #[cfg(feature = "video")]
    let surface: Element<'_, Message> = match app.embedded_video() {
        Some(video) => stack![
            surface,
            iced_video_player::VideoPlayer::new(video)
                .width(render::DESIGN_WIDTH * scale)
                .height(render::DESIGN_HEIGHT * scale)
                .content_fit(iced::ContentFit::Contain)
                .on_new_frame(Message::Noop)
        ]
        .into(),
        None => surface,
    };
    let slide = surface;

    let canvas_area: Element<'_, Message> = if app.pointer.visible() {
        let overlay = canvas(Overlay {
            pointer: &app.pointer,
            accent: render::color(app.theme.colors.accent),
            scale,
            scale_factor: app.window_scale_factor(window),
        })
        .width(render::DESIGN_WIDTH * scale)
        .height(render::DESIGN_HEIGHT * scale);

        // Convert window-local coordinates to design space so strokes
        // are shared with the presenter's miniature. Clipped so strokes
        // never escape the slide canvas.
        mouse_area(container(stack![slide, overlay]).clip(true))
            .on_move(move |p| Message::PointerMoved(iced::Point::new(p.x / scale, p.y / scale)))
            .on_press(Message::PointerPressed)
            .on_release(Message::PointerReleased)
            .into()
    } else {
        slide
    };

    // Letterbox: center the canvas on black.
    let base: Element<'_, Message> = container(center(canvas_area))
        .style(|_| container::background(Color::BLACK))
        .into();

    // Slide transition: overlay the captured outgoing frame over the live
    // incoming slide and animate it away. The screenshot covers the whole
    // window (slide + letterbox), so it fills the window exactly.
    match app.active_transition() {
        Some((handle, kind, progress)) => {
            let overlay = transition_overlay(handle.clone(), kind, progress, size);
            stack![base, overlay].into()
        }
        // At rest, render the current slide's cached frame *behind* the live
        // slide (fully occluded) so iced has finished its async GPU upload
        // before that frame is used as the next transition's overlay — without
        // this the incoming slide flashes through for the overlay's first frame.
        None => match app.warm_frame() {
            Some(handle) => stack![
                image(handle)
                    .width(Fill)
                    .height(Fill)
                    .content_fit(iced::ContentFit::Fill),
                base,
            ]
            .into(),
            None => base,
        },
    }
}

/// The outgoing-slide bitmap, animated over the live incoming slide per the
/// transition kind. `progress` runs 0→1.
fn transition_overlay(
    handle: image::Handle,
    kind: transition::Kind,
    progress: f32,
    size: iced::Size,
) -> Element<'static, Message> {
    let frame = image(handle)
        .width(Fill)
        .height(Fill)
        .content_fit(iced::ContentFit::Fill);
    match kind {
        // Cross-dissolve: fade the outgoing frame out to reveal the incoming.
        transition::Kind::Dissolve | transition::Kind::None => {
            container(frame.opacity(1.0 - progress))
                .width(Fill)
                .height(Fill)
                .into()
        }
        // Wipe: clip the outgoing frame's width down from the left edge, so
        // the incoming slide is revealed from the right. The inner frame keeps
        // the full window width and is clipped by the shrinking container.
        transition::Kind::Wipe => {
            let visible = (size.width * (1.0 - progress)).max(0.0);
            let curtain = container(
                container(frame.width(size.width).height(size.height))
                    .width(visible)
                    .height(Fill)
                    .clip(true),
            )
            .width(Fill)
            .height(Fill)
            .align_x(iced::alignment::Horizontal::Left);
            curtain.into()
        }
    }
}

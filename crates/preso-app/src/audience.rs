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

    // The ▶ badge only marks a clip that plays in an external player; an
    // inline clip is controlled from the presenter's play button and scrub
    // bar, so the audience just sees its frame (or the slide's own poster
    // until it plays).
    let show_badge = app.deck.current_slide().video.is_some() && !app.video_inline();

    // The slide canvas: fixed aspect, themed surface (background/gradient,
    // accent bar, logo) with the content on top. The screenshot transitions
    // overlay the outgoing frame on top of this live surface (see the end of
    // this fn); a content wipe splits the surface's layers instead.
    let scale_factor = app.window_scale_factor(window);
    let incoming = render::surface_layers(
        app.current_slide_element(scale, scale_factor, true, false, true),
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
            // The audience badge only marks an external-player clip, so it's
            // always ▶.
            video_playing: false,
            dither: true,
            zoom: render::SurfaceZoom::easing(
                app.deck.current_slide(),
                app.deck.current_step(),
                app.step_zoom_from(),
            ),
        },
    );
    let surface = match app.active_transition() {
        Some((kind @ (transition::Kind::ContentWipe | transition::Kind::Pan(_)), progress)) => {
            match (app.outgoing_layers(scale, scale_factor), kind) {
                (Some((outgoing, shared)), transition::Kind::Pan(direction)) => {
                    render::content_pan(outgoing, incoming, progress, direction, shared)
                }
                (Some((outgoing, shared)), _) => {
                    render::content_wipe(outgoing, incoming, 1.0 - progress, shared)
                }
                (None, _) => incoming.into_element(),
            }
        }
        _ => incoming.into_element(),
    };

    // Embedded video (the `video` feature, wgpu live): once the clip has been
    // played, the player draws over the slide content, sized to the canvas —
    // while playing, and paused on its current frame. A clip not yet played
    // isn't mounted (the poster badge stands in), so entering a video slide
    // shows the affordance, not a frozen first frame.
    #[cfg(feature = "video")]
    let surface: Element<'_, Message> = match app.embedded_video().filter(|_| app.video_on_screen())
    {
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
            scale_factor,
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
        Some((kind, progress)) => match app.transition_frame() {
            Some(handle) => {
                let overlay = transition_overlay(handle.clone(), kind, progress);
                stack![base, overlay].into()
            }
            // A content wipe: already composed into the slide above.
            None => base,
        },
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
) -> Element<'static, Message> {
    let frame = image(handle)
        .width(Fill)
        .height(Fill)
        .content_fit(iced::ContentFit::Fill);
    match kind {
        // Cross-dissolve: fade the outgoing frame out to reveal the incoming.
        transition::Kind::Dissolve
        | transition::Kind::None
        | transition::Kind::ContentWipe
        | transition::Kind::Pan(_) => container(frame.opacity(1.0 - progress))
            .width(Fill)
            .height(Fill)
            .into(),
        // Wipe: clip the outgoing frame's width down from the left edge, so
        // the incoming slide is revealed from the right. The frame keeps the
        // full window size; only its drawing is clipped (a narrowing container
        // would squash it instead).
        transition::Kind::Wipe => crate::clip::band(frame, 0.0, 1.0 - progress).into(),
    }
}

//! The presenter window: current slide, speaker notes, next slide, status.

use crate::app::{App, Message};
use crate::render;
use iced::widget::{column, container, row, scrollable, text};
use iced::{Element, Fill, FillPortion, window};

/// The next-slide preview's fixed scale (1/16 ladder, like everything).
const NEXT_PREVIEW_SCALE: f32 = 3.0 / 16.0;

pub fn view(app: &App, window: window::Id) -> Element<'_, Message> {
    let deck = &app.deck;
    let slide = deck.current_slide();

    // Current-slide preview scale follows the presenter window size, so
    // the preview is a faithful miniature of the audience surface.
    let size = app.window_size(window);
    let avail_width = (size.width - 48.0).max(300.0);
    let avail_height = (size.height - 320.0).max(180.0);
    let current_scale = render::quantize_scale(
        (avail_width / render::DESIGN_WIDTH).min(avail_height / render::DESIGN_HEIGHT),
    );

    // Overview grid replaces the whole presenter surface while open; its
    // column count fills the window width.
    if let Some(grid) = app.overview_element(size.width) {
        let muted = render::color(app.theme.colors.muted);
        return column![
            text("Overview — click a slide to jump, Esc to close")
                .size(13)
                .color(muted),
            grid,
        ]
        .spacing(8)
        .padding(14)
        .into();
    }

    let mut status = format!("Slide {}/{}", deck.current_index() + 1, deck.len());
    if slide.step_count() > 1 {
        status.push_str(&format!(
            "  |  step {}/{}",
            deck.current_step() + 1,
            slide.step_count()
        ));
    }
    if !app.jump_buffer.is_empty() {
        status.push_str(&format!("  |  jump to: {}", app.jump_buffer));
    }
    if slide.video.is_some() {
        status.push_str("  |  ▶ V: play video");
    }

    let muted = render::color(app.theme.colors.muted);
    let accent = render::color(app.theme.colors.accent);

    let mut status_row = vec![];
    if let Some(timer) = app.timer_display() {
        status.push_str(&format!("  |  {}", timer.elapsed));
        if let Some((remaining, warn)) = timer.remaining {
            status_row.push(text(status.clone()).size(14).color(muted).into());
            let color = if warn {
                iced::Color::from_rgb8(0xf3, 0x8b, 0xa8)
            } else {
                muted
            };
            status_row.push(
                text(format!("  ({remaining} left)"))
                    .size(14)
                    .color(color)
                    .into(),
            );
        }
    }
    if status_row.is_empty() {
        status_row.push(text(status).size(14).color(muted).into());
    }

    // Just the status line. The error (when present) is overlaid on the
    // slide below rather than taking its own header row, so it never shrinks
    // the slide and we don't reserve vertical space for it.
    let header: Element<'_, Message> = iced::widget::Row::with_children(status_row).into();

    // Faithful miniature of the audience surface (theme background,
    // accent bar, logo), centered in the available space. While a pointer
    // mode is active the markdown renders inert: pen presses should draw,
    // not accidentally open links.
    let surface = render::slide_surface(
        app.current_slide_element(current_scale, app.pointer.active()),
        &app.media,
        app.slide_theme(app.deck.current_slide()),
        &app.deck.current_slide().overrides,
        render::SurfaceOptions {
            scale: current_scale,
            size: iced::Size::new(
                render::DESIGN_WIDTH * current_scale,
                render::DESIGN_HEIGHT * current_scale,
            ),
            number: Some((
                deck.display_number(deck.current_index()),
                deck.display_total(),
            )),
            footnote: deck.current_slide().footnote.clone(),
            layer_images: deck.current_slide().layer_images.clone(),
            video: deck.current_slide().video.is_some(),
            dither: true,
        },
    );
    // Laser/pen work from here too: positions convert to design space,
    // so they mirror live on the audience window (and vice versa).
    let surface: Element<'_, Message> = if app.pointer.visible() {
        let overlay = iced::widget::canvas(crate::overlay::Overlay {
            pointer: &app.pointer,
            accent,
            scale: current_scale,
            scale_factor: app.window_scale_factor(window),
        })
        .width(render::DESIGN_WIDTH * current_scale)
        .height(render::DESIGN_HEIGHT * current_scale);
        // Clip so strokes near the slide edge can't bleed over the
        // presenter chrome.
        iced::widget::mouse_area(container(iced::widget::stack![surface, overlay]).clip(true))
            .on_move(move |p| {
                Message::PointerMoved(iced::Point::new(p.x / current_scale, p.y / current_scale))
            })
            .on_press(Message::PointerPressed)
            .on_release(Message::PointerReleased)
            .into()
    } else {
        surface
    };
    let current = container(surface)
        .width(Fill)
        .height(Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center);
    // A parse/reload problem shows as a banner overlaid on the top of the
    // slide area (it doesn't push anything down).
    let current: Element<'_, Message> = if let Some(error) = &app.error {
        let banner = container(text(format!("Problem: {error}")).size(14))
            .padding(6)
            .style(container::danger);
        iced::widget::stack![
            current,
            container(banner)
                .width(Fill)
                .padding(8)
                .align_x(iced::alignment::Horizontal::Center),
        ]
        .into()
    } else {
        current.into()
    };

    // Notes: one wrapped paragraph per note, in document order.
    let note_paragraphs: Vec<Element<'_, Message>> = slide
        .notes_at(deck.current_step())
        .map(|n| text(n.text.clone()).size(18).width(Fill).into())
        .collect();
    let notes: Element<'_, Message> = if note_paragraphs.is_empty() {
        text("no notes for this slide").size(14).color(muted).into()
    } else {
        column(note_paragraphs).spacing(12).width(Fill).into()
    };
    let notes_panel = column![
        text("Notes").size(12).color(accent),
        container(scrollable(notes).width(Fill))
            .padding(10)
            .width(Fill)
            .height(Fill)
            .style(container::bordered_box),
    ]
    .spacing(4)
    .width(FillPortion(3));

    let next: Element<'_, Message> = match app.next_preview(NEXT_PREVIEW_SCALE) {
        Some(element) => element,
        None => text("end of deck").size(13).color(muted).into(),
    };
    let next_panel = column![
        text("Next").size(12).color(muted),
        container(next)
            .width(Fill)
            .height(Fill)
            .align_x(iced::alignment::Horizontal::Center),
    ]
    .spacing(4)
    .width(FillPortion(2));

    // Notes on the left (wider), next-slide preview on the right.
    let bottom = row![notes_panel, next_panel].spacing(12).height(240);

    let help = text(
        "Left/Right: navigate    number+Enter: jump    Esc: overview    F: fullscreen    \
         L: laser    P: pen    C: clear    R: reset timer",
    )
    .size(12)
    .color(muted);

    column![header, current, bottom, help]
        .spacing(10)
        .padding(14)
        .into()
}

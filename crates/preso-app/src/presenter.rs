//! The presenter window: current slide, speaker notes, next slide, status.
//!
//! Two layouts arrange the same parts (see [`Layout`]); the status line, help
//! line and overview grid are shared by both.

use crate::app::{App, Message};
use crate::render;
use iced::widget::{Column, column, container, row, scrollable, text};
use iced::{Element, Fill, FillPortion, Size, Task, window};

/// The next-slide preview's fixed scale in the slide layout (1/16 ladder,
/// like everything).
const NEXT_PREVIEW_SCALE: f32 = 3.0 / 16.0;

/// The notes scrollable's id, so navigation can reset it to the top.
const NOTES_SCROLL_ID: &str = "preso-notes-scroll";

/// How the presenter window arranges its parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    /// The current slide fills the window; notes and the next preview share
    /// a strip beneath it.
    #[default]
    Slide,
    /// Notes fill the left of the window at full height, in larger type; the
    /// current and next slides stack in a column on the right.
    Notes,
}

impl Layout {
    /// The deck's `presenter:` frontmatter value. Unknown values fall back to
    /// the default, like an unknown `transition:`.
    pub fn from_frontmatter(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            None | Some("slide") => Self::Slide,
            Some("notes") => Self::Notes,
            Some(other) => {
                tracing::warn!(value = other, "unknown presenter layout, using `slide`");
                Self::Slide
            }
        }
    }

    /// The layout `N` switches to.
    pub fn next(self) -> Self {
        match self {
            Self::Slide => Self::Notes,
            Self::Notes => Self::Slide,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Slide => "Slide",
            Self::Notes => "Notes",
        }
    }
}

/// Scroll the notes back to the top (on a slide change).
pub fn scroll_notes_to_top() -> Task<Message> {
    iced::widget::operation::snap_to(
        NOTES_SCROLL_ID,
        iced::widget::scrollable::RelativeOffset { x: 0.0, y: 0.0 },
    )
}

pub fn view(app: &App, window: window::Id) -> Element<'_, Message> {
    let size = app.window_size(window);

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

    let body = match app.presenter_layout {
        Layout::Slide => slide_layout(app, window, size),
        Layout::Notes => notes_layout(app, window, size),
    };
    column![status_line(app), body, help_line(app)]
        .spacing(10)
        .padding(14)
        .into()
}

/// The current slide large and centred, with notes (left, wider) and the
/// next-slide preview (right) in a strip beneath.
fn slide_layout(app: &App, window: window::Id, size: Size) -> Element<'_, Message> {
    // Current-slide preview scale follows the presenter window size, so
    // the preview is a faithful miniature of the audience surface.
    let avail_width = (size.width - 48.0).max(300.0);
    let avail_height = (size.height - 320.0).max(180.0);
    let scale = render::quantize_scale(
        (avail_width / render::DESIGN_WIDTH).min(avail_height / render::DESIGN_HEIGHT),
    );
    let current = with_error_banner(
        app,
        container(current_slide(app, window, scale))
            .width(Fill)
            .height(Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .into(),
    );
    let bottom = row![
        notes_panel(app, 18.0).width(FillPortion(3)),
        next_panel(app, NEXT_PREVIEW_SCALE, false).width(FillPortion(2)),
    ]
    .spacing(12)
    .height(240);
    column![current, bottom].spacing(10).into()
}

/// Notes at full height on the left, in type sized to the column; the
/// current slide over the next preview in a column on the right.
fn notes_layout(app: &App, window: window::Id, size: Size) -> Element<'_, Message> {
    const GAP: f32 = 12.0;
    // The right column's share of the width, and the smallest the next
    // preview may be relative to the current slide above it.
    const SIDE_SHARE: f32 = 0.38;
    const MIN_NEXT_RATIO: f32 = 0.75;

    // Less the window padding (2 × 14), the status and help lines with their
    // gaps, the two labels and the preview frames.
    let inner_width = (size.width - 28.0).max(300.0);
    let side_height = (size.height - 100.0 - 2.0 * 20.0 - GAP).max(180.0);
    let scale = render::quantize_scale(
        (inner_width * SIDE_SHARE / render::DESIGN_WIDTH)
            .min(side_height / (render::DESIGN_HEIGHT * (1.0 + MIN_NEXT_RATIO))),
    );
    // The column is exactly as wide as the slide, so no width is wasted
    // beside it; the notes take the rest.
    let side_width = render::DESIGN_WIDTH * scale + 2.0;
    // The next preview takes the height left under the current slide, up to
    // the same size.
    let next_scale = render::quantize_scale(
        ((side_height - render::DESIGN_HEIGHT * scale) / render::DESIGN_HEIGHT).min(scale),
    );

    // ~60 characters to a line at the column's width (less its padding and
    // scrollbar), in whole points, capped so a wide screen doesn't shout.
    let notes_width = inner_width - side_width - GAP - 40.0;
    let notes_size = (notes_width / 30.0).round().clamp(20.0, 32.0);

    // Labelled like "Notes" and "Next", which also lines the frame up with
    // the notes box beside it.
    let current = column![
        text("Current")
            .size(12)
            .color(render::color(app.theme.colors.muted)),
        with_error_banner(app, framed(app, current_slide(app, window, scale))),
    ]
    .spacing(4);
    let side = column![current, next_panel(app, next_scale, true)]
        .spacing(GAP)
        .width(side_width);
    row![notes_panel(app, notes_size).width(Fill), side]
        .spacing(GAP)
        .height(Fill)
        .into()
}

/// Slide number, reveal step, pending jump, video and author-mode state, the
/// timer, and any transient hint — with the layout switch at the far right.
fn status_line(app: &App) -> Element<'_, Message> {
    let deck = &app.deck;
    let slide = deck.current_slide();
    let muted = render::color(app.theme.colors.muted);
    let accent = render::color(app.theme.colors.accent);

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
        if app.video_playing() {
            status.push_str("  |  ▮▮ Space: pause  ◀ ⌥◀: rewind");
        } else {
            status.push_str("  |  ▶ Space: play video");
        }
    }
    if app.authoring {
        status.push_str("  |  ✎ HIGHLIGHT AUTHOR (H)");
    }

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
    // Author-mode hint (drag result or prompt), in accent so it reads as a
    // transient confirmation rather than chrome.
    if let Some(hint) = &app.toast {
        status_row.push(text(format!("  |  {hint}")).size(14).color(accent).into());
    }

    // The same spot in every layout, so clicking it again switches back.
    status_row.push(iced::widget::Space::new().width(Fill).into());
    status_row.push(layout_switch(app));

    // Just the status line. The error (when present) is overlaid on the
    // slide rather than taking its own header row, so it never shrinks the
    // slide and we don't reserve vertical space for it.
    iced::widget::Row::with_children(status_row)
        .align_y(iced::alignment::Vertical::Center)
        .into()
}

/// The clickable "Layout: …" chip: a click does what `N` does.
fn layout_switch(app: &App) -> Element<'_, Message> {
    let muted = render::color(app.theme.colors.muted);
    let label = text(format!("Layout: {}", app.presenter_layout.label()))
        .size(12)
        .color(muted);
    iced::widget::mouse_area(
        container(label)
            .padding([2, 8])
            .style(container::bordered_box),
    )
    .on_press(Message::CyclePresenterLayout)
    .interaction(iced::mouse::Interaction::Pointer)
    .into()
}

fn help_line(app: &App) -> Element<'_, Message> {
    text(
        "Left/Right: navigate    number+Enter: jump    Esc: overview    F: fullscreen    \
         L: laser    P: pen    C: clear    R: reset timer    N: layout",
    )
    .size(12)
    .color(render::color(app.theme.colors.muted))
    .into()
}

/// Faithful miniature of the audience surface (theme background, accent
/// bar, logo) at `scale`. While a pointer mode is active the markdown
/// renders inert: pen presses should draw, not accidentally open links.
fn current_slide(app: &App, window: window::Id, scale: f32) -> Element<'_, Message> {
    let deck = &app.deck;
    let slide = deck.current_slide();
    let surface = render::slide_surface(
        app.current_slide_element(
            scale,
            app.window_scale_factor(window),
            app.pointer.active(),
            app.authoring,
        ),
        &app.media,
        app.slide_theme(slide),
        &slide.overrides,
        render::SurfaceOptions {
            scale,
            size: Size::new(render::DESIGN_WIDTH * scale, render::DESIGN_HEIGHT * scale),
            number: Some((
                deck.display_number(deck.current_index()),
                deck.display_total(),
            )),
            footnote: slide.footnote.clone(),
            layer_images: slide.layer_images.clone(),
            video: slide.video.is_some(),
            // Presenter never renders the clip itself, so the badge is its only
            // feedback: ⏸ while it plays on the audience window, ▶ otherwise.
            video_playing: app.video_playing(),
            dither: true,
        },
    );
    // Laser/pen work from here too: positions convert to design space,
    // so they mirror live on the audience window (and vice versa). Suppressed
    // in author mode: its full-slide mouse_area would swallow drags meant for
    // the per-image author canvas (leftover strokes can keep `visible()` true).
    if app.pointer.visible() && !app.authoring {
        let overlay = iced::widget::canvas(crate::overlay::Overlay {
            pointer: &app.pointer,
            accent: render::color(app.theme.colors.accent),
            scale,
            scale_factor: app.window_scale_factor(window),
        })
        .width(render::DESIGN_WIDTH * scale)
        .height(render::DESIGN_HEIGHT * scale);
        // Clip so strokes near the slide edge can't bleed over the
        // presenter chrome.
        iced::widget::mouse_area(container(iced::widget::stack![surface, overlay]).clip(true))
            .on_move(move |p| Message::PointerMoved(iced::Point::new(p.x / scale, p.y / scale)))
            .on_press(Message::PointerPressed)
            .on_release(Message::PointerReleased)
            .into()
    } else {
        surface
    }
}

/// A hairline around a slide miniature, so its edge shows even when the
/// slide's background matches the presenter's.
fn framed<'a>(app: &App, slide: Element<'a, Message>) -> Element<'a, Message> {
    let edge = render::color(app.theme.colors.muted).scale_alpha(0.5);
    container(slide)
        .padding(1)
        .style(move |_| container::Style {
            border: iced::Border {
                color: edge,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// A parse/reload problem shows as a banner overlaid on the top of the
/// slide area (it doesn't push anything down).
fn with_error_banner<'a>(app: &'a App, slide: Element<'a, Message>) -> Element<'a, Message> {
    let Some(error) = &app.error else {
        return slide;
    };
    let banner = container(text(format!("Problem: {error}")).size(14))
        .padding(6)
        .style(container::danger);
    iced::widget::stack![
        slide,
        container(banner)
            .width(Fill)
            .padding(8)
            .align_x(iced::alignment::Horizontal::Center),
    ]
    .into()
}

/// Notes: one wrapped paragraph per note, in document order, at `size`.
/// A step note the talk has moved past is muted, so the one for the step on
/// screen stands out.
fn notes_panel(app: &App, size: f32) -> Column<'_, Message> {
    let slide = app.deck.current_slide();
    let step = app.deck.current_step();
    let muted = render::color(app.theme.colors.muted);
    let paragraphs: Vec<Element<'_, Message>> = slide
        .notes_at(step)
        .map(|note| {
            let paragraph = text(note.text.clone()).size(size).width(Fill);
            if note.step.is_some_and(|s| s < step) {
                paragraph.color(muted).into()
            } else {
                paragraph.into()
            }
        })
        .collect();
    let notes: Element<'_, Message> = if paragraphs.is_empty() {
        text("no notes for this slide").size(14).color(muted).into()
    } else {
        column(paragraphs)
            .spacing((size * 2.0 / 3.0).round())
            .width(Fill)
            .into()
    };
    column![
        text("Notes")
            .size(12)
            .color(render::color(app.theme.colors.accent)),
        // An embedded scrollbar (`spacing`) takes its own lane; the default
        // floats over the text and hides the end of every line.
        container(scrollable(notes).id(NOTES_SCROLL_ID).spacing(8).width(Fill))
            .padding(10)
            .width(Fill)
            .height(Fill)
            .style(container::bordered_box),
    ]
    .spacing(4)
}

/// What the next press shows, at `scale`, under a "Next" label, optionally
/// [`framed`].
fn next_panel(app: &App, scale: f32, frame: bool) -> Column<'_, Message> {
    let muted = render::color(app.theme.colors.muted);
    let next: Element<'_, Message> = match app.next_preview(scale) {
        Some(element) if frame => framed(app, element),
        Some(element) => element,
        None => text("end of deck").size(13).color(muted).into(),
    };
    column![
        text("Next").size(12).color(muted),
        container(next)
            .width(Fill)
            .height(Fill)
            .align_x(iced::alignment::Horizontal::Center),
    ]
    .spacing(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_from_frontmatter() {
        assert_eq!(Layout::from_frontmatter(None), Layout::Slide);
        assert_eq!(Layout::from_frontmatter(Some("slide")), Layout::Slide);
        assert_eq!(Layout::from_frontmatter(Some(" notes ")), Layout::Notes);
        // Unknown values fall back to the default rather than erroring.
        assert_eq!(
            Layout::from_frontmatter(Some("teleprompter")),
            Layout::Slide
        );
    }

    #[test]
    fn layout_cycles_through_every_layout() {
        assert_eq!(Layout::Slide.next(), Layout::Notes);
        assert_eq!(Layout::Notes.next(), Layout::Slide);
    }
}

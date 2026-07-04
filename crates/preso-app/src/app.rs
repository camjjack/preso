use crate::media::Media;
use crate::overlay::{Pointer, PointerMode};
use crate::render::SlideContext;
use crate::timer::{Clock, SystemClock, Timer, TimerDisplay};
use crate::window_state::{Geometry, WindowState};
use crate::{audience, hot_reload, keyboard, presenter, render, window_state};
use iced::widget::{markdown, row, text};
use iced::{Element, Fill, Point, Size, Subscription, Task, Theme, window};
use preso_core::Deck;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// One rendered column of a two-column slide. The sub-slide re-parse
/// supplies per-column code-block metadata.
pub struct Column {
    content: markdown::Content,
    slide: preso_core::Slide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Presenter,
    Audience,
}

/// A slide transition animating on the audience window: the captured outgoing
/// frame, the effect, and when it began.
struct ActiveTransition {
    kind: crate::transition::Kind,
    outgoing: iced::widget::image::Handle,
    started: std::time::Instant,
}

/// A navigation intent, so the deck mutation can be deferred until the
/// outgoing slide has been screenshotted for a transition.
#[derive(Debug, Clone, Copy)]
enum Nav {
    Next,
    Prev,
    First,
    Last,
    /// Jump to a 0-based slide index.
    Goto(usize),
}

#[derive(Debug, Clone)]
pub enum Message {
    Opened(Role, window::Id),
    InitialGeometry(window::Id, Option<Point>, Size),
    /// A window's DPI scale factor, fetched at open and on `Rescaled`.
    ScaleFactor(window::Id, f32),
    Window(window::Id, window::Event),
    Key(iced::keyboard::Event),
    /// Overview grid: jump to a slide (0-based) and close the grid.
    OverviewJump(usize),
    /// Cursor moved over the audience slide area (logical coords).
    PointerMoved(Point),
    PointerPressed,
    PointerReleased,
    LinkClicked(markdown::Uri),
    FileChanged,
    /// A frame was rendered while a capture is pending: the new content is now
    /// on screen, so take the (now-correct) at-rest screenshot.
    CaptureTick,
    /// An at-rest screenshot of the slide at `(index, step)` finished: cache it
    /// as that slide's clean frame for use as a future transition's outgoing.
    CaptureFrame(usize, usize, iced::widget::image::Handle),
    /// One-second heartbeat while the talk timer runs (refreshes views).
    Tick,
    /// Periodic display-topology check: re-detect monitors so a screen
    /// plugged in (or unplugged) after launch re-runs auto placement.
    PollDisplays,
    /// Swallowed interactions (e.g. audience-window link clicks).
    Noop,
}

pub struct App {
    path: PathBuf,
    pub theme: preso_style::Theme,
    /// `theme` with the `[title]` / `[section]` overlays applied, for
    /// slides marked `<!-- slide: kind=... -->`.
    title_theme: preso_style::Theme,
    section_theme: preso_style::Theme,
    iced_theme: Theme,
    pub deck: Deck,
    /// Markdown content for the current slide at the current step.
    pub current_md: markdown::Content,
    /// Two-column split of the current step, when the slide uses
    /// `<!-- layout: TwoColumn -->`.
    current_cols: Option<(Column, Column)>,
    /// Markdown content for the next slide (fully revealed).
    pub next_md: Option<markdown::Content>,
    /// Markdown for the current slide's *next reveal step*, when the slide
    /// has more `<!-- pause -->` steps. Drives the presenter "Next" panel.
    next_preview_md: Option<markdown::Content>,
    /// Two-column split for whatever the "Next" panel previews (the current
    /// slide's next step, or the next slide), when it's a TwoColumn slide.
    next_cols: Option<(Column, Column)>,
    /// Per-slide two-column splits for the overview grid (parsed lazily with
    /// `overview_md`); `None` for single-column slides.
    overview_cols: Option<Vec<Option<(Column, Column)>>>,
    /// Rendered-media cache (math, mermaid, images).
    pub media: Media,
    /// Sticky banner: last hot-reload/parse problem (presenter-only display).
    pub error: Option<String>,
    windows: BTreeMap<window::Id, Role>,
    sizes: BTreeMap<window::Id, Size>,
    /// Per-window DPI scale factors; the pointer overlay needs them to
    /// counter a tiny-skia translation bug (see overlay.rs).
    scale_factors: BTreeMap<window::Id, f32>,
    audience_fullscreen: bool,
    /// Persisted geometry, updated live and saved when a window closes.
    window_state: WindowState,
    /// `window::Event::Opened` geometry that arrived before the role
    /// registration (`Message::Opened`) for that window.
    pending_opened: BTreeMap<window::Id, (Option<Point>, Size)>,
    /// Digits typed for `<number><Enter>` slide jumps.
    pub jump_buffer: String,
    /// Presenter overview grid (Esc). Thumbnails parse lazily on first
    /// open and are invalidated by reload.
    overview: bool,
    overview_md: Option<Vec<markdown::Content>>,
    /// Laser pointer / pen annotation state (audience window).
    pub pointer: Pointer,
    timer: Timer,
    clock: Box<dyn Clock>,
    /// CLI `--theme` wins over frontmatter, including on hot reload.
    theme_override: Option<String>,
    /// Dual-display auto placement: fullscreen the audience window and
    /// maximize the presenter when they open.
    auto_place: bool,
    /// Whether the last display poll saw a secondary monitor. Live
    /// placement only fires on a *transition* (a screen plugged in or
    /// unplugged), so manual window moves aren't clobbered each poll.
    secondary_present: bool,
    /// A slide transition in flight on the audience window: the captured
    /// outgoing frame plus its kind and start instant. See `transition`.
    transition: Option<ActiveTransition>,
    /// Screenshot of the current slide taken while it's *at rest* (no
    /// transition running), keyed by `(slide index, reveal step)`. Reused as
    /// the outgoing frame when navigating away — so a transition needs no
    /// screenshot on the critical path (the readback stalls the event loop, and
    /// at rest that stall is invisible; mid-transition it would flash).
    frame_cache: Option<(usize, usize, iced::widget::image::Handle)>,
    /// The current slide changed and wants a fresh `frame_cache` entry. The
    /// screenshot is deferred to the next rendered frame (via `window::frames`)
    /// so it captures the *new* content, not whatever was on screen when the
    /// change was applied (the screenshot reads the last-drawn frame).
    pending_capture: bool,
    /// App start, the epoch for GIF animation time.
    epoch: std::time::Instant,
    /// Whether the current step shows any animated GIF.
    current_has_gif: bool,
    /// Whether wgpu is the live backend. Embedded video only renders under
    /// wgpu, so this gates inline playback vs. the external-player fallback.
    /// Only read in `video`-feature builds.
    #[cfg_attr(not(feature = "video"), allow(dead_code))]
    gpu_active: bool,
    /// The GStreamer-backed video for the current slide (when built with the
    /// `video` feature, wgpu is live, and the slide has a `<!-- video: … -->`).
    /// Owns the pipeline; dropped/rebuilt as the current slide changes.
    #[cfg(feature = "video")]
    embedded: Option<crate::video::Embedded>,
}

/// Overview grid layout: thumbnail scale, gap between thumbnails, the room
/// reserved for window padding + scrollbar, the scrollable's id, and the page
/// jump (≈3 thumbnail rows) for PageUp/PageDown.
const OVERVIEW_SCALE: f32 = 2.0 / 16.0;
const OVERVIEW_GAP: f32 = 10.0;
const OVERVIEW_MARGIN: f32 = 52.0;
const OVERVIEW_SCROLL_ID: &str = "preso-overview-scroll";
const OVERVIEW_PAGE: f32 = 3.0 * render::DESIGN_HEIGHT * OVERVIEW_SCALE;

/// How many thumbnail columns fit in a presenter window of `window_width`,
/// at the overview scale — so the grid fills wide screens instead of a fixed
/// four columns. At least one.
fn overview_columns(window_width: f32) -> usize {
    let thumb = render::DESIGN_WIDTH * OVERVIEW_SCALE;
    let avail = (window_width - OVERVIEW_MARGIN).max(thumb);
    (((avail + OVERVIEW_GAP) / (thumb + OVERVIEW_GAP)).floor() as usize).max(1)
}

/// Relative scroll offset (0=top, 1=bottom) that brings the current slide's
/// row into view in the overview, biased to show the slides before it.
fn overview_scroll_y(current_index: usize, total: usize, columns: usize) -> f32 {
    let columns = columns.max(1);
    let num_rows = total.div_ceil(columns).max(1);
    if num_rows <= 1 {
        return 0.0;
    }
    let current_row = current_index / columns;
    (current_row as f32 / (num_rows - 1) as f32).clamp(0.0, 1.0)
}

/// Parse a [`Slide::column_slides`] split into the two [`Column`]s the views
/// render (each side's markdown content plus its re-parsed sub-slide).
fn columns_of(
    split: ((String, preso_core::Slide), (String, preso_core::Slide)),
) -> (Column, Column) {
    let ((ls, lslide), (rs, rslide)) = split;
    (
        Column {
            content: markdown::Content::parse(&ls),
            slide: lslide,
        },
        Column {
            content: markdown::Content::parse(&rs),
            slide: rslide,
        },
    )
}

/// A subscription that ticks every `period`, driven by a dedicated thread.
/// iced's own `time::every` needs an async-runtime feature (tokio/smol),
/// which preso avoids (AGENTS.md: std channels until genuinely needed).
/// Subscriptions with different periods are distinct; the thread exits when
/// the subscription is dropped and its channel closes.
fn every(period: std::time::Duration) -> Subscription<()> {
    Subscription::run_with(period, |period| {
        let period = *period;
        iced::stream::channel(2, async move |tx| {
            std::thread::spawn(move || {
                let mut tx = tx;
                loop {
                    std::thread::sleep(period);
                    if tx.try_send(()).is_err() {
                        return;
                    }
                }
            });
            // Keep the stream alive; items arrive from the ticker thread.
            std::future::pending::<()>().await;
        })
    })
}

/// The window/taskbar icon, decoded from the bundled logo. Honored on
/// Windows and X11; macOS takes its icon from the app bundle and Wayland
/// from the `.desktop` entry, so both ignore a per-window icon.
fn window_icon() -> Option<window::Icon> {
    let logo = include_bytes!("../../../assets/preso-logo-512.png");
    let rgba = ::image::load_from_memory_with_format(logo, ::image::ImageFormat::Png)
        .ok()?
        .to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    window::icon::from_rgba(rgba.into_raw(), width, height).ok()
}

/// Directories searched for `<name>.toml` user themes.
pub fn theme_search_dirs() -> Vec<PathBuf> {
    dirs::config_dir()
        .map(|d| vec![d.join("preso").join("themes")])
        .unwrap_or_default()
}

impl App {
    pub fn new(
        path: PathBuf,
        source: String,
        theme: preso_style::Theme,
        theme_override: Option<String>,
        audience_only: bool,
        duration_minutes: Option<u64>,
        gpu_active: bool,
    ) -> (Self, Task<Message>) {
        let (deck, error) = match Deck::from_source(&source) {
            Ok(deck) => (deck, None),
            Err(e) => (
                Deck::from_source("").expect("empty deck always parses"),
                Some(format!("{}: {e}", path.display())),
            ),
        };

        let saved = window_state::load();
        let icon = window_icon();
        let settings_for = |geometry: Option<Geometry>, default_size: Size| window::Settings {
            size: geometry.map_or(default_size, |g| Size::new(g.width, g.height)),
            position: geometry.map_or(window::Position::Default, |g| {
                window::Position::Specific(Point::new(g.x, g.y))
            }),
            icon: icon.clone(),
            ..window::Settings::default()
        };

        // Plan §15 default behavior: with a second display, the audience
        // window goes fullscreen there and the presenter is maximized on
        // the primary; saved geometry only drives single-display runs.
        let setup = crate::monitors::detect();
        let secondary = setup.and_then(|s| s.secondary);

        let audience_settings = match secondary {
            Some(monitor) => window::Settings {
                size: Size::new(1280.0, 720.0),
                position: window::Position::Specific(Point::new(
                    monitor.x + 40.0,
                    monitor.y + 40.0,
                )),
                icon: icon.clone(),
                ..window::Settings::default()
            },
            None => settings_for(saved.audience, Size::new(960.0, 540.0)),
        };
        let (_, open_audience) = window::open(audience_settings);
        let mut tasks = vec![open_audience.map(|id| Message::Opened(Role::Audience, id))];

        if !audience_only {
            let presenter_settings = match (secondary, setup) {
                // Dual display: pin the presenter to the primary, ready
                // to be maximized once it opens.
                (Some(_), Some(s)) => window::Settings {
                    size: Size::new(1100.0, 760.0),
                    position: window::Position::Specific(Point::new(
                        s.primary.x + 60.0,
                        s.primary.y + 60.0,
                    )),
                    icon: icon.clone(),
                    ..window::Settings::default()
                },
                _ => settings_for(saved.presenter, Size::new(1100.0, 760.0)),
            };
            let (_, open_presenter) = window::open(presenter_settings);
            tasks.push(open_presenter.map(|id| Message::Opened(Role::Presenter, id)));
        }
        let auto_place = secondary.is_some();

        let iced_theme = render::iced_theme(&theme);
        let media = Media::new(&path);
        let title_theme = theme.title.apply(&theme);
        let section_theme = theme.section.apply(&theme);
        let mut app = Self {
            path,
            theme,
            title_theme,
            section_theme,
            iced_theme,
            deck,
            current_md: markdown::Content::parse(""),
            current_cols: None,
            next_md: None,
            next_preview_md: None,
            next_cols: None,
            overview_cols: None,
            media,
            error,
            windows: BTreeMap::new(),
            sizes: BTreeMap::new(),
            scale_factors: BTreeMap::new(),
            audience_fullscreen: false,
            window_state: saved,
            pending_opened: BTreeMap::new(),
            jump_buffer: String::new(),
            overview: false,
            overview_md: None,
            pointer: Pointer::default(),
            timer: Timer::new(duration_minutes),
            clock: Box::new(SystemClock),
            theme_override,
            auto_place,
            secondary_present: secondary.is_some(),
            transition: None,
            frame_cache: None,
            pending_capture: false,
            epoch: SystemClock.now(),
            current_has_gif: false,
            gpu_active,
            #[cfg(feature = "video")]
            embedded: None,
        };
        app.refresh_markdown();
        (app, Task::batch(tasks))
    }

    /// Time since app start, for GIF frame selection.
    pub fn animation_time(&self) -> std::time::Duration {
        self.clock.now().saturating_duration_since(self.epoch)
    }

    /// Re-parse the markdown widgets after navigation or reload.
    fn refresh_markdown(&mut self) {
        let slide = self.deck.current_slide();
        let step = self.deck.current_step();
        self.current_has_gif = slide.step_source(step).to_lowercase().contains(".gif");
        self.current_md = markdown::Content::parse(slide.step_source(step));
        self.current_cols = slide.column_slides(step).map(columns_of);
        self.next_md = self
            .deck
            .next_slide()
            .map(|s| markdown::Content::parse(&s.source));
        // The presenter "Next" panel previews what the next ▸ press shows:
        // the current slide's next reveal step if there is one, else the
        // next slide. Cache the next-step markdown and its column split here.
        let has_next_step = step + 1 < slide.step_count();
        self.next_preview_md =
            has_next_step.then(|| markdown::Content::parse(slide.step_source(step + 1)));
        self.next_cols = if has_next_step {
            slide.column_slides(step + 1)
        } else {
            self.deck
                .next_slide()
                .and_then(|s| s.column_slides(s.step_count().saturating_sub(1)))
        }
        .map(columns_of);
        #[cfg(feature = "video")]
        self.sync_embedded_video();
    }

    /// The current slide as a themed element: single column or two-column
    /// row. `inert` renders links non-interactive (audience window).
    /// The theme for a slide: the base theme, or its `[title]` /
    /// `[section]` overlay resolution for `<!-- slide: kind=... -->`.
    pub fn slide_theme(&self, slide: &preso_core::Slide) -> &preso_style::Theme {
        match slide.overrides.kind.as_deref() {
            Some("title") => &self.title_theme,
            Some("section") => &self.section_theme,
            _ => &self.theme,
        }
    }

    pub fn current_slide_element(&self, scale: f32, inert: bool) -> Element<'_, Message> {
        let slide = self.deck.current_slide();
        self.slide_body(
            slide,
            &self.current_md,
            self.current_cols.as_ref(),
            scale,
            self.deck.current_step(),
            inert,
        )
    }

    /// A slide's themed body: a single markdown column, or — when `cols` is a
    /// TwoColumn split — the two columns side by side under a shared header
    /// band. Shared by the audience/main view, the presenter "Next" preview,
    /// and the overview thumbnails so all three render two-column slides the
    /// same way. `inert` renders links non-interactive.
    fn slide_body<'a>(
        &'a self,
        slide: &'a preso_core::Slide,
        content: &'a markdown::Content,
        cols: Option<&'a (Column, Column)>,
        scale: f32,
        step: usize,
        inert: bool,
    ) -> Element<'a, Message> {
        let render_one = |content, code_slide| {
            let ctx = SlideContext {
                media: &self.media,
                code_slide,
                math_slide: slide,
                theme: self.slide_theme(slide),
                scale,
                animation_time: self.animation_time(),
                halign: render::resolve_halign(&slide.overrides, self.slide_theme(slide)),
                step,
            };
            if inert {
                render::slide_inert(content, ctx)
            } else {
                render::slide(content, ctx)
            }
        };
        match cols {
            Some((left, right)) => {
                // Align the columns' bodies under a shared header band so a
                // heading on one side doesn't leave the other's body
                // floating up beside it.
                let (lp, rp) = render::column_header_pads(
                    left.slide.leading_heading_level(),
                    right.slide.leading_heading_level(),
                    self.slide_theme(slide),
                    scale,
                );
                // Per-slide column ratio (`<!-- layout: TwoColumn 2:1 -->`).
                let (lw, rw) = slide.layout.column_portions().unwrap_or((1, 1));
                row![
                    iced::widget::container(render_one(&left.content, &left.slide))
                        .width(iced::FillPortion(lw))
                        .padding(iced::padding::top(lp)),
                    iced::widget::container(render_one(&right.content, &right.slide))
                        .width(iced::FillPortion(rw))
                        .padding(iced::padding::top(rp)),
                ]
                .spacing(40.0 * scale)
                .into()
            }
            None => render_one(content, slide),
        }
    }

    /// The overview grid (Esc), when open: clickable thumbnails of every
    /// slide, the current one outlined. The column count fills `width` (the
    /// presenter window width) at the thumbnail scale.
    pub fn overview_element(&self, width: f32) -> Option<Element<'_, Message>> {
        use iced::widget::{column, container, mouse_area, scrollable};

        if !self.overview {
            return None;
        }
        let contents = self.overview_md.as_ref()?;
        const SCALE: f32 = OVERVIEW_SCALE;
        let columns = overview_columns(width);
        let cell_height = render::DESIGN_HEIGHT * SCALE;
        let accent = render::color(self.theme.colors.accent);
        let muted = render::color(self.theme.colors.muted);

        let rows = contents
            .chunks(columns)
            .enumerate()
            .map(|(row_index, chunk)| {
                let cells = chunk.iter().enumerate().map(|(col_index, content)| {
                    let index = row_index * columns + col_index;
                    let slide = &self.deck.slides()[index];
                    let cols = self
                        .overview_cols
                        .as_ref()
                        .and_then(|all| all.get(index))
                        .and_then(Option::as_ref);
                    let is_current = index == self.deck.current_index();
                    let border_color = if is_current { accent } else { muted };
                    // Faithful miniature: same surface as the audience,
                    // including two-column layouts. Thumbnails show the slide
                    // fully revealed.
                    let body = self.slide_body(
                        slide,
                        content,
                        cols,
                        SCALE,
                        slide.step_count().saturating_sub(1),
                        true,
                    );
                    let surface = render::slide_surface(
                        body,
                        &self.media,
                        self.slide_theme(slide),
                        &slide.overrides,
                        render::SurfaceOptions {
                            scale: SCALE,
                            size: iced::Size::new(render::DESIGN_WIDTH * SCALE, cell_height),
                            number: Some((
                                self.deck.display_number(index),
                                self.deck.display_total(),
                            )),
                            footnote: slide.footnote.clone(),
                            layer_images: slide.layer_images.clone(),
                            video: slide.video.is_some(),
                            dither: true,
                        },
                    );
                    let thumb = container(surface).clip(true).style(move |_| {
                        iced::widget::container::Style {
                            border: iced::border::rounded(6)
                                .color(border_color)
                                .width(if is_current { 2.0 } else { 1.0 }),
                            ..iced::widget::container::Style::default()
                        }
                    });
                    mouse_area(
                        column![thumb, text(format!("{}", index + 1)).size(11).color(muted),]
                            .spacing(2),
                    )
                    .on_press(Message::OverviewJump(index))
                    .into()
                });
                row(cells).spacing(10).into()
            })
            .collect::<Vec<Element<'_, Message>>>();

        Some(
            scrollable(column(rows).spacing(10).padding(4))
                .id(OVERVIEW_SCROLL_ID)
                .width(Fill)
                .height(Fill)
                .into(),
        )
    }

    /// Scroll the (open) overview grid to the current slide, showing the
    /// slides before it. A no-op when the grid is closed.
    fn scroll_overview_to_current(&self) -> Task<Message> {
        let width = self
            .presenter_id()
            .map_or(render::DESIGN_WIDTH, |id| self.window_size(id).width);
        let columns = overview_columns(width);
        let y = overview_scroll_y(self.deck.current_index(), self.deck.len(), columns);
        iced::widget::operation::snap_to(
            OVERVIEW_SCROLL_ID,
            iced::widget::scrollable::RelativeOffset { x: 0.0, y },
        )
    }

    /// The presenter "Next" preview as a themed surface: what the next ▸
    /// press will show. While the current slide has more reveal steps,
    /// that's the current slide at its next step (so the presenter sees
    /// the bullet about to appear); once fully revealed, it's the next
    /// slide. `None` only at the very end of the deck.
    pub fn next_preview(&self, scale: f32) -> Option<Element<'_, Message>> {
        // Next reveal step of the current slide, if any.
        if let Some(content) = self.next_preview_md.as_ref() {
            let current = self.deck.current_slide();
            return Some(self.preview_surface(
                content,
                current,
                self.deck.display_number(self.deck.current_index()),
                scale,
                // The preview is the current slide at its *next* step.
                self.deck.current_step() + 1,
            ));
        }
        // Otherwise the next slide, fully revealed.
        let next = self.deck.next_slide()?;
        Some(self.preview_surface(
            self.next_md.as_ref()?,
            next,
            self.deck.display_number(self.deck.current_index() + 1),
            scale,
            next.step_count().saturating_sub(1),
        ))
    }

    /// A themed preview surface for `content` rendered against `slide`,
    /// stamped with slide number `number`. Shared by the two
    /// [`Self::next_preview`] cases; `next_cols` supplies the column split
    /// for a TwoColumn slide.
    fn preview_surface<'a>(
        &'a self,
        content: &'a markdown::Content,
        slide: &'a preso_core::Slide,
        number: usize,
        scale: f32,
        step: usize,
    ) -> Element<'a, Message> {
        let body = self.slide_body(slide, content, self.next_cols.as_ref(), scale, step, false);
        render::slide_surface(
            body,
            &self.media,
            self.slide_theme(slide),
            &slide.overrides,
            render::SurfaceOptions {
                scale,
                size: iced::Size::new(render::DESIGN_WIDTH * scale, render::DESIGN_HEIGHT * scale),
                number: Some((number, self.deck.display_total())),
                footnote: slide.footnote.clone(),
                layer_images: slide.layer_images.clone(),
                video: slide.video.is_some(),
                dither: true,
            },
        )
    }

    pub fn window_size(&self, id: window::Id) -> Size {
        self.sizes
            .get(&id)
            .copied()
            .unwrap_or(Size::new(960.0, 540.0))
    }

    pub fn window_scale_factor(&self, id: window::Id) -> f32 {
        self.scale_factors.get(&id).copied().unwrap_or(1.0)
    }

    fn audience_id(&self) -> Option<window::Id> {
        self.windows
            .iter()
            .find(|(_, r)| **r == Role::Audience)
            .map(|(id, _)| *id)
    }

    fn presenter_id(&self) -> Option<window::Id> {
        self.windows
            .iter()
            .find(|(_, r)| **r == Role::Presenter)
            .map(|(id, _)| *id)
    }

    pub fn title(&self, window: window::Id) -> String {
        let deck_title = self
            .deck
            .frontmatter
            .title
            .clone()
            .unwrap_or_else(|| self.path.display().to_string());
        match self.windows.get(&window) {
            Some(Role::Presenter) => format!(
                "{deck_title} — presenter ({}/{})",
                self.deck.current_index() + 1,
                self.deck.len()
            ),
            _ => deck_title,
        }
    }

    pub fn theme(&self, _window: window::Id) -> Option<Theme> {
        Some(self.iced_theme.clone())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Opened(role, id) => {
                self.windows.insert(id, role);
                if let Some((position, size)) = self.pending_opened.remove(&id) {
                    self.record_opened(id, position, size);
                }
                let fetch_scale =
                    window::scale_factor(id).map(move |sf| Message::ScaleFactor(id, sf));
                // Dual-display auto placement (plan §15).
                if self.auto_place {
                    match role {
                        Role::Audience => {
                            self.audience_fullscreen = true;
                            return Task::batch([
                                window::set_mode(id, window::Mode::Fullscreen),
                                fetch_scale,
                            ]);
                        }
                        Role::Presenter => {
                            return Task::batch([window::maximize(id, true), fetch_scale]);
                        }
                    }
                }
                // The Opened window event fires before our subscription is
                // polled, so fetch the initial geometry explicitly.
                Task::batch([
                    window::position(id).then(move |position| {
                        window::size(id)
                            .map(move |size| Message::InitialGeometry(id, position, size))
                    }),
                    fetch_scale,
                ])
            }
            Message::InitialGeometry(id, position, size) => {
                self.record_opened(id, position, size);
                Task::none()
            }
            Message::ScaleFactor(id, factor) => {
                self.scale_factors.insert(id, factor);
                Task::none()
            }
            Message::Window(id, event) => self.handle_window_event(id, event),
            Message::Key(event) => self.handle_key(event),
            Message::OverviewJump(index) => {
                self.overview = false;
                self.navigate(Nav::Goto(index))
            }
            Message::PointerMoved(position) => {
                self.pointer.moved(position);
                Task::none()
            }
            Message::PointerPressed => {
                self.pointer.pressed();
                Task::none()
            }
            Message::PointerReleased => {
                self.pointer.released();
                Task::none()
            }
            Message::LinkClicked(uri) => {
                tracing::info!(%uri, "opening link in system browser");
                if let Err(e) = open::that_detached(&uri) {
                    tracing::warn!(%uri, error = %e, "failed to open link");
                }
                Task::none()
            }
            Message::FileChanged => {
                self.reload();
                Task::none()
            }
            Message::Tick => {
                // End finished transitions so the fast ticker can stop, then
                // request a clean at-rest frame of the now-settled slide.
                if self.transition.is_some() && self.active_transition().is_none() {
                    self.transition = None;
                    self.pending_capture = true;
                }
                Task::none()
            }
            Message::CaptureTick => {
                // A frame with the new content has rendered: capture it now.
                self.pending_capture = false;
                self.capture_current_if_idle()
            }
            Message::CaptureFrame(index, step, handle) => {
                // Ignore a capture that arrived after we moved on (it would be
                // mislabelled); otherwise it's the current slide's clean frame.
                if (self.deck.current_index(), self.deck.current_step()) == (index, step) {
                    self.frame_cache = Some((index, step, handle));
                }
                Task::none()
            }
            Message::PollDisplays => self.poll_displays(),
            Message::Noop => Task::none(),
        }
    }

    /// React to a display being plugged in or unplugged after launch. We
    /// only act on the *transition* (secondary appears / disappears) so a
    /// presenter who has manually arranged the windows isn't overridden on
    /// every poll.
    fn poll_displays(&mut self) -> Task<Message> {
        let secondary = crate::monitors::detect()
            .and_then(|s| s.secondary)
            .is_some();
        if secondary == self.secondary_present {
            return Task::none();
        }
        self.secondary_present = secondary;
        // Re-detect to get fresh coordinates for placement.
        let Some(setup) = crate::monitors::detect() else {
            return Task::none();
        };
        if secondary {
            self.attach_secondary(setup)
        } else {
            self.recall_to_primary(setup)
        }
    }

    /// A second screen appeared: fullscreen the audience window on it and
    /// maximize the presenter on the primary — the same end state as a
    /// dual-display launch (plan §15).
    fn attach_secondary(&mut self, setup: crate::monitors::Setup) -> Task<Message> {
        let Some(secondary) = setup.secondary else {
            return Task::none();
        };
        tracing::info!("second screen connected; moving audience window to it");
        self.auto_place = true;
        let mut tasks = Vec::new();
        if let Some(id) = self.audience_id() {
            self.audience_fullscreen = true;
            // Move onto the secondary *before* fullscreening: Mode::Fullscreen
            // targets whichever monitor the window currently occupies.
            tasks.push(
                window::move_to(id, Point::new(secondary.x + 40.0, secondary.y + 40.0))
                    .chain(window::set_mode(id, window::Mode::Fullscreen)),
            );
        }
        if let Some(id) = self.presenter_id() {
            tasks.push(
                window::move_to(
                    id,
                    Point::new(setup.primary.x + 60.0, setup.primary.y + 60.0),
                )
                .chain(window::maximize(id, true)),
            );
        }
        Task::batch(tasks)
    }

    /// The second screen was unplugged: pull the audience window out of
    /// fullscreen and restore both windows to a windowed layout on the
    /// primary, preferring the geometry saved from the last single-display
    /// run.
    fn recall_to_primary(&mut self, setup: crate::monitors::Setup) -> Task<Message> {
        tracing::info!("second screen disconnected; recalling windows to the primary");
        self.auto_place = false;
        self.audience_fullscreen = false;
        let primary = setup.primary;
        let mut tasks = Vec::new();
        if let Some(id) = self.audience_id() {
            let (size, pos) = Self::restore_geometry(
                self.window_state.audience,
                primary,
                Size::new(960.0, 540.0),
                40.0,
            );
            tasks.push(
                window::set_mode(id, window::Mode::Windowed)
                    .chain(window::resize(id, size))
                    .chain(window::move_to(id, pos)),
            );
        }
        if let Some(id) = self.presenter_id() {
            let (size, pos) = Self::restore_geometry(
                self.window_state.presenter,
                primary,
                Size::new(1100.0, 760.0),
                60.0,
            );
            tasks.push(
                window::maximize(id, false)
                    .chain(window::resize(id, size))
                    .chain(window::move_to(id, pos)),
            );
        }
        Task::batch(tasks)
    }

    /// Size + position for recalling a window: the saved geometry when we
    /// have it, otherwise a default-sized window inset into the primary.
    fn restore_geometry(
        saved: Option<Geometry>,
        primary: crate::monitors::Monitor,
        default_size: Size,
        inset: f32,
    ) -> (Size, Point) {
        match saved {
            Some(g) => (Size::new(g.width, g.height), Point::new(g.x, g.y)),
            None => (
                default_size,
                Point::new(primary.x + inset, primary.y + inset),
            ),
        }
    }

    fn handle_window_event(&mut self, id: window::Id, event: window::Event) -> Task<Message> {
        match event {
            window::Event::Opened { position, size } => {
                if self.windows.contains_key(&id) {
                    self.record_opened(id, position, size);
                } else {
                    self.pending_opened.insert(id, (position, size));
                }
            }
            window::Event::Closed => {
                // Closing either window ends the presentation.
                window_state::save(&self.window_state);
                return iced::exit();
            }
            window::Event::Moved(position) => {
                self.update_geometry(id, |g| {
                    g.x = position.x;
                    g.y = position.y;
                });
            }
            window::Event::Resized(size) => {
                self.sizes.insert(id, size);
                self.update_geometry(id, |g| {
                    g.width = size.width;
                    g.height = size.height;
                });
            }
            window::Event::Rescaled(factor) => {
                self.scale_factors.insert(id, factor);
            }
            window::Event::FileDropped(path) => {
                self.open_file(path);
            }
            _ => {}
        }
        Task::none()
    }

    fn record_opened(&mut self, id: window::Id, position: Option<Point>, size: Size) {
        self.sizes.insert(id, size);
        self.update_geometry(id, |g| {
            if let Some(position) = position {
                g.x = position.x;
                g.y = position.y;
            }
            g.width = size.width;
            g.height = size.height;
        });
    }

    /// Track live geometry for persistence. Fullscreen geometry is skipped
    /// (restoring it would open a borderless window the size of the
    /// screen), and so are dual-display auto-placed sessions: their
    /// geometry would clobber the saved single-display layout.
    fn update_geometry(&mut self, id: window::Id, f: impl FnOnce(&mut Geometry)) {
        if self.auto_place {
            return;
        }
        let current = self.window_size(id);
        let Some(role) = self.windows.get(&id) else {
            return;
        };
        if *role == Role::Audience && self.audience_fullscreen {
            return;
        }
        let slot = match role {
            Role::Audience => &mut self.window_state.audience,
            Role::Presenter => &mut self.window_state.presenter,
        };
        f(slot.get_or_insert(Geometry {
            x: 0.0,
            y: 0.0,
            width: current.width,
            height: current.height,
        }));
        // Persist immediately: a force-quit or Cmd+Q delivers no Closed
        // event, and the file is tiny.
        window_state::save(&self.window_state);
    }

    /// Open a different deck (drag-and-drop). The hot-reload subscription
    /// re-keys on `self.path` and starts watching the new file by itself.
    fn open_file(&mut self, path: PathBuf) {
        let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
        let deck = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))
            .and_then(|source| crate::include::expand(&source, dir).map_err(|e| e.to_string()))
            .and_then(|source| {
                Deck::from_source(&source).map_err(|e| format!("{}: {e}", path.display()))
            });
        match deck {
            Ok(deck) => {
                tracing::info!(file = %path.display(), slides = deck.len(), "opened deck");
                self.deck = deck;
                self.media.set_deck_path(&path);
                self.path = path;
                self.error = None;
                self.jump_buffer.clear();
                self.overview_md = None;
                self.overview_cols = None;
                self.reload_theme();
                self.refresh_markdown();
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// The in-flight audience transition as (outgoing frame, kind, eased
    /// progress 0→1), or `None` if none is running (or it just finished).
    pub fn active_transition(
        &self,
    ) -> Option<(&iced::widget::image::Handle, crate::transition::Kind, f32)> {
        let t = self.transition.as_ref()?;
        let p = crate::transition::progress(t.started, self.clock.now(), t.kind.duration())?;
        Some((&t.outgoing, t.kind, p))
    }

    /// The current slide's cached frame, to keep its GPU texture warm while at
    /// rest. iced uploads a freshly-seen raster image on a worker thread, so it
    /// isn't drawable on its first frame; by rendering it (hidden) now, the
    /// upload is already done by the time it becomes the next transition's
    /// outgoing overlay — otherwise the incoming slide flashes through for one
    /// frame. `None` unless the cache matches the slide on screen.
    pub fn warm_frame(&self) -> Option<iced::widget::image::Handle> {
        let (i, s, handle) = self.frame_cache.as_ref()?;
        ((*i, *s) == (self.deck.current_index(), self.deck.current_step())).then(|| handle.clone())
    }

    /// Navigate the deck, animating a transition when one applies. The outgoing
    /// slide is supplied by `frame_cache` — a screenshot taken earlier while
    /// that slide was at rest — so no screenshot happens here on the critical
    /// path (which would stall the first animation frame and flash). If there's
    /// no clean cached frame for the slide we're leaving (very fast navigation,
    /// or a transition still running), we simply cut.
    fn navigate(&mut self, nav: Nav) -> Task<Message> {
        let before = (self.deck.current_index(), self.deck.current_step());
        self.apply_nav(nav);
        // Snap any in-flight transition; a new one starts only on a real slide
        // change with a matching cached outgoing frame.
        self.transition = None;
        if self.deck.current_index() != before.0 {
            let kind = self.current_transition_kind();
            if kind != crate::transition::Kind::None
                && let Some((idx, step, handle)) = &self.frame_cache
                && (*idx, *step) == before
            {
                self.transition = Some(ActiveTransition {
                    kind,
                    outgoing: handle.clone(),
                    started: self.clock.now(),
                });
            }
        }
        self.refresh_markdown();
        // Request an at-rest capture of the new slide for next time — but only
        // when we didn't just start a transition. Capturing now would be a
        // no-op (the slide is mid-transition) and the extra `window::frames`
        // subscription churns a redraw right at the transition's first frame.
        // When a transition is running, the end-of-transition tick requests the
        // capture instead.
        self.pending_capture = self.transition.is_none();
        self.overview_scroll_task()
    }

    /// Mutate the deck for a navigation (and clear pen strokes / start the
    /// talk timer as the old per-action handlers did).
    fn apply_nav(&mut self, nav: Nav) {
        match nav {
            Nav::Next => {
                self.timer.start_if_needed(self.clock.now());
                self.deck.next();
            }
            Nav::Prev => {
                self.timer.start_if_needed(self.clock.now());
                self.deck.prev();
            }
            Nav::First => self.deck.first(),
            Nav::Last => self.deck.last(),
            Nav::Goto(index) => self.deck.jump(index),
        }
        self.pointer.clear_strokes();
    }

    /// Screenshot the current slide for `frame_cache` when it's safe and useful:
    /// the deck transitions somewhere, no transition is running (so the frame is
    /// clean, and the readback's event-loop stall is invisible on static
    /// content), an audience window exists, and we don't already have this
    /// exact `(index, step)` cached.
    fn capture_current_if_idle(&self) -> Task<Message> {
        let key = (self.deck.current_index(), self.deck.current_step());
        if self.transition.is_some()
            || !self.deck_uses_transitions()
            || self.frame_cache.as_ref().map(|(i, s, _)| (*i, *s)) == Some(key)
        {
            return Task::none();
        }
        match self.audience_id() {
            Some(id) => window::screenshot(id).map(move |shot| {
                Message::CaptureFrame(
                    key.0,
                    key.1,
                    iced::widget::image::Handle::from_rgba(
                        shot.size.width,
                        shot.size.height,
                        shot.rgba,
                    ),
                )
            }),
            None => Task::none(),
        }
    }

    /// Keep the highlighted slide in view when the overview grid is open.
    fn overview_scroll_task(&self) -> Task<Message> {
        if self.overview {
            self.scroll_overview_to_current()
        } else {
            Task::none()
        }
    }

    /// The transition kind for the change *into* the current slide: its
    /// per-slide `transition=` override if set, else the deck default.
    fn current_transition_kind(&self) -> crate::transition::Kind {
        let raw = self
            .deck
            .current_slide()
            .overrides
            .transition
            .as_deref()
            .or(self.deck.frontmatter.transition.as_deref());
        crate::transition::Kind::from_frontmatter(raw)
    }

    /// Whether any slide change in this deck could animate — the deck default
    /// is a real transition, or some slide carries a non-`none` override. Gates
    /// the per-navigation frame capture.
    fn deck_uses_transitions(&self) -> bool {
        use crate::transition::Kind;
        if Kind::from_frontmatter(self.deck.frontmatter.transition.as_deref()) != Kind::None {
            return true;
        }
        self.deck.slides().iter().any(|s| {
            s.overrides
                .transition
                .as_deref()
                .is_some_and(|t| Kind::from_frontmatter(Some(t)) != Kind::None)
        })
    }

    /// Resolve a deck-relative resource path (video, etc.) against the deck
    /// file's directory.
    fn deck_relative(&self, rel: &str) -> std::path::PathBuf {
        match self.path.parent() {
            Some(dir) => dir.join(rel),
            None => std::path::PathBuf::from(rel),
        }
    }

    /// `V`: play the current slide's video. Inline (toggle play/pause on the
    /// embedded player) when built with the `video` feature and wgpu is live;
    /// otherwise launch an external player.
    fn play_current_video(&mut self) {
        let Some(rel) = self.deck.current_slide().video.clone() else {
            return;
        };
        // Inline playback when built with `video` and wgpu is live: load the
        // pipeline lazily on this first press (so navigating *past* video
        // slides never pays the GStreamer decode/preroll cost), then toggle
        // play/pause on subsequent presses.
        #[cfg(feature = "video")]
        if self.gpu_active {
            if self.embedded.as_ref().map(crate::video::Embedded::path) != Some(rel.as_str()) {
                let path = self.deck_relative(&rel);
                match crate::video::Embedded::load(rel.clone(), &path) {
                    Ok(embedded) => self.embedded = Some(embedded),
                    Err(e) => {
                        self.embedded = None;
                        self.error = Some(format!("cannot load video {}: {e}", path.display()));
                    }
                }
            }
            if let Some(embedded) = self.embedded.as_mut() {
                embedded.toggle_pause();
            }
            return;
        }
        let path = self.deck_relative(&rel);
        if let Err(e) = crate::video::play(&path) {
            self.error = Some(format!("cannot play {}: {e}", path.display()));
        }
    }

    /// The embedded player for the current slide, if one is loaded.
    #[cfg(feature = "video")]
    pub fn embedded_video(&self) -> Option<&iced_video_player::Video> {
        self.embedded.as_ref().map(crate::video::Embedded::video)
    }

    /// Drop the embedded player when it no longer matches the current slide
    /// (navigated away, or wgpu off), which also stops a playing clip. Loading
    /// is deferred to the first `V` press (see `play_current_video`) so moving
    /// past video slides stays instant. No-op unless the `video` feature is on.
    #[cfg(feature = "video")]
    fn sync_embedded_video(&mut self) {
        let current = if self.gpu_active {
            self.deck.current_slide().video.as_deref()
        } else {
            None
        };
        let stale = self.embedded.as_ref().map(crate::video::Embedded::path) != current;
        if stale {
            self.embedded = None;
        }
    }

    fn handle_key(&mut self, event: iced::keyboard::Event) -> Task<Message> {
        let Some(action) = keyboard::action(&event, &mut self.jump_buffer, self.overview) else {
            return Task::none();
        };
        tracing::debug!(?action, "keyboard action");
        match action {
            keyboard::Action::Next => self.navigate(Nav::Next),
            keyboard::Action::Prev => self.navigate(Nav::Prev),
            keyboard::Action::First => self.navigate(Nav::First),
            keyboard::Action::Last => self.navigate(Nav::Last),
            keyboard::Action::Jump(n) => self.navigate(Nav::Goto(n.saturating_sub(1))),
            keyboard::Action::ResetTimer => {
                self.timer.reset();
                Task::none()
            }
            keyboard::Action::ToggleLaser => {
                self.pointer.toggle(PointerMode::Laser);
                Task::none()
            }
            keyboard::Action::TogglePen => {
                self.pointer.toggle(PointerMode::Pen);
                Task::none()
            }
            keyboard::Action::ClearAnnotations => {
                self.pointer.clear_strokes();
                Task::none()
            }
            keyboard::Action::PlayVideo => {
                self.play_current_video();
                Task::none()
            }
            keyboard::Action::OverviewTop => iced::widget::operation::snap_to(
                OVERVIEW_SCROLL_ID,
                iced::widget::scrollable::RelativeOffset { x: 0.0, y: 0.0 },
            ),
            keyboard::Action::OverviewBottom => iced::widget::operation::snap_to(
                OVERVIEW_SCROLL_ID,
                iced::widget::scrollable::RelativeOffset { x: 0.0, y: 1.0 },
            ),
            keyboard::Action::OverviewPageUp => iced::widget::operation::scroll_by(
                OVERVIEW_SCROLL_ID,
                iced::widget::scrollable::AbsoluteOffset {
                    x: 0.0,
                    y: -OVERVIEW_PAGE,
                },
            ),
            keyboard::Action::OverviewPageDown => iced::widget::operation::scroll_by(
                OVERVIEW_SCROLL_ID,
                iced::widget::scrollable::AbsoluteOffset {
                    x: 0.0,
                    y: OVERVIEW_PAGE,
                },
            ),
            keyboard::Action::ToggleOverview => {
                self.overview = !self.overview;
                if self.overview && self.overview_md.is_none() {
                    self.overview_md = Some(
                        self.deck
                            .slides()
                            .iter()
                            .map(|s| markdown::Content::parse(&s.source))
                            .collect(),
                    );
                    // Two-column splits per slide (fully revealed), so the
                    // grid renders TwoColumn slides like the audience view.
                    self.overview_cols = Some(
                        self.deck
                            .slides()
                            .iter()
                            .map(|s| {
                                s.column_slides(s.step_count().saturating_sub(1))
                                    .map(columns_of)
                            })
                            .collect(),
                    );
                }
                // Opening: jump the grid to the current slide.
                if self.overview {
                    self.scroll_overview_to_current()
                } else {
                    Task::none()
                }
            }
            keyboard::Action::ToggleFullscreen => {
                if let Some(id) = self.audience_id() {
                    self.audience_fullscreen = !self.audience_fullscreen;
                    let mode = if self.audience_fullscreen {
                        window::Mode::Fullscreen
                    } else {
                        window::Mode::Windowed
                    };
                    return window::set_mode(id, mode);
                }
                Task::none()
            }
        }
    }

    fn reload(&mut self) {
        let source = match std::fs::read_to_string(&self.path) {
            Ok(source) => source,
            Err(e) => {
                tracing::warn!(error = %e, "reload read failure, keeping previous deck");
                self.error = Some(format!("cannot read {}: {e}", self.path.display()));
                return;
            }
        };
        // Re-expand `<!-- include: … -->` chapter files on every reload, so
        // editing a child file updates the deck too.
        let dir = self
            .path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let source = match crate::include::expand(&source, dir) {
            Ok(source) => source,
            Err(e) => {
                tracing::warn!(error = %e, "reload include failure, keeping previous deck");
                self.error = Some(e.to_string());
                return;
            }
        };
        match self.deck.reload(&source) {
            Ok(()) => {
                tracing::info!(slides = self.deck.len(), "deck reloaded");
                self.error = None;
                self.overview_md = None;
                self.overview_cols = None;
                self.reload_theme();
                self.refresh_markdown();
            }
            Err(e) => {
                // Keep presenting the old deck; surface the problem.
                tracing::warn!(error = %e, "reload parse failure, keeping previous deck");
                self.error = Some(e.to_string());
            }
        }
    }

    /// Re-resolve the theme on every reload and apply it if it changed. This
    /// re-reads the theme file each time, so editing the theme `.toml` (or the
    /// frontmatter `theme:`) updates the live deck — a `--theme` path is
    /// honoured the same way. Built-in themes resolve to the same value, so
    /// this is a cheap no-op for them.
    fn reload_theme(&mut self) {
        let name = self
            .theme_override
            .clone()
            .or_else(|| self.deck.frontmatter.theme.clone());
        let resolved = match &name {
            Some(name) => preso_style::load_with_search(name, &theme_search_dirs()),
            None => Ok(preso_style::Theme::default()),
        };
        match resolved {
            Ok(theme) if theme != self.theme => {
                tracing::info!(theme = %theme.name, "theme reloaded");
                self.set_theme(theme);
            }
            Ok(_) => {} // unchanged
            Err(e) => self.error = Some(format!("theme {:?}: {e}", name.unwrap_or_default())),
        }
    }

    /// Swap in a new theme and rebuild everything derived from it.
    fn set_theme(&mut self, theme: preso_style::Theme) {
        self.iced_theme = render::iced_theme(&theme);
        self.title_theme = theme.title.apply(&theme);
        self.section_theme = theme.section.apply(&theme);
        self.theme = theme;
        // Math is rendered in the theme's text colour, so drop the rendered-
        // media cache; it re-renders lazily against the new theme.
        self.media = Media::new(&self.path);
    }

    pub fn timer_display(&self) -> Option<TimerDisplay> {
        self.timer.display(self.clock.now())
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let deck_dir = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let mut subscriptions = vec![
            window::events().map(|(id, event)| Message::Window(id, event)),
            iced::keyboard::listen().map(Message::Key),
            hot_reload::subscription(&deck_dir),
            // Poll the display topology so a screen connected/disconnected
            // after launch re-runs auto placement. iced 0.14 exposes no
            // monitor-change event, so polling is the only option.
            every(std::time::Duration::from_millis(1500)).map(|()| Message::PollDisplays),
        ];
        // Watch the theme file's directory too (when it's elsewhere), so
        // editing the theme `.toml` hot-reloads the deck's styling.
        if let Some(theme_dir) = &self.theme.source_dir
            && *theme_dir != deck_dir
        {
            subscriptions.push(hot_reload::subscription(theme_dir));
        }
        if self.timer.running() {
            subscriptions.push(every(std::time::Duration::from_secs(1)).map(|()| Message::Tick));
        }
        // ~30fps heartbeat while a transition is animating or a GIF is showing.
        if self.transition.is_some() || self.current_has_gif {
            subscriptions.push(every(std::time::Duration::from_millis(33)).map(|()| Message::Tick));
        }
        // A capture is pending: tick on rendered frames so we screenshot the
        // new content once it's actually on screen (not the frame before it).
        if self.pending_capture {
            subscriptions.push(window::frames().map(|_| Message::CaptureTick));
        }
        Subscription::batch(subscriptions)
    }

    pub fn view(&self, window: window::Id) -> Element<'_, Message> {
        match self.windows.get(&window) {
            Some(Role::Audience) => audience::view(self, window),
            Some(Role::Presenter) => presenter::view(self, window),
            None => text("").into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal `Escape` key-press event for driving `Message::Key`.
    fn escape_event() -> iced::keyboard::Event {
        use iced::keyboard::key::{Code, Named, Physical};
        use iced::keyboard::{Event, Key, Location, Modifiers};
        Event::KeyPressed {
            key: Key::Named(Named::Escape),
            modified_key: Key::Named(Named::Escape),
            physical_key: Physical::Code(Code::Escape),
            location: Location::Standard,
            modifiers: Modifiers::default(),
            text: None,
            repeat: false,
        }
    }

    #[test]
    fn overview_scroll_offset_biases_to_before() {
        // 20 slides, 4 columns → 5 rows (0..=4).
        assert_eq!(overview_scroll_y(0, 20, 4), 0.0); // first row → top
        assert_eq!(overview_scroll_y(19, 20, 4), 1.0); // last row → bottom
        assert_eq!(overview_scroll_y(8, 20, 4), 0.5); // row 2 of 5 → middle
        // Wider grid → fewer rows; slide 8 is now in row 0 (10 per row) → top.
        assert_eq!(overview_scroll_y(8, 20, 10), 0.0);
        // One row (or empty): nothing to scroll.
        assert_eq!(overview_scroll_y(0, 3, 4), 0.0);
        assert_eq!(overview_scroll_y(0, 0, 4), 0.0);
    }

    #[test]
    fn overview_columns_fill_width() {
        // Thumbnail ≈ 240px. A 960px window fits ~3, a wide one many more.
        assert!(overview_columns(960.0) >= 3);
        assert!(overview_columns(2560.0) >= 8);
        assert_eq!(overview_columns(100.0), 1); // never zero
    }

    fn app(source: &str) -> App {
        let (app, _task) = App::new(
            PathBuf::from("test.md"),
            source.to_string(),
            preso_style::Theme::default(),
            None,
            false,
            Some(30),
            false,
        );
        app
    }

    /// Views are pure widget-tree builders; building them must not panic
    /// for any deck state, including empty and error states.
    #[test]
    fn views_build_for_example_talk() {
        let mut app = app(include_str!("../../../docs/example-talk.md"));
        for _ in 0..40 {
            let _ = presenter::view(&app, window::Id::unique());
            let _ = audience::view(&app, window::Id::unique());
            app.deck.next();
            app.refresh_markdown();
        }

        // Overview grid builds for the whole deck too.
        app.overview = true;
        app.overview_md = Some(
            app.deck
                .slides()
                .iter()
                .map(|s| markdown::Content::parse(&s.source))
                .collect(),
        );
        assert!(app.overview_element(1920.0).is_some());
        let _ = presenter::view(&app, window::Id::unique());
    }

    #[test]
    fn two_column_slides_split_in_preview_and_overview() {
        // Slide 1 single-column, slide 2 a TwoColumn slide.
        let src = "# One\n\n---\n\n<!-- layout: TwoColumn -->\n\nleft\n\n***\n\nright\n";
        let mut app = app(src);
        app.refresh_markdown();
        // On slide 1: the "Next" preview is slide 2 → its column split cached.
        assert!(
            app.next_cols.is_some(),
            "next preview should split the TwoColumn next slide"
        );
        // Opening the overview caches a split for the two-column slide only.
        let _ = app.update(Message::Key(escape_event()));
        let cols = app.overview_cols.expect("overview cols cached");
        assert!(cols[0].is_none(), "single-column slide has no split");
        assert!(cols[1].is_some(), "two-column slide is split");
    }

    #[test]
    fn views_build_for_empty_and_error_decks() {
        let empty = app("");
        let _ = presenter::view(&empty, window::Id::unique());
        let _ = audience::view(&empty, window::Id::unique());

        // Invalid frontmatter → error banner + empty fallback deck
        let broken = app("---\ntitle: [unclosed\n---\n# S\n");
        assert!(broken.error.is_some());
        let _ = presenter::view(&broken, window::Id::unique());
        let _ = audience::view(&broken, window::Id::unique());
    }

    #[test]
    fn pen_strokes_are_stored_in_design_space() {
        use crate::overlay::PointerMode;

        let mut app = app("# One\n---\n# Two\n");
        app.pointer.toggle(PointerMode::Pen);

        // Simulate a drag: views convert window-local to design space
        // before sending PointerMoved, so these are design units.
        let _ = app.update(Message::PointerMoved(Point::new(100.0, 200.0)));
        let _ = app.update(Message::PointerPressed);
        let _ = app.update(Message::PointerMoved(Point::new(300.0, 400.0)));
        let _ = app.update(Message::PointerReleased);

        assert_eq!(app.pointer.strokes.len(), 1);
        assert_eq!(
            app.pointer.strokes[0],
            vec![Point::new(100.0, 200.0), Point::new(300.0, 400.0)]
        );

        // Slide change clears annotations.
        let _ = app.update(Message::OverviewJump(1));
        assert!(app.pointer.strokes.is_empty());
    }

    #[test]
    fn gif_decodes_and_selects_frames_by_time() {
        use std::time::Duration;

        // Encode a 2-frame GIF (red, then green; 100ms each).
        let dir = std::env::temp_dir().join(format!("preso-gif-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("anim.gif");
        {
            let file = std::fs::File::create(&path).unwrap();
            let mut encoder = image::codecs::gif::GifEncoder::new(file);
            for color in [[255u8, 0, 0, 255], [0, 255, 0, 255]] {
                let buffer = image::RgbaImage::from_pixel(8, 8, image::Rgba(color));
                let frame = image::Frame::from_parts(
                    buffer,
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(100, 1),
                );
                encoder.encode_frames([frame]).unwrap();
            }
        }

        let deck = dir.join("deck.md");
        std::fs::write(&deck, "![anim](anim.gif)\n").unwrap();
        let app = {
            let (app, _task) = App::new(
                deck,
                "![anim](anim.gif)\n".to_string(),
                preso_style::Theme::default(),
                None,
                false,
                None,
                false,
            );
            app
        };
        assert!(app.current_has_gif);

        let gif = app.media.gif("anim.gif").expect("gif decodes");
        assert_eq!(gif.frames.len(), 2);
        // Frame selection loops: 0-100ms → frame 0, 100-200ms → frame 1.
        let f0 = gif.frame_at(Duration::from_millis(10));
        let f1 = gif.frame_at(Duration::from_millis(150));
        let f0_again = gif.frame_at(Duration::from_millis(210));
        assert!(f0 != f1);
        assert!(f0 == f0_again);
    }

    #[test]
    fn drag_and_drop_swaps_deck_and_path() {
        let dir = std::env::temp_dir().join(format!("preso-app-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dropped.md");
        std::fs::write(&path, "# Dropped\n---\n# Two\n").unwrap();

        let mut app = app("# Original\n");
        app.open_file(path.clone());
        assert_eq!(app.deck.len(), 2);
        assert!(app.error.is_none());

        // A bad drop keeps the current deck and surfaces the error.
        app.open_file(dir.join("missing.md"));
        assert_eq!(app.deck.len(), 2);
        assert!(app.error.is_some());
    }

    #[test]
    fn editing_the_theme_file_reloads_styling() {
        let dir = std::env::temp_dir().join(format!("preso-theme-reload-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let theme_path = dir.join("t.toml");
        let deck_path = dir.join("deck.md");
        let theme_toml = |bg: &str| {
            format!(
                "name = \"t\"\ncode_theme = \"base16-ocean.dark\"\n\
                 [colors]\nbackground = \"{bg}\"\ntext = \"#ffffff\"\nheading = \"#ffffff\"\n\
                 accent = \"#ffffff\"\nlink = \"#ffffff\"\nmuted = \"#888888\"\n\
                 code_background = \"#111111\"\n\
                 [fonts]\nbody_size = 36\nh1_size = 80\nh2_size = 56\nh3_size = 44\ncode_size = 30\n\
                 [spacing]\nslide_padding = 60\nparagraph_gap = 20\n"
            )
        };
        std::fs::write(&theme_path, theme_toml("#010101")).unwrap();
        std::fs::write(
            &deck_path,
            format!("---\ntheme: {}\n---\n\n# Hi\n", theme_path.display()),
        )
        .unwrap();

        let theme = preso_style::load_with_search(&theme_path.display().to_string(), &[]).unwrap();
        let (mut app, _task) = App::new(
            deck_path.clone(),
            std::fs::read_to_string(&deck_path).unwrap(),
            theme,
            None,
            false,
            None,
            false,
        );
        assert_eq!(
            app.theme.colors.background,
            preso_style::Color::rgb(1, 1, 1)
        );

        // Edit the theme file on disk; a reload picks up the new colour.
        std::fs::write(&theme_path, theme_toml("#0a0b0c")).unwrap();
        app.reload();
        assert_eq!(
            app.theme.colors.background,
            preso_style::Color::rgb(10, 11, 12)
        );
        assert!(app.error.is_none());
    }
}

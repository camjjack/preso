use serde::Deserialize;

/// An RGBA color, deserialized from `"#rgb"`, `"#rrggbb"`, or `"#rrggbbaa"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let hex = s.strip_prefix('#')?;
        let from = |s: &str| u8::from_str_radix(s, 16).ok();
        match hex.len() {
            3 => {
                let d = |i: usize| from(&hex[i..=i]).map(|v| v * 17);
                Some(Self {
                    r: d(0)?,
                    g: d(1)?,
                    b: d(2)?,
                    a: 255,
                })
            }
            6 | 8 => Some(Self {
                r: from(&hex[0..2])?,
                g: from(&hex[2..4])?,
                b: from(&hex[4..6])?,
                a: if hex.len() == 8 {
                    from(&hex[6..8])?
                } else {
                    255
                },
            }),
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Color::parse(&s).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "invalid color {s:?}: expected #rgb, #rrggbb, or #rrggbbaa"
            ))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    pub background: Color,
    pub text: Color,
    pub heading: Color,
    pub accent: Color,
    pub link: Color,
    pub muted: Color,
    pub code_background: Color,
}

/// Font sizes in design units (1920×1080 canvas), plus optional families.
/// Family names must be loaded fonts (bundled, system, or `files`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fonts {
    pub body_size: f32,
    pub h1_size: f32,
    pub h2_size: f32,
    pub h3_size: f32,
    pub code_size: f32,
    #[serde(default)]
    pub body_family: Option<String>,
    #[serde(default)]
    pub heading_family: Option<String>,
    #[serde(default)]
    pub code_family: Option<String>,
    /// Font files to load, relative to the theme file.
    #[serde(default)]
    pub files: Vec<String>,
}

/// `[slide]` — full-slide visual treatment behind the content.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlideStyle {
    /// Full-bleed background image (path, theme-relative; resolved on
    /// load). Cover-fit over the whole canvas; wins over `gradient` and
    /// `colors.background`. Content renders on top.
    #[serde(default)]
    pub background_image: Option<String>,
    /// Linear gradient background (wins over `colors.background`).
    #[serde(default)]
    pub gradient: Option<Gradient>,
    /// A solid accent band along one edge.
    #[serde(default)]
    pub bar: Option<AccentBar>,
    /// Additional accent bars (e.g. two for a title slide). Rendered after
    /// `bar`. A `[title]`/`[section]` overlay that sets `bars` replaces the
    /// inherited single `bar`.
    #[serde(default)]
    pub bars: Vec<AccentBar>,
    /// Watermark logo drawn on every slide.
    #[serde(default)]
    pub logo: Option<Logo>,
    /// Vertical alignment of slide content.
    #[serde(default)]
    pub align: VerticalAlign,
    /// Horizontal alignment of slide content (headings, body, bullets).
    #[serde(default)]
    pub halign: HorizontalAlign,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gradient {
    pub from: Color,
    pub to: Color,
    /// Degrees, clockwise; 0 points up (`to` at the top), 180 down.
    #[serde(default = "default_gradient_angle")]
    pub angle: f32,
}

fn default_gradient_angle() -> f32 {
    180.0
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccentBar {
    #[serde(default)]
    pub side: Side,
    /// Thickness in design units.
    #[serde(default = "default_bar_size")]
    pub size: f32,
    #[serde(default)]
    pub color: Color,
    /// `bar = { hidden = true }` in a `[title]`/`[section]` overlay
    /// removes the base theme's bar for that slide kind.
    #[serde(default)]
    pub hidden: bool,
    /// When `true`, slide content is kept clear of the bar (its edge is
    /// padded by the bar's thickness). Background chrome — the logo and
    /// slide number — still draws over the bar. Defaults to `false`, so
    /// content runs under the bar as before.
    #[serde(default)]
    pub reserve: bool,
}

fn default_bar_size() -> f32 {
    12.0
}

impl Default for Color {
    fn default() -> Self {
        Self::rgb(0, 0, 0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Top,
    Bottom,
    #[default]
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Logo {
    /// Image path, relative to the theme file (resolved on load).
    #[serde(default)]
    pub path: String,
    /// Width as a percentage of the slide width.
    #[serde(default = "default_logo_width")]
    pub width: f32,
    #[serde(default)]
    pub position: Corner,
    /// 0.0–1.0.
    #[serde(default = "default_logo_opacity")]
    pub opacity: f32,
    /// `logo = { hidden = true }` in a `[title]`/`[section]` overlay
    /// removes the base theme's logo for that slide kind.
    #[serde(default)]
    pub hidden: bool,
    /// Optional frame, like content images.
    #[serde(default)]
    pub border: Option<ImageBorder>,
    /// Optional drop shadow: `true` or `{ color, offset, blur }`.
    #[serde(default)]
    pub shadow: Option<ShadowSetting>,
    /// Inset from the anchored corner, design units (so it scales like the
    /// bars, keeping the position window-size independent). `padding_x`
    /// pushes in from the left/right edge, `padding_y` from the top/bottom.
    #[serde(default = "default_chrome_padding")]
    pub padding_x: f32,
    #[serde(default = "default_chrome_padding")]
    pub padding_y: f32,
}

fn default_logo_width() -> f32 {
    10.0
}

/// Default inset for corner chrome (logo, slide number), design units.
fn default_chrome_padding() -> f32 {
    24.0
}

/// The uniform-`padding`-plus-per-side-overrides pattern shared by
/// `[code_block]`, `[table]`, and `[quote]`. The five fields are declared
/// in each struct rather than `#[serde(flatten)]`ed from a shared one
/// because serde's `deny_unknown_fields` — which keeps theme typos loud
/// (§17.4) — does not support `flatten`. This macro dedupes the resolution.
macro_rules! impl_padding_sides {
    ($($ty:ty),+ $(,)?) => {$(
        impl $ty {
            /// Per-side padding `[top, right, bottom, left]` in design
            /// units: each side uses its override, else the uniform
            /// `padding`, else `default`.
            pub fn padding_sides(&self, default: f32) -> [f32; 4] {
                let base = self.padding.unwrap_or(default);
                [
                    self.padding_top.unwrap_or(base),
                    self.padding_right.unwrap_or(base),
                    self.padding_bottom.unwrap_or(base),
                    self.padding_left.unwrap_or(base),
                ]
            }
        }
    )+};
}
impl_padding_sides!(CodeBlockStyle, TableStyle, QuoteStyle);

fn default_logo_opacity() -> f32 {
    1.0
}

/// `[image]` — default framing for content images. Per-image markdown
/// flags override: `{border}` / `{shadow}` force one on, `{plain}`
/// strips both.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageStyle {
    #[serde(default)]
    pub border: Option<ImageBorder>,
    #[serde(default)]
    pub shadow: Option<ShadowSetting>,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageBorder {
    /// The theme's muted color when omitted.
    #[serde(default)]
    pub color: Option<Color>,
    /// Design units.
    #[serde(default = "default_border_width")]
    pub width: f32,
    /// Corner radius in design units; rounds the image itself.
    #[serde(default = "default_border_radius")]
    pub radius: f32,
}

impl Default for ImageBorder {
    fn default() -> Self {
        Self {
            color: None,
            width: default_border_width(),
            radius: default_border_radius(),
        }
    }
}

fn default_border_width() -> f32 {
    3.0
}

fn default_border_radius() -> f32 {
    8.0
}

/// `shadow = true` (defaults) or a full `{ color, offset, blur }` table.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum ShadowSetting {
    Enabled(bool),
    Custom(ImageShadow),
}

impl ShadowSetting {
    pub fn resolve(&self) -> Option<ImageShadow> {
        match self {
            Self::Enabled(true) => Some(ImageShadow::default()),
            Self::Enabled(false) => None,
            Self::Custom(shadow) => Some(*shadow),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageShadow {
    #[serde(default = "default_shadow_color")]
    pub color: Color,
    /// `[x, y]` in design units.
    #[serde(default = "default_shadow_offset")]
    pub offset: [f32; 2],
    #[serde(default = "default_shadow_blur")]
    pub blur: f32,
}

impl Default for ImageShadow {
    fn default() -> Self {
        Self {
            color: default_shadow_color(),
            offset: default_shadow_offset(),
            blur: default_shadow_blur(),
        }
    }
}

fn default_shadow_color() -> Color {
    Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0x99,
    }
}

fn default_shadow_offset() -> [f32; 2] {
    [0.0, 8.0]
}

fn default_shadow_blur() -> f32 {
    24.0
}

/// `[slide_number]` — the current slide number stamped on every slide.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlideNumber {
    /// Template; `{current}` and `{total}` expand. E.g. `"{current} / {total}"`.
    #[serde(default = "default_number_format")]
    pub format: String,
    /// Text size in design units.
    #[serde(default = "default_number_size")]
    pub size: f32,
    /// Font family name; the theme's body font when omitted.
    #[serde(default)]
    pub font: Option<String>,
    #[serde(default)]
    pub position: Corner,
    /// The theme's muted color when omitted.
    #[serde(default)]
    pub color: Option<Color>,
    /// `[title.slide_number] hidden = true` removes the base theme's
    /// slide number for that slide kind.
    #[serde(default)]
    pub hidden: bool,
    /// Inset from the anchored corner, design units (see [`Logo::padding_x`]).
    #[serde(default = "default_chrome_padding")]
    pub padding_x: f32,
    #[serde(default = "default_chrome_padding")]
    pub padding_y: f32,
}

fn default_number_format() -> String {
    "{current}".to_owned()
}

fn default_number_size() -> f32 {
    24.0
}

/// `[footnote]` — a small disclaimer line along the bottom of each slide,
/// carrying the text of a `<!-- footnote: … -->` directive (e.g. image
/// credits). Unlike `[slide_number]` this always has defaults: the directive,
/// not the theme section, decides whether a slide shows one.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Footnote {
    /// Text size in design units.
    #[serde(default = "default_footnote_size")]
    pub size: f32,
    /// Font family name; the theme's body font when omitted.
    #[serde(default)]
    pub font: Option<String>,
    /// The theme's muted color when omitted.
    #[serde(default)]
    pub color: Option<Color>,
    /// Horizontal placement of the line along the bottom edge.
    #[serde(default)]
    pub align: HorizontalAlign,
    /// `[title.footnote] hidden = true` drops the footnote for that kind.
    #[serde(default)]
    pub hidden: bool,
    /// Inset from the side edges, design units (so it lines up with content).
    #[serde(default = "default_footnote_padding_x")]
    pub padding_x: f32,
    /// Inset from the bottom edge, design units.
    #[serde(default = "default_footnote_padding_y")]
    pub padding_y: f32,
}

impl Default for Footnote {
    fn default() -> Self {
        Self {
            size: default_footnote_size(),
            font: None,
            color: None,
            align: HorizontalAlign::default(),
            hidden: false,
            padding_x: default_footnote_padding_x(),
            padding_y: default_footnote_padding_y(),
        }
    }
}

fn default_footnote_size() -> f32 {
    18.0
}

fn default_footnote_padding_x() -> f32 {
    // Matches the default `slide_padding` so the line aligns with content.
    60.0
}

fn default_footnote_padding_y() -> f32 {
    36.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerticalAlign {
    #[default]
    Top,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HorizontalAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Spacing in design units.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spacing {
    pub slide_padding: f32,
    pub paragraph_gap: f32,
}

/// `[heading]` — per-level heading color overrides (v3b).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadingStyle {
    /// All levels, unless a per-level override applies.
    #[serde(default)]
    pub color: Option<Color>,
    #[serde(default)]
    pub h1_color: Option<Color>,
    #[serde(default)]
    pub h2_color: Option<Color>,
    #[serde(default)]
    pub h3_color: Option<Color>,
}

/// How `{n,m-k}` line selections are emphasised in a code block.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HighlightStyle {
    /// Paint a background tint behind the selected lines (default).
    #[default]
    Background,
    /// Leave the selected lines at full strength and fade everything
    /// else — "focus mode".
    Dim,
}

/// `[code_block]` — code block container styling (v3b).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeBlockStyle {
    #[serde(default)]
    pub background: Option<Color>,
    #[serde(default)]
    pub border_radius: Option<f32>,
    /// Design units.
    #[serde(default)]
    pub padding: Option<f32>,
    /// Per-side overrides for `padding` (design units); each falls back to
    /// `padding`, then the renderer's default.
    #[serde(default)]
    pub padding_top: Option<f32>,
    #[serde(default)]
    pub padding_right: Option<f32>,
    #[serde(default)]
    pub padding_bottom: Option<f32>,
    #[serde(default)]
    pub padding_left: Option<f32>,
    /// Background tint behind `{n,m-k}` highlighted lines (used when
    /// [`highlight_style`](Self::highlight_style) is `background`).
    #[serde(default)]
    pub highlight_color: Option<Color>,
    /// Whether `{n,m-k}` selections are shown by tinting the selected
    /// lines (`background`, default) or dimming the rest (`dim`).
    #[serde(default)]
    pub highlight_style: Option<HighlightStyle>,
    /// Opacity (0.0–1.0) applied to non-selected lines in `dim` mode.
    /// Defaults to 0.35.
    #[serde(default)]
    pub dim_opacity: Option<f32>,
}

/// `[table]` — markdown table styling. Unset fields fall back to palette
/// defaults (see `render`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableStyle {
    /// Background behind the header row.
    #[serde(default)]
    pub header_background: Option<Color>,
    /// Text colour of the header row.
    #[serde(default)]
    pub header_color: Option<Color>,
    /// Background of alternate (odd) body rows — zebra striping.
    #[serde(default)]
    pub stripe_background: Option<Color>,
    /// Colour of the cell separator lines.
    #[serde(default)]
    pub border_color: Option<Color>,
    /// Separator line thickness, design units.
    #[serde(default)]
    pub border_width: Option<f32>,
    /// Cell padding, design units.
    #[serde(default)]
    pub padding: Option<f32>,
    /// Per-side overrides for `padding` (design units); each falls back to
    /// `padding`, then the default.
    #[serde(default)]
    pub padding_top: Option<f32>,
    #[serde(default)]
    pub padding_right: Option<f32>,
    #[serde(default)]
    pub padding_bottom: Option<f32>,
    #[serde(default)]
    pub padding_left: Option<f32>,
    /// Outer corner radius, design units.
    #[serde(default)]
    pub border_radius: Option<f32>,
}

/// `[quote]` — blockquote (`> …`) callout styling. Unset fields fall back to
/// palette defaults (see `render`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuoteStyle {
    /// Fill behind the quote.
    #[serde(default)]
    pub background: Option<Color>,
    /// Accent bar along the leading edge; defaults to `colors.accent`.
    #[serde(default)]
    pub border_color: Option<Color>,
    /// Accent bar thickness, design units (default 4).
    #[serde(default)]
    pub border_width: Option<f32>,
    /// Inner padding, design units.
    #[serde(default)]
    pub padding: Option<f32>,
    /// Per-side overrides for `padding` (design units); each falls back to
    /// `padding`, then the default.
    #[serde(default)]
    pub padding_top: Option<f32>,
    #[serde(default)]
    pub padding_right: Option<f32>,
    #[serde(default)]
    pub padding_bottom: Option<f32>,
    #[serde(default)]
    pub padding_left: Option<f32>,
    /// Corner radius, design units.
    #[serde(default)]
    pub border_radius: Option<f32>,
    /// Render the quote text italic (default `false`).
    #[serde(default)]
    pub italic: Option<bool>,
    /// Horizontal placement of the callout (`left` default, `center`, `right`).
    #[serde(default)]
    pub align: Option<HorizontalAlign>,
}

/// `[title]` / `[section]` — a partial theme deep-merged onto the base
/// for slides of that kind (`<!-- slide: kind=title -->`). Anything not
/// set falls back to the base theme.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeOverlay {
    #[serde(default)]
    pub code_theme: Option<String>,
    #[serde(default)]
    pub colors: PaletteOverlay,
    #[serde(default)]
    pub fonts: FontsOverlay,
    #[serde(default)]
    pub spacing: SpacingOverlay,
    #[serde(default)]
    pub slide: SlideStyleOverlay,
    /// Field-level merge: set fields win, unset inherit.
    #[serde(default)]
    pub heading: HeadingStyle,
    /// Field-level merge: set fields win, unset inherit.
    #[serde(default)]
    pub code_block: CodeBlockStyle,
    /// Field-level merge: set fields win, unset inherit.
    #[serde(default)]
    pub table: TableStyle,
    /// Field-level merge: set fields win, unset inherit.
    #[serde(default)]
    pub quote: QuoteStyle,
    /// Field-level merge: set fields win, unset inherit.
    #[serde(default)]
    pub image: ImageStyle,
    /// Whole-section replace; `hidden = true` removes the base's number.
    #[serde(default)]
    pub slide_number: Option<SlideNumber>,
    /// Whole-section replace; `hidden = true` removes the footnote for the kind.
    #[serde(default)]
    pub footnote: Option<Footnote>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaletteOverlay {
    #[serde(default)]
    pub background: Option<Color>,
    #[serde(default)]
    pub text: Option<Color>,
    #[serde(default)]
    pub heading: Option<Color>,
    #[serde(default)]
    pub accent: Option<Color>,
    #[serde(default)]
    pub link: Option<Color>,
    #[serde(default)]
    pub muted: Option<Color>,
    #[serde(default)]
    pub code_background: Option<Color>,
}

/// No `files` here: font files load once at startup from the base theme.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontsOverlay {
    #[serde(default)]
    pub body_size: Option<f32>,
    #[serde(default)]
    pub h1_size: Option<f32>,
    #[serde(default)]
    pub h2_size: Option<f32>,
    #[serde(default)]
    pub h3_size: Option<f32>,
    #[serde(default)]
    pub code_size: Option<f32>,
    #[serde(default)]
    pub body_family: Option<String>,
    #[serde(default)]
    pub heading_family: Option<String>,
    #[serde(default)]
    pub code_family: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpacingOverlay {
    #[serde(default)]
    pub slide_padding: Option<f32>,
    #[serde(default)]
    pub paragraph_gap: Option<f32>,
}

/// `gradient`/`bar`/`logo` replace the base's whole value when set
/// (use `{ hidden = true }` on bar/logo to remove the base's).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlideStyleOverlay {
    #[serde(default)]
    pub background_image: Option<String>,
    #[serde(default)]
    pub gradient: Option<Gradient>,
    #[serde(default)]
    pub bar: Option<AccentBar>,
    #[serde(default)]
    pub bars: Vec<AccentBar>,
    #[serde(default)]
    pub logo: Option<Logo>,
    #[serde(default)]
    pub align: Option<VerticalAlign>,
    #[serde(default)]
    pub halign: Option<HorizontalAlign>,
}

/// Field-level merge for [`ThemeOverlay::apply`]: a set overlay field wins,
/// an unset one inherits the base. Three shapes:
/// `copy:` writes an `Option<T>` overlay onto a plain `T` base (Copy types);
/// `opt:` merges `Option` onto `Option` (Copy types);
/// `clone:` merges `Option` onto `Option` for clone-only types.
macro_rules! merge {
    (copy: $src:expr => $dst:expr; $($field:ident),+ $(,)?) => {
        $($dst.$field = $src.$field.unwrap_or($dst.$field);)+
    };
    (opt: $src:expr => $dst:expr; $($field:ident),+ $(,)?) => {
        $($dst.$field = $src.$field.or($dst.$field);)+
    };
    (clone: $src:expr => $dst:expr; $($field:ident),+ $(,)?) => {
        $($dst.$field = $src.$field.clone().or($dst.$field.take());)+
    };
}

impl ThemeOverlay {
    /// Resolve this overlay against a base theme: set values win,
    /// everything else inherits.
    pub fn apply(&self, base: &Theme) -> Theme {
        let mut theme = base.clone();
        if let Some(v) = &self.code_theme {
            theme.code_theme = v.clone();
        }

        merge!(copy: self.colors => theme.colors;
            background, text, heading, accent, link, muted, code_background);

        merge!(copy: self.fonts => theme.fonts;
            body_size, h1_size, h2_size, h3_size, code_size);
        merge!(clone: self.fonts => theme.fonts;
            body_family, heading_family, code_family);

        merge!(copy: self.spacing => theme.spacing; slide_padding, paragraph_gap);

        merge!(clone: self.slide => theme.slide; background_image, logo);
        merge!(opt: self.slide => theme.slide; gradient);
        merge!(copy: self.slide => theme.slide; align, halign);
        // An overlay that sets `bars` fully defines this kind's bars,
        // superseding the inherited single `bar`; otherwise inherit/merge.
        if self.slide.bars.is_empty() {
            theme.slide.bar = self.slide.bar.or(theme.slide.bar);
        } else {
            theme.slide.bar = self.slide.bar;
            theme.slide.bars = self.slide.bars.clone();
        }

        merge!(opt: self.heading => theme.heading; color, h1_color, h2_color, h3_color);
        merge!(opt: self.code_block => theme.code_block;
            background, border_radius, padding, padding_top, padding_right,
            padding_bottom, padding_left, highlight_color, highlight_style, dim_opacity);
        merge!(opt: self.table => theme.table;
            header_background, header_color, stripe_background, border_color,
            border_width, padding, padding_top, padding_right, padding_bottom,
            padding_left, border_radius);
        merge!(opt: self.quote => theme.quote;
            background, border_color, border_width, padding, padding_top,
            padding_right, padding_bottom, padding_left, border_radius, italic, align);
        merge!(opt: self.image => theme.image; border, shadow);

        // Whole-section replacements (`hidden = true` inside removes them).
        if let Some(n) = &self.slide_number {
            theme.slide_number = Some(n.clone());
        }
        if let Some(f) = &self.footnote {
            theme.footnote = f.clone();
        }

        theme
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub name: String,
    /// Which syntect/highlighter theme code blocks use.
    pub code_theme: String,
    pub colors: Palette,
    pub fonts: Fonts,
    pub spacing: Spacing,
    #[serde(default)]
    pub slide: SlideStyle,
    #[serde(default)]
    pub heading: HeadingStyle,
    #[serde(default)]
    pub code_block: CodeBlockStyle,
    /// `[table]` — markdown table styling.
    #[serde(default)]
    pub table: TableStyle,
    /// `[quote]` — blockquote callout styling.
    #[serde(default)]
    pub quote: QuoteStyle,
    /// `[image]` — default framing for content images.
    #[serde(default)]
    pub image: ImageStyle,
    /// `[slide_number]` — when present, every slide shows its number.
    #[serde(default)]
    pub slide_number: Option<SlideNumber>,
    /// `[footnote]` — styling for `<!-- footnote: … -->` disclaimer lines.
    #[serde(default)]
    pub footnote: Footnote,
    /// `[title]` — overlay for `kind=title` slides.
    #[serde(default)]
    pub title: ThemeOverlay,
    /// `[section]` — overlay for `kind=section` slides.
    #[serde(default)]
    pub section: ThemeOverlay,
    /// Directory of the theme file (for `files`/`logo` resolution);
    /// `None` for built-ins.
    #[serde(skip)]
    pub source_dir: Option<std::path::PathBuf>,
}

impl Theme {
    pub fn from_toml(source: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_colors() {
        assert_eq!(Color::parse("#fff"), Some(Color::rgb(255, 255, 255)));
        assert_eq!(Color::parse("#1e1e2e"), Some(Color::rgb(30, 30, 46)));
        assert_eq!(
            Color::parse("#11223344"),
            Some(Color {
                r: 0x11,
                g: 0x22,
                b: 0x33,
                a: 0x44
            })
        );
        assert_eq!(Color::parse("red"), None);
        assert_eq!(Color::parse("#12345"), None);
    }

    #[test]
    fn slide_style_and_font_families_parse() {
        let toml = r##"
name = "corporate"
code_theme = "SolarizedDark"

[colors]
background = "#0f2027"
text = "#e6edf3"
heading = "#7dd3fc"
accent = "#f5c2e7"
link = "#7dd3fc"
muted = "#8b949e"
code_background = "#0b1416"

[fonts]
body_size = 36
h1_size = 80
h2_size = 56
h3_size = 44
code_size = 30
heading_family = "Georgia"
files = ["Custom-Regular.ttf"]

[spacing]
slide_padding = 60
paragraph_gap = 20

[slide]
align = "center"
halign = "right"
gradient = { from = "#0f2027", to = "#2c5364", angle = 135 }
bar = { side = "bottom", size = 16, color = "#f5c2e7" }
logo = { path = "logo.png", width = 12, position = "bottom-right", opacity = 0.4 }

[slide_number]
format = "{current} / {total}"
size = 20
font = "JetBrains Mono"
position = "bottom-left"
color = "#8b949e"
"##;
        let theme = Theme::from_toml(toml).unwrap();
        let slide = &theme.slide;
        assert_eq!(slide.align, VerticalAlign::Center);
        assert_eq!(slide.halign, HorizontalAlign::Right);
        let gradient = slide.gradient.unwrap();
        assert_eq!(gradient.angle, 135.0);
        assert_eq!(slide.bar.unwrap().side, Side::Bottom);
        let logo = slide.logo.as_ref().unwrap();
        assert_eq!(logo.position, Corner::BottomRight);
        assert_eq!(logo.opacity, 0.4);
        assert_eq!(theme.fonts.heading_family.as_deref(), Some("Georgia"));
        assert_eq!(theme.fonts.files, vec!["Custom-Regular.ttf"]);
        let number = theme.slide_number.as_ref().unwrap();
        assert_eq!(number.format, "{current} / {total}");
        assert_eq!(number.size, 20.0);
        assert_eq!(number.font.as_deref(), Some("JetBrains Mono"));
        assert_eq!(number.position, Corner::BottomLeft);
        assert_eq!(number.color, Some(Color::rgb(0x8b, 0x94, 0x9e)));
    }

    #[test]
    fn image_framing_parses() {
        let toml = r##"
name = "framed"
code_theme = "base16-ocean.dark"

[colors]
background = "#000000"
text = "#ffffff"
heading = "#ffffff"
accent = "#ffffff"
link = "#ffffff"
muted = "#888888"
code_background = "#111111"

[fonts]
body_size = 36
h1_size = 80
h2_size = 56
h3_size = 44
code_size = 30

[spacing]
slide_padding = 60
paragraph_gap = 20

[image]
border = { color = "#ffffff", width = 4, radius = 12 }
shadow = true

[slide]
logo = { path = "logo.png", shadow = { blur = 40 }, border = {} }
"##;
        let theme = Theme::from_toml(toml).unwrap();
        let border = theme.image.border.unwrap();
        assert_eq!(border.color, Some(Color::rgb(255, 255, 255)));
        assert_eq!(border.width, 4.0);
        assert_eq!(border.radius, 12.0);
        // `shadow = true` resolves to the default shadow.
        let shadow = theme.image.shadow.unwrap().resolve().unwrap();
        assert_eq!(shadow, ImageShadow::default());
        // `shadow = false` resolves to none.
        assert_eq!(ShadowSetting::Enabled(false).resolve(), None);

        let logo = theme.slide.logo.as_ref().unwrap();
        assert_eq!(logo.shadow.unwrap().resolve().unwrap().blur, 40.0);
        assert_eq!(logo.border.unwrap(), ImageBorder::default());
    }

    #[test]
    fn kind_overlays_merge_onto_base() {
        let toml = r##"
name = "kinds"
code_theme = "base16-ocean.dark"

[colors]
background = "#000000"
text = "#ffffff"
heading = "#aaaaaa"
accent = "#ff00ff"
link = "#0000ff"
muted = "#888888"
code_background = "#111111"

[fonts]
body_size = 36
h1_size = 80
h2_size = 56
h3_size = 44
code_size = 30

[spacing]
slide_padding = 60
paragraph_gap = 20

[slide]
background_image = "deck-bg.png"
bar = { side = "right", size = 24, color = "#ff00ff" }
logo = { path = "logo.png" }

[slide_number]
format = "{current}"

[title.colors]
heading = "#ff0000"

[title.fonts]
h1_size = 120

[title.slide]
align = "center"
background_image = "title-bg.png"
logo = { hidden = true }

[title.slide_number]
hidden = true

[section.heading]
h2_color = "#00ff00"
"##;
        let theme = Theme::from_toml(toml).unwrap();

        let title = theme.title.apply(&theme);
        assert_eq!(title.colors.heading, Color::rgb(255, 0, 0));
        assert_eq!(title.colors.text, theme.colors.text); // inherited
        assert_eq!(title.fonts.h1_size, 120.0);
        assert_eq!(title.fonts.body_size, theme.fonts.body_size); // inherited
        assert_eq!(title.slide.align, VerticalAlign::Center);
        assert!(title.slide.logo.as_ref().unwrap().hidden);
        assert_eq!(title.slide.bar, theme.slide.bar); // inherited
        assert_eq!(
            title.slide.background_image.as_deref(),
            Some("title-bg.png") // overlay wins
        );
        assert!(title.slide_number.as_ref().unwrap().hidden);

        let section = theme.section.apply(&theme);
        assert_eq!(section.heading.h2_color, Some(Color::rgb(0, 255, 0)));
        assert_eq!(section.colors.heading, theme.colors.heading); // inherited
        assert_eq!(
            section.slide.background_image.as_deref(),
            Some("deck-bg.png") // inherited from base
        );
        assert_eq!(section.slide_number, theme.slide_number); // inherited

        // An empty overlay resolves to the base theme (modulo overlays).
        let plain = ThemeOverlay::default().apply(&theme);
        assert_eq!(plain.colors, theme.colors);
        assert_eq!(plain.slide, theme.slide);
    }

    #[test]
    fn overlay_bars_replace_single_bar() {
        let toml = r##"
name = "bars"
code_theme = "base16-ocean.dark"

[colors]
background = "#000000"
text = "#ffffff"
heading = "#ffffff"
accent = "#ffffff"
link = "#ffffff"
muted = "#888888"
code_background = "#111111"

[fonts]
body_size = 36
h1_size = 80
h2_size = 56
h3_size = 44
code_size = 30

[spacing]
slide_padding = 60
paragraph_gap = 20

[slide]
bar = { side = "bottom", size = 16, color = "#111111", reserve = true }

[title.slide]
bars = [
  { side = "top", size = 10, color = "#222222" },
  { side = "bottom", size = 10, color = "#333333" },
]
"##;
        let theme = Theme::from_toml(toml).unwrap();
        // Base: a single bottom bar that reserves content space.
        let bar = theme.slide.bar.unwrap();
        assert_eq!(bar.side, Side::Bottom);
        assert!(bar.reserve);
        assert!(theme.slide.bars.is_empty());

        // Title overlay: two bars replace the inherited single bar.
        let title = theme.title.apply(&theme);
        assert_eq!(title.slide.bar, None);
        assert_eq!(title.slide.bars.len(), 2);
        assert_eq!(title.slide.bars[0].side, Side::Top);
        assert_eq!(title.slide.bars[1].side, Side::Bottom);

        // A kind with no bar override inherits the base single bar.
        let plain = ThemeOverlay::default().apply(&theme);
        assert_eq!(plain.slide.bar, theme.slide.bar);
        assert!(plain.slide.bars.is_empty());
    }

    #[test]
    fn slide_number_defaults() {
        let toml = r##"
name = "x"
code_theme = "base16-ocean.dark"

[colors]
background = "#000000"
text = "#ffffff"
heading = "#ffffff"
accent = "#ffffff"
link = "#ffffff"
muted = "#888888"
code_background = "#111111"

[fonts]
body_size = 36
h1_size = 80
h2_size = 56
h3_size = 44
code_size = 30

[spacing]
slide_padding = 60
paragraph_gap = 20

[slide_number]
"##;
        let theme = Theme::from_toml(toml).unwrap();
        let number = theme.slide_number.unwrap();
        assert_eq!(number.format, "{current}");
        assert_eq!(number.size, 24.0);
        assert_eq!(number.font, None);
        assert_eq!(number.position, Corner::BottomRight);
        assert_eq!(number.color, None);

        // Without the section the stamp is off entirely.
        let dark = crate::registry::builtin("dark").unwrap();
        assert_eq!(dark.slide_number, None);
    }

    #[test]
    fn slide_style_defaults_for_v2_themes() {
        // The built-in themes have no [slide] section; everything defaults.
        let theme = crate::registry::builtin("dark").unwrap();
        assert_eq!(theme.slide, SlideStyle::default());
        assert_eq!(theme.fonts.body_family, None);
    }

    #[test]
    fn code_block_focus_mode_parses() {
        let toml = r##"
name = "x"
code_theme = "base16-ocean.dark"

[colors]
background = "#000000"
text = "#ffffff"
heading = "#ffffff"
accent = "#ffffff"
link = "#ffffff"
muted = "#888888"
code_background = "#111111"

[fonts]
body_size = 36
h1_size = 80
h2_size = 56
h3_size = 44
code_size = 30

[spacing]
slide_padding = 60
paragraph_gap = 20

[code_block]
highlight_style = "dim"
dim_opacity = 0.2
"##;
        let theme = Theme::from_toml(toml).unwrap();
        assert_eq!(theme.code_block.highlight_style, Some(HighlightStyle::Dim));
        assert_eq!(theme.code_block.dim_opacity, Some(0.2));
        // Default (no override) leaves focus mode off.
        assert_eq!(CodeBlockStyle::default().highlight_style, None);
    }

    #[test]
    fn quote_style_parses_and_defaults() {
        let toml = r##"
name = "x"
code_theme = "base16-ocean.dark"

[colors]
background = "#000000"
text = "#ffffff"
heading = "#ffffff"
accent = "#ff00ff"
link = "#ffffff"
muted = "#888888"
code_background = "#111111"

[fonts]
body_size = 36
h1_size = 80
h2_size = 56
h3_size = 44
code_size = 30

[spacing]
slide_padding = 60
paragraph_gap = 20

[quote]
background = "#172230"
border_color = "#f5c2e7"
border_width = 6
italic = true
align = "center"
"##;
        let theme = Theme::from_toml(toml).unwrap();
        assert_eq!(theme.quote.border_width, Some(6.0));
        assert_eq!(theme.quote.italic, Some(true));
        assert_eq!(theme.quote.align, Some(HorizontalAlign::Center));
        // The built-in themes set no quote styling (renderer uses defaults).
        assert_eq!(Theme::default().quote, QuoteStyle::default());
    }

    #[test]
    fn unknown_theme_keys_are_rejected() {
        // Catches typos in user themes instead of silently ignoring them.
        let toml = r##"
name = "x"
code_theme = "base16-ocean.dark"
typo_field = 1

[colors]
background = "#000000"
text = "#ffffff"
heading = "#ffffff"
accent = "#ffffff"
link = "#ffffff"
muted = "#888888"
code_background = "#111111"

[fonts]
body_size = 36
h1_size = 80
h2_size = 56
h3_size = 44
code_size = 30

[spacing]
slide_padding = 60
paragraph_gap = 20
"##;
        assert!(Theme::from_toml(toml).is_err());
    }
}

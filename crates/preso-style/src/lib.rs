//! preso-style: theme configuration.
//!
//! Themes are TOML files deserialized into a typed [`Theme`]. All sizes are
//! in **design units** on the 1920×1080 virtual canvas; the app
//! scales them to the actual window. This crate has no GUI dependencies.

pub mod model;
pub mod registry;

pub use model::{
    AccentBar, CodeBlockStyle, Color, Corner, Fonts, FontsOverlay, Footnote, Gradient,
    HeadingStyle, HighlightStyle, HorizontalAlign, ImageBorder, ImageShadow, ImageStyle, Logo,
    Palette, PaletteOverlay, QuoteStyle, ShadowSetting, Side, SlideNumber, SlideStyle,
    SlideStyleOverlay, Spacing, SpacingOverlay, TableStyle, Theme, ThemeOverlay, VerticalAlign,
};
pub use registry::{ThemeError, load, load_with_search};

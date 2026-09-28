//! preso-core: markdown → slide deck model.
//!
//! Parsing rules are specified in the project plan §6.2. The load-bearing
//! rule: `---` on its own line splits slides, but only outside fenced code
//! blocks.

pub mod complete;
pub mod edit;
pub mod error;
pub mod fence;
pub mod hover;
pub mod include;
pub mod lint;
pub mod model;
pub mod outline;
pub mod parser;
pub mod state;
pub mod style;

pub use error::ParseError;
pub use model::{
    Anchor, CodeBlock, Frontmatter, Highlight, HighlightMode, HighlightShape, ImageRef, ImageRow,
    ImageText, ImageTextRun, LayerImage, Layout, MathBlock, Note, Slide, SlideOverrides, SlideZoom,
    Table, TableAlign, ZoomFocus, display_number, display_total,
};
pub use state::Deck;

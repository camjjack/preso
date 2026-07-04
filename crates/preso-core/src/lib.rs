//! preso-core: markdown → slide deck model.
//!
//! Parsing rules are specified in the project plan §6.2. The load-bearing
//! rule: `---` on its own line splits slides, but only outside fenced code
//! blocks.

pub mod error;
pub mod fence;
pub mod model;
pub mod parser;
pub mod state;

pub use error::ParseError;
pub use model::{
    Anchor, CodeBlock, Frontmatter, ImageRef, ImageRow, LayerImage, Layout, MathBlock, Note, Slide,
    SlideOverrides, Table, TableAlign, display_number, display_total,
};
pub use state::Deck;

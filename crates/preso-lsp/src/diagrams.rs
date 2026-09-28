//! Diagrams drawn to check and complete their zoom stages' node labels.
//!
//! Whether `{all|Layout zoom}` names a node is only known once the diagram
//! is laid out, so this draws it with the same renderer and font the app
//! uses (`preso_diagram`) and keeps the result: a diagram's labels are
//! asked about on every keystroke, but its source rarely changes.

use preso_diagram::Renderer;
use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;

/// The body font the app draws diagram text in. Labels have to be shaped
/// to be found at all (text no font can shape is dropped), so the server
/// can't rely on what the machine has installed.
const INTER_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");

/// A diagram by (language, source).
type Key = (String, String);

/// Most diagrams kept drawn before the cache starts over.
const MAX_DRAWN: usize = 64;

#[derive(Default)]
pub struct Diagrams {
    /// Built on first use: loading the system fonts takes a moment, and most
    /// decks have no diagram that zooms.
    renderer: OnceCell<Renderer>,
    /// Each diagram's labels by (language, source); `None` if it doesn't
    /// draw.
    drawn: RefCell<HashMap<Key, Option<Vec<String>>>>,
}

impl Diagrams {
    fn renderer(&self) -> &Renderer {
        self.renderer
            .get_or_init(|| Renderer::new(&[INTER_REGULAR], "Inter", "Inter"))
    }

    /// The labels a diagram draws, or `None` if it doesn't render (a syntax
    /// error the author is still typing through, say).
    pub fn labels(&self, language: &str, source: &str) -> Option<Vec<String>> {
        let key = (language.to_string(), source.to_string());
        if let Some(labels) = self.drawn.borrow().get(&key) {
            return labels.clone();
        }
        let renderer = self.renderer();
        let svg = match language {
            "mermaid" => renderer.mermaid_svg(source, false).ok(),
            _ => renderer.graphviz_svg(source).ok(),
        };
        let labels = svg.and_then(|svg| renderer.labels(&svg).ok());
        let mut drawn = self.drawn.borrow_mut();
        if drawn.len() >= MAX_DRAWN {
            drawn.clear();
        }
        drawn.insert(key, labels.clone());
        labels
    }

    /// Whether a zoom stage's `label` finds a node, matched the way the app
    /// matches it: case-insensitively with whitespace collapsed, exactly if
    /// any label matches so, else as part of one. `None` if the diagram
    /// doesn't render, so there's nothing to say.
    pub fn finds(&self, language: &str, source: &str, label: &str) -> Option<bool> {
        let normal = |s: &str| {
            s.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
        };
        let wanted = normal(label);
        let labels = self.labels(language, source)?;
        Some(labels.iter().any(|l| normal(l).contains(&wanted)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_found_as_the_app_finds_them() {
        let diagrams = Diagrams::default();
        let src = "graph LR\n  a[Parse input] --> b[Layout]\n";
        assert_eq!(
            diagrams.labels("mermaid", src).unwrap(),
            ["Parse input", "Layout"]
        );
        assert_eq!(diagrams.finds("mermaid", src, "layout"), Some(true));
        assert_eq!(diagrams.finds("mermaid", src, "Parse  input"), Some(true));
        // Part of a label is enough, as in `label_region`.
        assert_eq!(diagrams.finds("mermaid", src, "Parse"), Some(true));
        assert_eq!(diagrams.finds("mermaid", src, "Paint"), Some(false));
        // Graphviz too.
        let dot = "digraph { parse -> layout }";
        assert_eq!(diagrams.finds("dot", dot, "layout"), Some(true));
    }

    #[test]
    fn the_example_talk_zooms_onto_nodes_it_has() {
        let src = include_str!("../../../docs/example-talk.md");
        let diagrams = Diagrams::default();
        for diagram in preso_core::lint::check(src).diagrams {
            for (label, _) in &diagram.labels {
                assert_eq!(
                    diagrams.finds(&diagram.language, &diagram.source, label),
                    Some(true),
                    "{label}"
                );
            }
        }
    }

    #[test]
    fn a_diagram_that_wont_draw_says_nothing() {
        let diagrams = Diagrams::default();
        assert_eq!(diagrams.finds("dot", "digraph {", "a"), None);
    }
}

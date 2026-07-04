use crate::error::ParseError;
use crate::model::{Frontmatter, Slide};
use crate::parser;

/// A loaded deck plus the current navigation position (slide + reveal step).
#[derive(Debug, Clone, PartialEq)]
pub struct Deck {
    pub frontmatter: Frontmatter,
    slides: Vec<Slide>,
    current: usize,
    step: usize,
}

impl Deck {
    pub fn from_source(source: &str) -> Result<Self, ParseError> {
        let parsed = parser::parse(source)?;
        Ok(Self {
            frontmatter: parsed.frontmatter,
            slides: parsed.slides,
            current: 0,
            step: 0,
        })
    }

    pub fn slides(&self) -> &[Slide] {
        &self.slides
    }

    pub fn len(&self) -> usize {
        self.slides.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slides.is_empty()
    }

    pub fn current_index(&self) -> usize {
        self.current
    }

    pub fn current_step(&self) -> usize {
        self.step
    }

    pub fn current_slide(&self) -> &Slide {
        &self.slides[self.current]
    }

    /// The 1-based number to *display* for the slide at `index`, honoring
    /// `<!-- slide: number=N -->` resets (see [`crate::display_number`]).
    pub fn display_number(&self, index: usize) -> usize {
        crate::display_number(&self.slides, index)
    }

    /// The largest display number across the deck, for `{total}`.
    pub fn display_total(&self) -> usize {
        crate::display_total(&self.slides)
    }

    pub fn next_slide(&self) -> Option<&Slide> {
        self.slides.get(self.current + 1)
    }

    /// Advance one reveal step, or to the next slide when fully revealed.
    pub fn next(&mut self) {
        if self.step + 1 < self.current_slide().step_count() {
            self.step += 1;
        } else if self.current + 1 < self.slides.len() {
            self.current += 1;
            self.step = 0;
        }
    }

    /// Go back one reveal step, or to the previous slide (fully revealed).
    pub fn prev(&mut self) {
        if self.step > 0 {
            self.step -= 1;
        } else if self.current > 0 {
            self.current -= 1;
            self.step = self.current_slide().step_count() - 1;
        }
    }

    pub fn first(&mut self) {
        self.current = 0;
        self.step = 0;
    }

    pub fn last(&mut self) {
        self.current = self.slides.len() - 1;
        self.step = 0;
    }

    pub fn jump(&mut self, index: usize) {
        self.current = index.min(self.slides.len() - 1);
        self.step = 0;
    }

    /// Replace deck contents after a hot reload, preserving the current
    /// position (clamped if the deck shrank).
    pub fn reload(&mut self, source: &str) -> Result<(), ParseError> {
        let parsed = parser::parse(source)?;
        self.frontmatter = parsed.frontmatter;
        self.slides = parsed.slides;
        self.current = self.current.min(self.slides.len() - 1);
        self.step = self.step.min(self.current_slide().step_count() - 1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deck(n: usize) -> Deck {
        let src = (0..n)
            .map(|i| format!("# Slide {i}\n"))
            .collect::<Vec<_>>()
            .join("\n---\n");
        Deck::from_source(&src).unwrap()
    }

    #[test]
    fn navigation_clamps_at_both_ends() {
        let mut d = deck(3);
        d.prev();
        assert_eq!(d.current_index(), 0);
        d.next();
        d.next();
        d.next(); // past the end
        assert_eq!(d.current_index(), 2);
    }

    #[test]
    fn first_last_jump() {
        let mut d = deck(5);
        d.last();
        assert_eq!(d.current_index(), 4);
        d.first();
        assert_eq!(d.current_index(), 0);
        d.jump(3);
        assert_eq!(d.current_index(), 3);
        d.jump(99);
        assert_eq!(d.current_index(), 4);
    }

    #[test]
    fn next_slide_lookahead() {
        let mut d = deck(2);
        assert!(d.next_slide().is_some());
        d.next();
        assert!(d.next_slide().is_none());
    }

    #[test]
    fn steps_advance_before_slides() {
        let src = "a\n<!-- pause -->\nb\n---\n# Two\n";
        let mut d = Deck::from_source(src).unwrap();
        assert_eq!((d.current_index(), d.current_step()), (0, 0));
        d.next();
        assert_eq!((d.current_index(), d.current_step()), (0, 1));
        d.next();
        assert_eq!((d.current_index(), d.current_step()), (1, 0));
    }

    #[test]
    fn prev_returns_to_fully_revealed_slide() {
        let src = "a\n<!-- pause -->\nb\n---\n# Two\n";
        let mut d = Deck::from_source(src).unwrap();
        d.next();
        d.next(); // on slide 2
        d.prev();
        // back on slide 1, fully revealed
        assert_eq!((d.current_index(), d.current_step()), (0, 1));
        d.prev();
        assert_eq!((d.current_index(), d.current_step()), (0, 0));
    }

    #[test]
    fn reload_preserves_position() {
        let mut d = deck(5);
        d.jump(4);
        d.reload("# a\n---\n# b\n").unwrap();
        assert_eq!(d.current_index(), 1); // clamped to new length
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn reload_clamps_step() {
        let src = "a\n<!-- pause -->\nb\n";
        let mut d = Deck::from_source(src).unwrap();
        d.next();
        assert_eq!(d.current_step(), 1);
        d.reload("a only\n").unwrap();
        assert_eq!(d.current_step(), 0);
    }
}

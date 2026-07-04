use iced::keyboard::key::Named;
use iced::keyboard::{Event, Key};

#[derive(Debug, Clone, Copy)]
pub enum Action {
    Next,
    Prev,
    First,
    Last,
    /// 1-based slide number from `<digits><Enter>`.
    Jump(usize),
    ToggleFullscreen,
    ResetTimer,
    /// Escape: open/close the slide overview grid (presenter).
    ToggleOverview,
    /// `L`: toggle the laser pointer on the audience window.
    ToggleLaser,
    /// `P`: toggle pen annotation mode on the audience window.
    TogglePen,
    /// `C`: clear pen annotations.
    ClearAnnotations,
    /// `V`: play the current slide's `<!-- video: … -->` clip in an external
    /// fullscreen player.
    PlayVideo,
    /// Overview grid is open: scroll it to the top / bottom / by a page.
    OverviewTop,
    OverviewBottom,
    OverviewPageUp,
    OverviewPageDown,
}

/// Map a keyboard event to a navigation action. `jump_buffer` accumulates
/// typed digits for number+Enter jumps. When `overview` is set (the grid is
/// open), Home/End/PageUp/PageDown scroll the grid instead of navigating the
/// deck; the arrow keys still move the highlighted slide.
pub fn action(event: &Event, jump_buffer: &mut String, overview: bool) -> Option<Action> {
    let Event::KeyPressed { key, .. } = event else {
        return None;
    };
    if overview {
        // Grid-scroll keys take precedence while the overview is open.
        match key.as_ref() {
            Key::Named(Named::Home) => {
                jump_buffer.clear();
                return Some(Action::OverviewTop);
            }
            Key::Named(Named::End) => {
                jump_buffer.clear();
                return Some(Action::OverviewBottom);
            }
            Key::Named(Named::PageUp) => {
                jump_buffer.clear();
                return Some(Action::OverviewPageUp);
            }
            Key::Named(Named::PageDown) => {
                jump_buffer.clear();
                return Some(Action::OverviewPageDown);
            }
            _ => {}
        }
    }
    match key.as_ref() {
        Key::Named(Named::ArrowRight | Named::ArrowDown | Named::Space | Named::PageDown) => {
            jump_buffer.clear();
            Some(Action::Next)
        }
        Key::Named(Named::ArrowLeft | Named::ArrowUp | Named::Backspace | Named::PageUp) => {
            jump_buffer.clear();
            Some(Action::Prev)
        }
        Key::Named(Named::Home) => {
            jump_buffer.clear();
            Some(Action::First)
        }
        Key::Named(Named::End) => {
            jump_buffer.clear();
            Some(Action::Last)
        }
        Key::Named(Named::Enter) => {
            let n: usize = jump_buffer.parse().ok()?;
            jump_buffer.clear();
            Some(Action::Jump(n))
        }
        Key::Named(Named::Escape) => {
            jump_buffer.clear();
            Some(Action::ToggleOverview)
        }
        Key::Character("f") => {
            jump_buffer.clear();
            Some(Action::ToggleFullscreen)
        }
        Key::Character("r") => {
            jump_buffer.clear();
            Some(Action::ResetTimer)
        }
        Key::Character("l") => {
            jump_buffer.clear();
            Some(Action::ToggleLaser)
        }
        Key::Character("p") => {
            jump_buffer.clear();
            Some(Action::TogglePen)
        }
        Key::Character("c") => {
            jump_buffer.clear();
            Some(Action::ClearAnnotations)
        }
        Key::Character("v") => {
            jump_buffer.clear();
            Some(Action::PlayVideo)
        }
        Key::Character(c) if c.chars().all(|c| c.is_ascii_digit()) => {
            jump_buffer.push_str(c);
            None
        }
        _ => None,
    }
}

//! Persisted window geometry: `~/.config/preso/window-state.toml`.
//!
//! Positions are global desktop coordinates, which on every platform we
//! target encode *which display* a window sits on — so restoring position
//! restores the external-display setup from the previous run. (iced 0.14
//! has no monitor enumeration, so geometry is the whole story for now;
//! see plan §16.2.)

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub audience: Option<Geometry>,
    pub presenter: Option<Geometry>,
}

fn state_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("preso").join("window-state.toml"))
}

pub fn load() -> WindowState {
    let Some(path) = state_path() else {
        return WindowState::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(source) => toml::from_str(&source).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "ignoring malformed window-state.toml");
            WindowState::default()
        }),
        Err(_) => WindowState::default(),
    }
}

pub fn save(state: &WindowState) {
    let Some(path) = state_path() else { return };
    let Ok(serialized) = toml::to_string_pretty(state) else {
        return;
    };
    if let Some(dir) = path.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        tracing::warn!(error = %e, "cannot create config dir");
        return;
    }
    if let Err(e) = std::fs::write(&path, serialized) {
        tracing::warn!(error = %e, "cannot save window state");
    }
}

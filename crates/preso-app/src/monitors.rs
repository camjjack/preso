//! Display detection for automatic window placement (plan §15 default
//! behavior). iced 0.14 has no monitor API, so displays come from the
//! `display-info` crate; their global coordinates map directly onto
//! window positions.

/// One display in logical (point) coordinates. Width/height are kept for
/// future placement logic (e.g. centering) even though only the origin
/// drives positioning today.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct Monitor {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Setup {
    pub primary: Monitor,
    /// The largest non-primary display, when one exists.
    pub secondary: Option<Monitor>,
}

pub fn detect() -> Option<Setup> {
    let displays = display_info::DisplayInfo::all()
        .map_err(|e| tracing::warn!(error = %e, "display detection failed"))
        .ok()?;

    let logical = |d: &display_info::DisplayInfo| {
        // macOS reports logical points already; Windows/X11 report
        // physical pixels, which scale_factor maps back to points.
        let scale = if cfg!(target_os = "macos") {
            1.0
        } else {
            d.scale_factor.max(0.5)
        };
        Monitor {
            x: d.x as f32 / scale,
            y: d.y as f32 / scale,
            width: d.width as f32 / scale,
            height: d.height as f32 / scale,
        }
    };

    let primary = displays.iter().find(|d| d.is_primary)?;
    let secondary = displays
        .iter()
        .filter(|d| !d.is_primary)
        .max_by_key(|d| u64::from(d.width) * u64::from(d.height));

    tracing::debug!(
        displays = displays.len(),
        primary = %primary.friendly_name,
        secondary = secondary.map(|d| d.friendly_name.clone()).unwrap_or_default(),
        "display setup"
    );

    Some(Setup {
        primary: logical(primary),
        secondary: secondary.map(logical),
    })
}

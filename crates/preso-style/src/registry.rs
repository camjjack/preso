use crate::model::Theme;
use std::path::Path;
use thiserror::Error;

const DARK: &str = include_str!("../../../assets/themes/dark.toml");
const LIGHT: &str = include_str!("../../../assets/themes/light.toml");

#[derive(Debug, Error)]
pub enum ThemeError {
    #[error("unknown theme {0:?}: not a built-in (dark, light) and not a readable file")]
    NotFound(String),

    #[error("failed to read theme file {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },

    #[error("invalid theme TOML: {0}")]
    Toml(#[from] toml::de::Error),
}

/// Built-in theme by name.
pub fn builtin(name: &str) -> Option<Theme> {
    let source = match name {
        "dark" => DARK,
        "light" => LIGHT,
        _ => return None,
    };
    Some(Theme::from_toml(source).expect("built-in themes must be valid"))
}

/// Resolve a theme: built-in name first, then a path to a `.toml` file.
pub fn load(name_or_path: &str) -> Result<Theme, ThemeError> {
    load_with_search(name_or_path, &[])
}

/// Resolve a theme, also checking `<dir>/<name>.toml` for each search
/// directory (e.g. the user's `…/preso/themes/`). Precedence:
/// built-in name → search dirs → literal file path.
///
/// A relative path is taken from the current directory: right for a
/// `--theme` typed on the command line. A deck's frontmatter `theme:` wants
/// [`load_for_deck`].
pub fn load_with_search(
    name_or_path: &str,
    search_dirs: &[std::path::PathBuf],
) -> Result<Theme, ThemeError> {
    load_resolved(name_or_path, search_dirs, None)
}

/// [`load_with_search`] for a theme named in a deck's frontmatter: a
/// relative path is found next to the deck (in `deck_dir`), wherever preso
/// was started, as the deck's images and includes are. The current
/// directory is still tried after it, for decks written when that was the
/// only place looked.
pub fn load_for_deck(
    name_or_path: &str,
    search_dirs: &[std::path::PathBuf],
    deck_dir: &Path,
) -> Result<Theme, ThemeError> {
    load_resolved(name_or_path, search_dirs, Some(deck_dir))
}

fn load_resolved(
    name_or_path: &str,
    search_dirs: &[std::path::PathBuf],
    deck_dir: Option<&Path>,
) -> Result<Theme, ThemeError> {
    if let Some(theme) = builtin(name_or_path) {
        return Ok(theme);
    }
    let read = |path: &Path| -> Result<Theme, ThemeError> {
        let source = std::fs::read_to_string(path).map_err(|source| ThemeError::Io {
            path: path.display().to_string(),
            source,
        })?;
        let mut theme = Theme::from_toml(&source)?;
        theme.source_dir = path.parent().map(Path::to_path_buf);
        // Resolve theme-relative asset paths once, here. Absolutize so
        // downstream joins against other base dirs leave them intact.
        if let Some(dir) = &theme.source_dir {
            let abs = |rel: &str| {
                let joined = dir.join(rel);
                std::path::absolute(&joined)
                    .unwrap_or(joined)
                    .display()
                    .to_string()
            };
            let logos = [
                theme.slide.logo.as_mut(),
                theme.title.slide.logo.as_mut(),
                theme.section.slide.logo.as_mut(),
            ];
            for logo in logos.into_iter().flatten() {
                if !logo.path.is_empty() {
                    logo.path = abs(&logo.path);
                }
            }
            let backgrounds = [
                theme.slide.background_image.as_mut(),
                theme.title.slide.background_image.as_mut(),
                theme.section.slide.background_image.as_mut(),
            ];
            for bg in backgrounds.into_iter().flatten() {
                *bg = abs(bg);
            }
        }
        Ok(theme)
    };
    for dir in search_dirs {
        let candidate = dir.join(format!("{name_or_path}.toml"));
        if candidate.is_file() {
            return read(&candidate);
        }
    }
    let path = Path::new(name_or_path);
    if let Some(dir) = deck_dir.filter(|_| path.is_relative()) {
        let beside = dir.join(path);
        if beside.is_file() {
            return read(&beside);
        }
    }
    if path.is_file() {
        return read(path);
    }
    Err(ThemeError::NotFound(name_or_path.to_string()))
}

impl Default for Theme {
    fn default() -> Self {
        builtin("dark").expect("dark theme is built-in")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_load() {
        for name in ["dark", "light"] {
            let theme = builtin(name).unwrap();
            assert_eq!(theme.name, name);
            assert!(theme.fonts.h1_size > theme.fonts.body_size);
        }
    }

    #[test]
    fn load_falls_through_name_then_path() {
        assert!(load("dark").is_ok());
        assert!(matches!(
            load("no-such-theme"),
            Err(ThemeError::NotFound(_))
        ));
    }

    #[test]
    fn a_deck_finds_its_theme_beside_it() {
        let dir = std::env::temp_dir().join(format!("preso-theme-beside-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("themes")).unwrap();
        let toml = DARK.replacen("name = \"dark\"", "name = \"beside\"", 1);
        std::fs::write(dir.join("themes/house.toml"), toml).unwrap();

        // Relative to the deck, whatever the current directory is (the test
        // runs from the crate, where there's no `themes/house.toml`).
        let theme = load_for_deck("themes/house.toml", &[], &dir).unwrap();
        assert_eq!(theme.name, "beside");
        // Its own asset paths still resolve against the theme file.
        assert_eq!(
            theme.source_dir.as_deref(),
            Some(dir.join("themes").as_path())
        );
        // A command-line path is still taken from the current directory.
        assert!(load_with_search("themes/house.toml", &[]).is_err());
        // Built-ins and absolute paths are unaffected.
        assert_eq!(load_for_deck("light", &[], &dir).unwrap().name, "light");
        let absolute = dir.join("themes/house.toml").display().to_string();
        assert_eq!(
            load_for_deck(&absolute, &[], Path::new("/nonexistent"))
                .unwrap()
                .name,
            "beside"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_from_file() {
        let dir = std::env::temp_dir().join(format!("preso-style-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("custom.toml");
        let custom = include_str!("../../../assets/themes/light.toml")
            .replace("name = \"light\"", "name = \"custom\"");
        std::fs::write(&path, custom).unwrap();
        let theme = load(path.to_str().unwrap()).unwrap();
        assert_eq!(theme.name, "custom");
    }
}

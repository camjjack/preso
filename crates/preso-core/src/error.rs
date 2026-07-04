use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("frontmatter is not valid YAML: {0}")]
    Frontmatter(#[from] serde_norway::Error),
}


#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    #[error("{upload_name}/{rolling_prefix_or_tag} already exists")]
    Conflict {
        upload_name: String,
        rolling_prefix_or_tag: String,
    },
}

impl Error {
    pub(crate) fn should_suggest_issue(&self) -> bool {
        match self {
            Self::Unauthorized(_) | Self::Conflict { .. } => false,
        }
    }
}
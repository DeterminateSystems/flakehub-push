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
    pub(crate) fn maybe_github_actions_annotation(&self) {
        if std::env::var("GITHUB_ACTIONS").is_ok() {
            match self {
                Error::Unauthorized(message) => println!("::error title=Unauthorized::<<EOF\n{message}\nEOF"),
                Error::Conflict { .. } => println!("::error title=Conflict::<<EOF\n{self}\nEOF"),
            }
        }
    }
}

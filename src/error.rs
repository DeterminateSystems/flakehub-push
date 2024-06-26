#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    /// Unauthorized, with a single line message detailing the nature of the problem.
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    #[error("{upload_name}/{release_version} already exists")]
    Conflict {
        upload_name: String,
        release_version: String,
    },
    #[error("Bad request: {0}")]
    BadRequest(String),
}

impl Error {
    pub(crate) fn should_suggest_issue(&self) -> bool {
        match self {
            Self::Unauthorized(_) | Self::Conflict { .. } | Self::BadRequest(_) => false,
        }
    }

    /// Output a Github Actions annotation command if desired.
    // Note: These may only be one line! Any further lines will not be printed!
    pub(crate) fn maybe_github_actions_annotation(&self) {
        if std::env::var("GITHUB_ACTIONS").is_ok() {
            match self {
                Error::Unauthorized(message) => {
                    println!("::error title=Unauthorized::{message}")
                }
                Error::Conflict { .. } => println!("::error title=Conflict::{self}"),
                Error::BadRequest(_) => println!("::error title=BadRequest::{self}"),
            }
        }
    }
}

#![allow(unreachable_code, unused_variables)]

use std::{fmt::Display, io::IsTerminal};

use clap::Parser;
use error::Error;
mod cli;
mod error;
mod flake_info;
mod graphql;
mod release_metadata;

#[tokio::main]
async fn main() -> color_eyre::Result<std::process::ExitCode> {
    color_eyre::config::HookBuilder::default()
        .issue_url(concat!(env!("CARGO_PKG_REPOSITORY"), "/issues/new"))
        .add_issue_metadata("version", env!("CARGO_PKG_VERSION"))
        .add_issue_metadata("os", std::env::consts::OS)
        .add_issue_metadata("arch", std::env::consts::ARCH)
        .theme(if !std::io::stderr().is_terminal() {
            color_eyre::config::Theme::new()
        } else {
            color_eyre::config::Theme::dark()
        })
        .issue_filter(|kind| match kind {
            color_eyre::ErrorKind::NonRecoverable(_) => true,
            color_eyre::ErrorKind::Recoverable(error) => {
                if let Some(known_error) = error.downcast_ref::<Error>() {
                    known_error.should_suggest_issue()
                } else {
                    true
                }
            }
        })
        .install()?;

    let cli = cli::FlakeHubPushCli::parse();
    cli.instrumentation.setup()?;
    cli.execute().await
}

#[derive(Debug, Clone, Copy, clap::ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Visibility {
    Public,
    // a backwards-compatible alias to unlisted
    #[serde(rename = "unlisted")]
    Hidden,
    Unlisted,
    Private,
}

impl Display for Visibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Visibility::Public => f.write_str("public"),
            Visibility::Hidden | Visibility::Unlisted => f.write_str("unlisted"),
            Visibility::Private => f.write_str("private"),
        }
    }
}

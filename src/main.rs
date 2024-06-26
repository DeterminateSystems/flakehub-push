use std::{fmt::Display, io::IsTerminal, process::ExitCode};

use clap::Parser;
use color_eyre::eyre::{eyre, Result};
use error::Error;
use http::StatusCode;

use crate::{
    flakehub_client::{FlakeHubClient, StageResult},
    push_context::PushContext,
};
mod cli;
mod error;
mod flake_info;
mod flakehub_auth_fake;
mod flakehub_client;
mod git_context;
mod github;
mod gitlab;
mod push_context;
mod release_metadata;
mod revision_info;
mod s3;

const DEFAULT_ROLLING_PREFIX: &str = "0.1";

pub(crate) fn build_http_client() -> reqwest::ClientBuilder {
    reqwest::Client::builder().user_agent("flakehub-push")
}

#[tokio::main]
async fn main() -> Result<std::process::ExitCode> {
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

    match execute().await {
        Ok(exit) => Ok(exit),
        Err(error) => {
            if let Some(known_error) = error.downcast_ref::<Error>() {
                known_error.maybe_github_actions_annotation()
            }
            Err(error)
        }
    }
}

async fn execute() -> Result<std::process::ExitCode> {
    let ctx = {
        let mut cli = cli::FlakeHubPushCli::parse();
        cli.instrumentation.setup()?;
        PushContext::from_cli_and_env(&mut cli).await?
    };

    let fhclient = FlakeHubClient::new(ctx.flakehub_host, ctx.auth_token)?;

    // "upload.rs" - stage the release
    let stage_result = fhclient
        .release_stage(
            &ctx.upload_name,
            &ctx.release_version,
            &ctx.metadata,
            &ctx.tarball,
        )
        .await;

    let stage_result: StageResult = match stage_result {
        Err(e) => {
            return Err(e)?;
        }
        Ok(response) => {
            let response_status = response.status();
            match response_status {
                StatusCode::OK => {
                    let stage_result: StageResult = response
                        .json()
                        .await
                        .map_err(|_| eyre!("Decoding release metadata POST response"))?;

                    stage_result
                }
                StatusCode::CONFLICT => {
                    tracing::info!(
                        "Release for revision `{revision}` of {upload_name}/{release_version} already exists; flakehub-push will not upload it again",
                        revision = &ctx.metadata.revision,
                        upload_name = ctx.upload_name,
                        release_version = &ctx.release_version,
                    );
                    if ctx.error_if_release_conflicts {
                        return Err(Error::Conflict {
                            upload_name: ctx.upload_name.to_string(),
                            release_version: ctx.release_version.to_string(),
                        })?;
                    } else {
                        // we're just done, and happy about it:
                        return Ok(ExitCode::SUCCESS);
                    }
                }
                StatusCode::UNAUTHORIZED => {
                    let body = response.bytes().await?;
                    let message = serde_json::from_slice::<String>(&body)?;

                    return Err(Error::Unauthorized(message))?;
                }
                _ => {
                    return if let Ok(body) = response.bytes().await {
                        let message = serde_json::from_slice::<String>(&body)?;
                        Err(eyre!(
                            "\
                            Status {} from metadata POST\n\
                            {}\
                        ",
                            response_status,
                            message
                        ))
                    } else {
                        Err(eyre!("Status {} from metadata POST", response_status))
                    }
                }
            }
        }
    };

    // upload tarball to s3
    s3::upload_release_to_s3(stage_result.s3_upload_url, ctx.tarball).await?;

    // "publish.rs" - publish the release after upload
    fhclient.release_publish(stage_result.uuid).await?;

    tracing::info!(
        "Successfully released new version of {}/{}",
        ctx.upload_name,
        ctx.release_version
    );

    Ok(ExitCode::SUCCESS)
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

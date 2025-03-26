use std::{fmt::Display, io::IsTerminal, process::ExitCode};

use clap::Parser;
use color_eyre::eyre::{eyre, Result};
use error::Error;
use http::StatusCode;
use reqwest::Response;

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
mod github_actions;
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
    let mut cli = cli::FlakeHubPushCli::parse();
    cli.instrumentation.setup()?;

    // NOTE(cole-h): If --dest-dir is passed, we're intentionally avoiding doing any actual
    // networking (i.e. for FlakeHub and GitHub)
    if let Some(dest_dir) = &cli.dest_dir.0 {
        let local_git_root = cli.resolve_local_git_root()?;
        let local_rev_info = revision_info::RevisionInfo::from_git_root(&local_git_root)?;
        let git_ctx = git_context::GitContext {
            spdx_expression: cli.spdx_expression.0.clone(),
            repo_topics: vec![],
            revision_info: local_rev_info,
        };

        let release_version = cli.release_version(&git_ctx)?;
        let release_tarball_name = format!("{release_version}.tar.gz");
        let release_json_name = format!("{release_version}.json");

        let (release_metadata, tarball) =
            release_metadata::ReleaseMetadata::new(&cli, &git_ctx, None).await?;

        std::fs::create_dir_all(dest_dir)?;

        {
            let dest_file = dest_dir.join(release_tarball_name);
            tracing::info!("Writing tarball to {}", dest_file.display());
            std::fs::write(dest_file, tarball.bytes)?;
        }

        {
            let dest_file = dest_dir.join(release_json_name);
            tracing::info!("Writing release metadata to {}", dest_file.display());
            std::fs::write(dest_file, serde_json::to_string(&release_metadata)?)?;
        }

        return Ok(ExitCode::SUCCESS);
    }

    let ctx = PushContext::from_cli_and_env(&mut cli).await?;

    let fhclient = FlakeHubClient::new(ctx.flakehub_host, ctx.auth_token)?;

    let response = fhclient.token_status().await?;
    if let Err(e) = response.error_for_status() {
        let was_client_error = e.status().is_some_and(|x| x.is_client_error());
        if std::env::var("GITHUB_ACTIONS").is_ok() {
            if was_client_error {
                tracing::error!("FlakeHub Unauthenticated: {}", e);
                github::print_unauthenticated_error();
            } else {
                println!("::error title=FlakeHub: Unauthenticated::Unable to authenticate to FlakeHub. {}", e);
            }
        }
        return Err(e.into());
    }

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

                    set_release_outputs(&ctx.upload_name, &ctx.release_version).await;

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
                    return Err(Error::Unauthorized(response_text(response).await))?;
                }
                StatusCode::BAD_REQUEST => {
                    return Err(Error::BadRequest(response_text(response).await))?;
                }
                _ => {
                    return Err(eyre!(
                        "\
                        Status {} from metadata POST\n\
                        {}\
                        ",
                        response_status,
                        response_text(response).await,
                    ));
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

    set_release_outputs(&ctx.upload_name, &ctx.release_version).await;

    Ok(ExitCode::SUCCESS)
}

async fn response_text(res: Response) -> String {
    if let Ok(message) = res.text().await {
        message
    } else {
        String::from("no body")
    }
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

async fn set_release_outputs(upload_name: &str, release_version: &str) {
    let outputs = [
        ("flake_name", upload_name),
        ("flake_version", release_version),
        (
            "flakeref_at_least",
            &format!("{}/{}", upload_name, release_version),
        ),
        (
            "flakeref_exact",
            &format!("{}/={}", upload_name, release_version),
        ),
    ];
    for (output_name, value) in outputs.into_iter() {
        if let Err(e) = github_actions::set_output(output_name, value).await {
            tracing::warn!(
                "Failed to set the `{}` output to {}: {}",
                output_name,
                value,
                e
            );
        }
    }
}

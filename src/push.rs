use color_eyre::eyre::{eyre, WrapErr};
use reqwest::{header::HeaderMap, StatusCode};
use std::{
    path::{Path, PathBuf},
    str::FromStr,
};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{
    build_http_client,
    error::Error,
    flake_info::{check_flake_evaluates, get_flake_metadata, get_flake_outputs, get_flake_tarball},
    release_metadata::ReleaseMetadata,
    Visibility,
};

const DEFAULT_ROLLING_PREFIX: &str = "0.1";

#[tracing::instrument(
    skip_all,
    fields(
        host,
        flake_root,
        subdir,
        revision,
        revision_count,
        repository,
        upload_name,
        mirror,
        %visibility,
        tag,
        rolling,
        rolling_minor,
        labels = labels.join(","),
        mirror,
        spdx_expression,
        error_if_release_conflicts,
        include_output_paths,
        project_id,
        owner_id,
    )
)]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn push_new_release(
    host: &str,
    upload_bearer_token: &str,
    flake_root: &Path,
    subdir: &Path,
    revision: String,
    revision_count: usize,
    upload_name: String,
    mirror: bool,
    visibility: Visibility,
    tag: Option<String>,
    rolling: bool,
    rolling_minor: Option<u64>,
    labels: Vec<String>,
    spdx_expression: Option<spdx::Expression>,
    error_if_release_conflicts: bool,
    include_output_paths: bool,
) -> color_eyre::Result<()> {
    let span = tracing::Span::current();
    span.record("upload_name", tracing::field::display(upload_name.clone()));

    let rolling_prefix_or_tag = match (rolling_minor.as_ref(), tag) {
        (Some(_), _) if !rolling => {
            return Err(eyre!(
                "You must enable `rolling` to upload a release with a specific `rolling-minor`."
            ));
        }
        (Some(minor), _) => format!("0.{minor}"),
        (None, _) if rolling => DEFAULT_ROLLING_PREFIX.to_string(),
        (None, Some(tag)) => {
            let version_only = tag.strip_prefix('v').unwrap_or(&tag);
            // Ensure the version respects semver
            semver::Version::from_str(version_only).wrap_err_with(|| eyre!("Failed to parse version `{tag}` as semver, see https://semver.org/ for specifications"))?;
            tag
        }
        (None, None) => {
            return Err(eyre!("Could not determine tag or rolling minor version, `--tag`, `GITHUB_REF_NAME`, or `--rolling-minor` must be set"));
        }
    };

    tracing::info!("Preparing release of {upload_name}/{rolling_prefix_or_tag}");

    let tempdir = tempfile::Builder::new()
        .prefix("flakehub_push")
        .tempdir()
        .wrap_err("Creating tempdir")?;

    let flake_dir = flake_root.join(subdir);

    check_flake_evaluates(&flake_dir)
        .await
        .wrap_err("Checking flake evaluates")?;
    let flake_metadata = get_flake_metadata(&flake_dir)
        .await
        .wrap_err("Getting flake metadata")?;
    tracing::debug!("Got flake metadata: {:?}", flake_metadata);

    // FIXME: bail out if flake_metadata denotes a dirty tree.

    let flake_locked_url = flake_metadata
        .get("url")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            eyre!("Could not get `url` attribute from `nix flake metadata --json` output")
        })?;
    tracing::debug!("Locked URL = {}", flake_locked_url);
    let flake_metadata_value_path = flake_metadata
        .get("path")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            eyre!("Could not get `path` attribute from `nix flake metadata --json` output")
        })?;
    let flake_metadata_value_resolved_dir = flake_metadata
        .pointer("/resolved/dir")
        .and_then(serde_json::Value::as_str);

    let flake_outputs = get_flake_outputs(flake_locked_url, include_output_paths).await?;
    tracing::debug!("Got flake outputs: {:?}", flake_outputs);

    let source = match flake_metadata_value_resolved_dir {
        Some(flake_metadata_value_resolved_dir) => {
            Path::new(flake_metadata_value_path).join(flake_metadata_value_resolved_dir)
        }
        None => PathBuf::from(flake_metadata_value_path),
    };
    span.record("source", tracing::field::display(source.clone().display()));
    tracing::debug!("Found source");

    if flake_dir.join("flake.lock").exists() {
        let output = tokio::process::Command::new("nix")
            .arg("flake")
            .arg("metadata")
            .arg("--json")
            .arg("--no-update-lock-file")
            .arg(&flake_dir)
            .output()
            .await
            .wrap_err_with(|| {
                eyre!(
                    "Failed to execute `nix flake metadata --json --no-update-lock-file {}`",
                    flake_dir.display()
                )
            })?;

        if !output.status.success() {
            let command = format!(
                "nix flake metadata --json --no-update-lock-file {}",
                flake_dir.display(),
            );
            let msg = format!(
                "\
                Failed to execute command `{command}`{maybe_status} \n\
                stdout: {stdout}\n\
                stderr: {stderr}\n\
                ",
                stdout = String::from_utf8_lossy(&output.stdout),
                stderr = String::from_utf8_lossy(&output.stderr),
                maybe_status = if let Some(status) = output.status.code() {
                    format!(" with status {status}")
                } else {
                    String::new()
                }
            );
            return Err(eyre!(msg))?;
        }
    }

    let last_modified = if let Some(last_modified) = flake_metadata.get("lastModified") {
        last_modified.as_u64().ok_or_else(|| {
            eyre!("`nix flake metadata --json` does not have a integer `lastModified` field")
        })?
    } else {
        return Err(eyre!(
            "`nix flake metadata` did not return a `lastModified` attribute"
        ));
    };
    tracing::debug!("lastModified = {}", last_modified);

    let flake_tarball = get_flake_tarball(&source, last_modified)
        .await
        .wrap_err("Making release tarball")?;

    let flake_tarball_len: usize = flake_tarball.len();
    let flake_tarball_hash = {
        let mut context = ring::digest::Context::new(&ring::digest::SHA256);
        context.update(&flake_tarball);
        context.finish()
    };
    let flake_tarball_hash_base64 = {
        // TODO: Use URL_SAFE_NO_PAD
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        STANDARD.encode(flake_tarball_hash)
    };
    tracing::debug!(
        flake_tarball_len,
        flake_tarball_hash_base64,
        "Got tarball metadata"
    );

    let flake_tarball_path = tempdir.path().join("release.tar.gz");
    let mut tempfile = tokio::fs::File::create(&flake_tarball_path)
        .await
        .wrap_err("Creating release.tar.gz")?;
    tempfile
        .write_all(&flake_tarball)
        .await
        .wrap_err("Writing compressed tarball to tempfile")?;

    let release_metadata = ReleaseMetadata::build(
        &source,
        subdir,
        revision,
        revision_count,
        flake_metadata,
        flake_outputs,
        upload_name.clone(),
        mirror,
        visibility,
        labels,
        spdx_expression,
    )
    .await
    .wrap_err("Building release metadata")?;

    let flakehub_client = build_http_client().build()?;

    let rolling_minor_with_postfix_or_tag = if rolling_minor.is_some() || rolling {
        format!(
            "{rolling_prefix_or_tag}.{}+rev-{}",
            release_metadata.commit_count, release_metadata.revision
        )
    } else {
        rolling_prefix_or_tag.to_string() // This will always be the tag since `self.rolling_prefix` was empty.
    };

    let release_metadata_post_url = format!(
        "{host}/upload/{upload_name}/{rolling_minor_with_postfix_or_tag}/{flake_tarball_len}/{flake_tarball_hash_base64}"
    );
    tracing::debug!(
        url = release_metadata_post_url,
        "Computed release metadata POST URL"
    );

    let flakehub_headers = {
        let mut header_map = HeaderMap::new();

        header_map.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_str("application/json").unwrap(),
        );
        header_map.insert(
            reqwest::header::HeaderName::from_static("ngrok-skip-browser-warning"),
            reqwest::header::HeaderValue::from_str("please").unwrap(),
        );
        header_map
    };

    let release_metadata_post_response = flakehub_client
        .post(release_metadata_post_url)
        .bearer_auth(upload_bearer_token)
        .headers(flakehub_headers.clone())
        .json(&release_metadata)
        .send()
        .await
        .wrap_err("Sending release metadata")?;

    let release_metadata_post_response_status = release_metadata_post_response.status();
    tracing::trace!(
        status = tracing::field::display(release_metadata_post_response_status),
        "Got release metadata POST response"
    );

    match release_metadata_post_response_status {
        StatusCode::OK => (),
        StatusCode::CONFLICT => {
            tracing::info!(
                "Release for revision `{revision}` of {upload_name}/{rolling_prefix_or_tag} already exists; flakehub-push will not upload it again",
                revision = release_metadata.revision
            );
            if error_if_release_conflicts {
                return Err(Error::Conflict {
                    upload_name,
                    rolling_prefix_or_tag,
                })?;
            } else {
                return Ok(());
            }
        }
        StatusCode::UNAUTHORIZED => {
            let body = &release_metadata_post_response.bytes().await?;
            let message = serde_json::from_slice::<String>(body)?;

            return Err(Error::Unauthorized(message))?;
        }
        _ => {
            let body = &release_metadata_post_response.bytes().await?;
            let message = serde_json::from_slice::<String>(body)?;
            return Err(eyre!(
                "\
                Status {release_metadata_post_response_status} from metadata POST\n\
                {}\
            ",
                message
            ));
        }
    }

    #[derive(serde::Deserialize)]
    struct Result {
        s3_upload_url: String,
        uuid: Uuid,
    }

    let release_metadata_post_result: Result = release_metadata_post_response
        .json()
        .await
        .wrap_err("Decoding release metadata POST response")?;

    let tarball_put_response = flakehub_client
        .put(release_metadata_post_result.s3_upload_url)
        .headers({
            let mut header_map = HeaderMap::new();
            header_map.insert(
                reqwest::header::CONTENT_LENGTH,
                reqwest::header::HeaderValue::from_str(&format!("{}", flake_tarball_len)).unwrap(),
            );
            header_map.insert(
                reqwest::header::HeaderName::from_static("x-amz-checksum-sha256"),
                reqwest::header::HeaderValue::from_str(&flake_tarball_hash_base64).unwrap(),
            );
            header_map.insert(
                reqwest::header::CONTENT_TYPE,
                reqwest::header::HeaderValue::from_str("application/gzip").unwrap(),
            );
            header_map
        })
        .body(flake_tarball)
        .send()
        .await
        .wrap_err("Sending tarball PUT")?;

    let tarball_put_response_status = tarball_put_response.status();
    tracing::trace!(
        status = tracing::field::display(release_metadata_post_response_status),
        "Got tarball PUT response"
    );
    if !tarball_put_response_status.is_success() {
        return Err(eyre!(
            "Got {tarball_put_response_status} status from PUT request"
        ));
    }

    // Make the release we just uploaded visible.
    let publish_post_url = format!("{host}/publish/{}", release_metadata_post_result.uuid);
    tracing::debug!(url = publish_post_url, "Computed publish POST URL");

    let publish_response = flakehub_client
        .post(publish_post_url)
        .bearer_auth(upload_bearer_token)
        .headers(flakehub_headers)
        .send()
        .await
        .wrap_err("Publishing release")?;

    let publish_response_status = publish_response.status();
    tracing::trace!(
        status = tracing::field::display(publish_response_status),
        "Got publish POST response"
    );

    if publish_response_status != 200 {
        return Err(eyre!(
            "\
                Status {publish_response_status} from publish POST\n\
                {}\
            ",
            String::from_utf8_lossy(&publish_response.bytes().await.unwrap())
        ));
    }

    tracing::info!(
        "Successfully released new version of {upload_name}/{rolling_minor_with_postfix_or_tag}"
    );

    Ok(())
}

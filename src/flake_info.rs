use std::{io::Write, path::Path};

use color_eyre::eyre::{eyre, WrapErr};
use tokio::io::AsyncWriteExt;

// The UUID embedded in our flake that we'll replace with the flake URL of the flake we're trying to
// get outputs from.
const FLAKE_URL_PLACEHOLDER_UUID: &str = "c9026fc0-ced9-48e0-aa3c-fc86c4c86df1";

#[tracing::instrument(
    skip_all,
    fields(
        directory = %directory.display(),
    )
)]
pub(crate) async fn get_flake_tarball(
    directory: &Path,
    last_modified: u64,
) -> color_eyre::Result<Vec<u8>> {
    let mut tarball_builder = tar::Builder::new(vec![]);
    tarball_builder.follow_symlinks(false);
    tarball_builder.force_mtime(last_modified);

    tracing::trace!("Creating tarball");
    // `tar` works according to the current directory (yay)
    // So we change dir and restory it after
    // TODO: Fix this
    let current_dir = std::env::current_dir().wrap_err("Could not get current directory")?;
    std::env::set_current_dir(
        directory
            .parent()
            .ok_or_else(|| eyre!("Getting parent directory"))?,
    )?;
    let dirname = directory
        .file_name()
        .ok_or_else(|| eyre!("No file name of directory"))?;
    tarball_builder
        .append_dir_all(dirname, dirname)
        .wrap_err_with(|| eyre!("Adding `{}` to tarball", directory.display()))?;
    std::env::set_current_dir(current_dir).wrap_err("Could not set current directory")?;

    let tarball = tarball_builder.into_inner().wrap_err("Creating tarball")?;
    tracing::trace!("Created tarball, compressing...");
    let mut gzip_encoder = flate2::write::GzEncoder::new(vec![], flate2::Compression::default());
    gzip_encoder
        .write_all(&tarball[..])
        .wrap_err("Adding tarball to gzip")?;
    let compressed_tarball = gzip_encoder.finish().wrap_err("Creating gzip")?;
    tracing::trace!("Compressed tarball");

    Ok(compressed_tarball)
}

#[tracing::instrument(
    skip_all,
    fields(
        directory = %directory.display(),
    )
)]
pub(crate) async fn check_flake_evaluates(directory: &Path) -> color_eyre::Result<()> {
    let output = tokio::process::Command::new("nix")
        .arg("flake")
        .arg("show")
        .arg("--all-systems")
        .arg("--json")
        .arg("--no-write-lock-file")
        .arg(directory)
        .output()
        .await
        .wrap_err_with(|| {
            eyre!(
                "Failed to execute `nix flake show --all-systems --json --no-write-lock-file {}`",
                directory.display()
            )
        })?;

    if !output.status.success() {
        let command = format!(
            "nix flake show --all-systems --json --no-write-lock-file {}",
            directory.display(),
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

    Ok(())
}

#[tracing::instrument(
    skip_all,
    fields(
        directory = %directory.display(),
    )
)]
pub(crate) async fn get_flake_metadata(directory: &Path) -> color_eyre::Result<serde_json::Value> {
    let output = tokio::process::Command::new("nix")
        .arg("flake")
        .arg("metadata")
        .arg("--json")
        .arg("--no-write-lock-file")
        .arg(directory)
        .output()
        .await
        .wrap_err_with(|| {
            eyre!(
                "Failed to execute `nix flake metadata --json {}`",
                directory.display()
            )
        })?;

    let output_json = serde_json::from_slice(&output.stdout).wrap_err_with(|| {
        eyre!(
            "Parsing `nix flake metadata --json {}` as JSON",
            directory.display()
        )
    })?;

    Ok(output_json)
}

#[tracing::instrument(skip_all, fields(flake_url,))]
pub(crate) async fn get_flake_outputs(flake_url: &str) -> color_eyre::Result<serde_json::Value> {
    let tempdir = tempfile::Builder::new()
        .prefix("flakehub_push_outputs")
        .tempdir()
        .wrap_err("Creating tempdir")?;

    let flake_contents = include_str!("mixed-flake.nix").replace(
        FLAKE_URL_PLACEHOLDER_UUID,
        &flake_url.escape_default().to_string(),
    );

    let mut flake = tokio::fs::File::create(tempdir.path().join("flake.nix")).await?;
    flake.write_all(flake_contents.as_bytes()).await?;

    let mut cmd = tokio::process::Command::new("nix");
    cmd.arg("eval");
    cmd.arg("--json");
    cmd.arg("--no-write-lock-file");
    cmd.arg(format!("{}#contents", tempdir.path().display()));
    let output = cmd
        .output()
        .await
        .wrap_err_with(|| eyre!("Failed to get flake outputs from tarball {}", flake_url))?;

    if !output.status.success() {
        return Err(eyre!(
            "Failed to get flake outputs from tarball {}: {}",
            flake_url,
            String::from_utf8(output.stderr).unwrap()
        ));
    }

    let output_json = serde_json::from_slice(&output.stdout).wrap_err_with(|| {
        eyre!(
            "Parsing flake outputs from {} as JSON: {}",
            flake_url,
            String::from_utf8(output.stdout).unwrap(),
        )
    })?;

    Ok(output_json)
}

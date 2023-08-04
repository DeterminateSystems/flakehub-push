use color_eyre::eyre::{eyre, WrapErr};
use std::{io::Write, path::Path};

#[tracing::instrument(
    skip_all,
    fields(
        directory = %directory.display(),
    )
)]
pub(crate) async fn get_flake_tarball(directory: &Path) -> color_eyre::Result<Vec<u8>> {
    let mut tarball_builder = tar::Builder::new(vec![]);

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

// FIXME: make this configurable
pub(crate) const FLAKE_SCHEMAS_LOCKED_URL: &str =
    "https://nxfr.com/f/pinned/DeterminateSystems/flake-schemas/0.0.5%252Brev-92d8d7803fe5f3a3810a3cceb02fa6a4b65f15a6/0189c11e-9bb8-7bc5-a4fb-2df59ad36d55/source.tar.gz?narHash=sha256-Cv74iWkgDQeTiW3YKmvYC2RBoo4u133V73HZ%2BJnovVk%3D";

#[tracing::instrument(skip_all, fields(flake_url,))]
pub(crate) async fn get_flake_outputs(flake_url: &str) -> color_eyre::Result<serde_json::Value> {
    let output = tokio::process::Command::new("nix")
        .arg("eval")
        .arg("--json")
        .arg("--no-write-lock-file")
        .arg("--expr")
        .arg(format!(
            "((builtins.getFlake \"{}\").lib.evalFlake (builtins.getFlake \"{}\")).contents",
            FLAKE_SCHEMAS_LOCKED_URL, &flake_url
        ))
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

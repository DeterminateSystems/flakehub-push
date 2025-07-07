use std::{
    io::Write,
    path::{Path, PathBuf},
};

use color_eyre::eyre::{eyre, Result, WrapErr};
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

use crate::flakehub_client::Tarball;

// The UUID embedded in our flake that we'll replace with the flake URL of the flake we're trying to
// get outputs from.
const FLAKE_URL_PLACEHOLDER_UUID: &str = "c9026fc0-ced9-48e0-aa3c-fc86c4c86df1";
const README_FILENAME_LOWERCASE: &str = "readme.md";

#[derive(Debug)]
pub struct FlakeMetadata {
    pub(crate) source_dir: std::path::PathBuf,
    pub(crate) flake_locked_url: String,
    pub(crate) metadata_json: serde_json::Value,
    my_flake_is_too_big: bool,
}

#[derive(Debug, Deserialize)]
pub struct FlakeOutputs(pub serde_json::Value);

impl FlakeMetadata {
    pub async fn from_dir(directory: &Path, my_flake_is_too_big: bool) -> Result<Self> {
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

        let metadata_json: serde_json::Value = serde_json::from_slice(&output.stdout)
            .wrap_err_with(|| {
                eyre!(
                    "Parsing `nix flake metadata --json {}` as JSON",
                    directory.display()
                )
            })?;

        let flake_locked_url = metadata_json
            .get("url")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                eyre!("Could not get `url` attribute from `nix flake metadata --json` output")
            })?;
        tracing::debug!("Locked URL = {}", flake_locked_url);
        let flake_metadata_value_resolved_dir = metadata_json
            .pointer("/resolved/dir")
            .and_then(serde_json::Value::as_str);

        let output = tokio::process::Command::new("nix")
            .arg("flake")
            .arg("prefetch")
            .arg("--json")
            .arg("--no-write-lock-file")
            .arg(directory)
            .output()
            .await
            .wrap_err_with(|| {
                eyre!(
                    "Failed to execute `nix flake prefetch --json {}`",
                    directory.display()
                )
            })?;

        let prefetch_json: serde_json::Value = serde_json::from_slice(&output.stdout)
            .wrap_err_with(|| {
                eyre!(
                    "Parsing `nix flake prefetch --json {}` as JSON",
                    directory.display()
                )
            })?;

        let flake_prefetch_value_path = prefetch_json
            .get("storePath")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                eyre!("Could not get `storePath` attribute from `nix flake prefetch --json` output")
            })?;

        let source = match flake_metadata_value_resolved_dir {
            Some(flake_metadata_value_resolved_dir) => {
                Path::new(flake_prefetch_value_path).join(flake_metadata_value_resolved_dir)
            }
            None => PathBuf::from(flake_prefetch_value_path),
        };

        Ok(FlakeMetadata {
            source_dir: source,
            flake_locked_url: flake_locked_url.to_string(),
            metadata_json,
            my_flake_is_too_big,
        })
    }

    /// check_evalutes checks that the flake evaluates
    /// (note it is not necessary for the target to have a flake.lock)
    pub async fn check_evaluates(&self) -> Result<()> {
        let mut command = tokio::process::Command::new("nix");
        command.arg("flake");
        command.arg("show");

        if !self.my_flake_is_too_big {
            command.arg("--all-systems");
        }

        command.arg("--json");
        command.arg("--no-write-lock-file");
        command.arg(&self.source_dir);

        let output = command.output().await.wrap_err_with(|| {
            eyre!(
                "Failed to execute `nix flake show --all-systems --json --no-write-lock-file {}`",
                self.source_dir.display()
            )
        })?;

        if !output.status.success() {
            let command = format!(
                "nix flake show --all-systems --json --no-write-lock-file {}",
                self.source_dir.display(),
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

    /// check_lock_if_exists is specifically to check locked flakes to make sure the flake.lock
    /// has not "drifted" from flake.nix. This would happen if the user added a new flake.nix input,
    /// and committed/pushed that without the corresponding update to the flake.lock. Importantly,
    /// this does not ensure anything about the recentness of the locked revs.
    pub async fn check_lock_if_exists(&self) -> Result<()> {
        if self.source_dir.join("flake.lock").exists() {
            let output = tokio::process::Command::new("nix")
                .arg("flake")
                .arg("metadata")
                .arg("--json")
                .arg("--no-update-lock-file")
                .arg(&self.source_dir)
                .output()
                .await
                .wrap_err_with(|| {
                    eyre!(
                        "Failed to execute `nix flake metadata --json --no-update-lock-file {}`",
                        self.source_dir.display()
                    )
                })?;

            if !output.status.success() {
                let command = format!(
                    "nix flake metadata --json --no-update-lock-file {}",
                    self.source_dir.display(),
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
        Ok(())
    }

    pub fn flake_tarball(&self) -> Result<Tarball> {
        let last_modified = if let Some(last_modified) = self.metadata_json.get("lastModified") {
            last_modified.as_u64().ok_or_else(|| {
                eyre!("`nix flake metadata --json` does not have a integer `lastModified` field")
            })?
        } else {
            return Err(eyre!(
                "`nix flake metadata` did not return a `lastModified` attribute"
            ));
        };
        tracing::debug!("lastModified = {}", last_modified);

        let mut tarball_builder = tar::Builder::new(vec![]);
        tarball_builder.follow_symlinks(false);
        tarball_builder.force_mtime(last_modified);

        tracing::trace!("Creating tarball");
        // `tar` works according to the current directory (yay)
        // So we change dir and restory it after
        // TODO: Fix this
        let source = &self.source_dir; // refactor to be known when we create struct with from_dir
        let current_dir = std::env::current_dir().wrap_err("Could not get current directory")?;
        std::env::set_current_dir(
            source
                .parent()
                .ok_or_else(|| eyre!("Getting parent directory"))?,
        )?;
        let dirname = self
            .source_dir
            .file_name()
            .ok_or_else(|| eyre!("No file name of directory"))?;
        tarball_builder
            .append_dir_all(dirname, dirname)
            .wrap_err_with(|| eyre!("Adding `{}` to tarball", self.source_dir.display()))?;
        std::env::set_current_dir(current_dir).wrap_err("Could not set current directory")?;

        let tarball = tarball_builder.into_inner().wrap_err("Creating tarball")?;
        tracing::trace!("Created tarball, compressing...");
        let mut gzip_encoder =
            flate2::write::GzEncoder::new(vec![], flate2::Compression::default());
        gzip_encoder
            .write_all(&tarball[..])
            .wrap_err("Adding tarball to gzip")?;
        let compressed_tarball = gzip_encoder.finish().wrap_err("Creating gzip")?;
        tracing::trace!("Compressed tarball");

        let flake_tarball_hash = {
            let mut context = ring::digest::Context::new(&ring::digest::SHA256);
            context.update(&compressed_tarball);
            context.finish()
        };
        let flake_tarball_hash_base64 = {
            // TODO: Use URL_SAFE_NO_PAD
            use base64::{engine::general_purpose::STANDARD, Engine as _};
            STANDARD.encode(flake_tarball_hash)
        };

        let tarball = Tarball {
            bytes: compressed_tarball,
            hash_base64: flake_tarball_hash_base64,
        };

        Ok(tarball)
    }

    pub async fn outputs(&self, include_output_paths: bool) -> Result<FlakeOutputs> {
        if self.my_flake_is_too_big {
            return Ok(FlakeOutputs(serde_json::json!({})));
        }

        let tempdir = tempfile::Builder::new()
            .prefix("flakehub_push_outputs")
            .tempdir()
            .wrap_err("Creating tempdir")?;
        // NOTE(cole-h): Work around the fact that macOS's /tmp is a symlink to /private/tmp.
        // Otherwise, Nix is unhappy:
        // error:
        //        … while fetching the input 'path:/tmp/nix-shell.q1H8OB/flakehub_push_outputsfG1YvC'
        //
        //        error: path '/tmp' is a symlink
        let tempdir_path = tempdir.path().canonicalize()?;

        let flake_contents = include_str!("flake-contents/flake.nix")
            .replace(
                FLAKE_URL_PLACEHOLDER_UUID,
                &self.flake_locked_url.escape_default().to_string(),
            )
            .replace(
                "INCLUDE_OUTPUT_PATHS",
                if include_output_paths {
                    "true"
                } else {
                    "false"
                },
            );

        let mut flake = tokio::fs::File::create(tempdir_path.join("flake.nix")).await?;
        flake.write_all(flake_contents.as_bytes()).await?;

        let mut cmd = tokio::process::Command::new("nix");
        cmd.arg("eval");
        cmd.arg("--json");
        cmd.arg("--no-write-lock-file");
        cmd.arg(format!("{}#contents", tempdir_path.display()));
        let output = cmd.output().await.wrap_err_with(|| {
            eyre!(
                "Failed to get flake outputs from tarball {}",
                &self.flake_locked_url
            )
        })?;

        if !output.status.success() {
            return Err(eyre!(
                "Failed to get flake outputs from tarball {}: {}",
                &self.flake_locked_url,
                String::from_utf8(output.stderr).unwrap()
            ));
        }

        let output_json = serde_json::from_slice(&output.stdout).wrap_err_with(|| {
            eyre!(
                "Parsing flake outputs from {} as JSON: {}",
                &self.flake_locked_url,
                String::from_utf8(output.stdout).unwrap(),
            )
        })?;

        Ok(output_json)
    }

    #[tracing::instrument(skip_all, fields(readme_dir))]
    pub(crate) async fn get_readme_contents(&self) -> Result<Option<String>> {
        let mut read_dir = tokio::fs::read_dir(&self.source_dir).await?;

        let readme_path: Option<PathBuf> = {
            let mut readme_path = None;
            while let Some(entry) = read_dir.next_entry().await? {
                if entry.file_name().to_ascii_lowercase() == README_FILENAME_LOWERCASE {
                    readme_path = Some(entry.path());
                }
            }
            readme_path
        };
        let readme = if let Some(readme_path) = readme_path {
            Some(tokio::fs::read_to_string(&readme_path).await?)
        } else {
            None
        };
        Ok(readme)
    }
}

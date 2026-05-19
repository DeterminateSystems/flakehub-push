use std::path::{Path, PathBuf};

use color_eyre::eyre::{eyre, Result, WrapErr};
use flake_schemas::{InspectOptions, InspectOutput};

use crate::flakehub_client::Tarball;

const README_FILENAME_LOWERCASE: &str = "readme.md";

#[derive(Debug)]
pub struct FlakeMetadata {
    pub(crate) source_dir: std::path::PathBuf,
    pub(crate) flake_locked_url: String,
    pub(crate) metadata_json: serde_json::Value,
    my_flake_is_too_big: bool,
}

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

        let output = flate2::write::GzEncoder::new(vec![], flate2::Compression::default());
        let output = std::io::BufWriter::new(output);
        let mut tarball_builder = tar::Builder::new(output);
        tarball_builder.follow_symlinks(false);

        let source = &self.source_dir; // refactor to be known when we create struct with from_dir
        let parent = source
            .parent()
            .ok_or_else(|| eyre!("Source dir had no parent, cannot continue"))?;

        tracing::trace!("Creating compressed tarball");
        for entry in walkdir::WalkDir::new(source).sort_by_file_name() {
            let entry = entry?;
            let path = entry.path();
            let subpath = path.strip_prefix(parent)?;

            let metadata = path.symlink_metadata()?;

            let mut header = tar::Header::new_gnu();
            header.set_metadata_in_mode(&metadata, tar::HeaderMode::Deterministic);
            header.set_mtime(last_modified);
            header.set_uid(0);
            header.set_gid(0);

            if metadata.is_dir() {
                tarball_builder.append_data(&mut header, subpath, std::io::Cursor::new([]))?;
            } else if metadata.is_file() {
                let src = std::fs::File::open(path).map(std::io::BufReader::new)?;
                tarball_builder.append_data(&mut header, subpath, src)?;
            } else if metadata.is_symlink() {
                let target = path.read_link()?;
                tarball_builder.append_link(&mut header, subpath, target)?;
            } else {
                tracing::warn!(?path, "Ignoring unexpected special file");
                continue;
            }
        }

        let tarball = tarball_builder.into_inner().wrap_err("Creating tarball")?;
        tracing::trace!("Created tarball, finishing compression...");
        let compressed_tarball = tarball
            .into_inner()
            .wrap_err("Creating gzip")?
            .finish()
            .wrap_err("Finalizing compression")?;
        tracing::trace!("Finished tarball");

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

    pub async fn outputs(&self, include_output_paths: bool) -> Result<InspectOutput> {
        if self.my_flake_is_too_big {
            return Ok(InspectOutput::new());
        }

        let options = InspectOptions::new().with_output(include_output_paths);

        flake_schemas::inspect_with_options(&self.flake_locked_url, &options)
            .wrap_err_with(|| eyre!("Parsing flake outputs from {}", self.flake_locked_url))
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

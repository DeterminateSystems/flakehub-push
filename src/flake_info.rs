use std::{io::Write, path::{Path, PathBuf}};

use color_eyre::eyre::{eyre, Result, WrapErr};
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

use crate::flakehub_client::Tarball;

// The UUID embedded in our flake that we'll replace with the flake URL of the flake we're trying to
// get outputs from.
const FLAKE_URL_PLACEHOLDER_UUID: &str = "c9026fc0-ced9-48e0-aa3c-fc86c4c86df1";
const README_FILENAME_LOWERCASE: &str = "readme.md";

// TODO: can't we just do this sanity checking in from_dir?
// // TODO(colemickens): can we move this to a method on FlakeMetadata?
// // maybe FlakeMetadata should know its dir? probably inside the json already
// #[tracing::instrument(
//     skip_all,
//     fields(
//         directory = %directory.display(),
//     )
// )]
// pub(crate) async fn check_flake_evaluates(directory: &Path) -> color_eyre::Result<()> {
//     let output = tokio::process::Command::new("nix")
//         .arg("flake")
//         .arg("show")
//         .arg("--all-systems")
//         .arg("--json")
//         .arg("--no-write-lock-file")
//         .arg(directory)
//         .output()
//         .await
//         .wrap_err_with(|| {
//             eyre!(
//                 "Failed to execute `nix flake show --all-systems --json --no-write-lock-file {}`",
//                 directory.display()
//             )
//         })?;

//     if !output.status.success() {
//         let command = format!(
//             "nix flake show --all-systems --json --no-write-lock-file {}",
//             directory.display(),
//         );
//         let msg = format!(
//             "\
//             Failed to execute command `{command}`{maybe_status} \n\
//             stdout: {stdout}\n\
//             stderr: {stderr}\n\
//             ",
//             stdout = String::from_utf8_lossy(&output.stdout),
//             stderr = String::from_utf8_lossy(&output.stderr),
//             maybe_status = if let Some(status) = output.status.code() {
//                 format!(" with status {status}")
//             } else {
//                 String::new()
//             }
//         );
//         return Err(eyre!(msg))?;
//     }

//     Ok(())
// }


#[derive(Debug)]
pub struct FlakeMetadata {
    pub(crate) source_dir: std::path::PathBuf,
    pub(crate) flake_locked_url: String,
    pub(crate) metadata_json: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct FlakeOutputs(pub serde_json::Value);

impl FlakeMetadata {
    pub async fn from_dir(directory: &Path) -> Result<Self> {
        // TODO(colemickens): the de-duped block below runs `--no-update-lock-file`, this one is `--no-write-lock-file`
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

        let metadata_json: serde_json::Value = serde_json::from_slice(&output.stdout).wrap_err_with(|| {
            eyre!(
                "Parsing `nix flake metadata --json {}` as JSON",
                directory.display()
            )
        })?;

        /*
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
        */

        // determine flake's store (sub)dir:
        
        let flake_locked_url = metadata_json
            .get("url")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                eyre!("Could not get `url` attribute from `nix flake metadata --json` output")
            })?;
        tracing::debug!("Locked URL = {}", flake_locked_url);
        let flake_metadata_value_path = metadata_json
            .get("path")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                eyre!("Could not get `path` attribute from `nix flake metadata --json` output")
            })?;
        let flake_metadata_value_resolved_dir = metadata_json
            .pointer("/resolved/dir")
            .and_then(serde_json::Value::as_str);

        let source = match flake_metadata_value_resolved_dir {
            Some(flake_metadata_value_resolved_dir) => {
                Path::new(flake_metadata_value_path).join(flake_metadata_value_resolved_dir)
            }
            None => PathBuf::from(flake_metadata_value_path),
        };

        Ok(FlakeMetadata {
            source_dir: source,
            flake_locked_url: flake_locked_url.to_string(),
            metadata_json: metadata_json,
            // TODO(Colemickens): remove this, we want to use get_source()
            //dir: directory,
            // move the source determination to from_dir so we just know it as a property
            // rename 'dir' to 'source'
        })
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
        let source = self.source_dir; // refactor to be known when we create struct with from_dir
        let current_dir = std::env::current_dir().wrap_err("Could not get current directory")?;
        std::env::set_current_dir(
            source
                .parent()
                .ok_or_else(|| eyre!("Getting parent directory"))?,
        )?;
        let dirname = self.source_dir
            .file_name()
            .ok_or_else(|| eyre!("No file name of directory"))?;
        tarball_builder
            .append_dir_all(dirname, dirname)
            .wrap_err_with(|| eyre!("Adding `{}` to tarball", self.source_dir.display()))?;
        std::env::set_current_dir(current_dir).wrap_err("Could not set current directory")?;
    
        let tarball = tarball_builder.into_inner().wrap_err("Creating tarball")?;
        tracing::trace!("Created tarball, compressing...");
        let mut gzip_encoder = flate2::write::GzEncoder::new(vec![], flate2::Compression::default());
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
            hash_base64: flake_tarball_hash_base64
        };
    
        Ok(tarball)
    }

    pub async fn outputs(&self, include_output_paths: bool) -> Result<FlakeOutputs> {
        let tempdir = tempfile::Builder::new()
            .prefix("flakehub_push_outputs")
            .tempdir()
            .wrap_err("Creating tempdir")?;

        let flake_contents = include_str!("mixed-flake.nix")
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
            .wrap_err_with(|| eyre!("Failed to get flake outputs from tarball {}", &self.flake_locked_url))?;

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
            while let Some(entry) = read_dir.next_entry().await? {
                if entry.file_name().to_ascii_lowercase() == README_FILENAME_LOWERCASE {
                    Some(Some(entry.path()));
                }
            }
            None
        };
        let readme = if let Some(readme_path) = readme_path {
            Some(tokio::fs::read_to_string(&readme_path).await?)
        } else {
            None
        };
        Ok(readme)
    }
}

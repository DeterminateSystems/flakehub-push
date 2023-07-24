use color_eyre::eyre::{eyre, WrapErr};
use std::path::Path;

use crate::Visibility;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ReleaseMetadata {
    pub(crate) commit_count: usize,
    pub(crate) description: Option<String>,
    pub(crate) outputs: serde_json::Value,
    pub(crate) raw_flake_metadata: serde_json::Value,
    pub(crate) readme: Option<String>,
    pub(crate) repo: String,
    pub(crate) revision: String,
    pub(crate) visibility: Visibility,
    pub(crate) mirrored: bool,
}

impl ReleaseMetadata {
    #[tracing::instrument(skip_all, fields(
        directory = %directory.display(),
        description = tracing::field::Empty,
        readme_path = tracing::field::Empty,
        revision = tracing::field::Empty,
        revision_count = tracing::field::Empty,
        commit_count = tracing::field::Empty,
        visibility = ?visibility,
    ))]
    pub(crate) async fn build(
        reqwest_client: reqwest::Client,
        directory: &Path,
        git_root: &Path,
        flake_metadata: serde_json::Value,
        flake_outputs: serde_json::Value,
        project_owner: &str,
        project_name: &str,
        mirrored: bool,
        visibility: Visibility,
    ) -> color_eyre::Result<ReleaseMetadata> {
        let span = tracing::Span::current();
        let gix_repository = gix::open(git_root).wrap_err("Opening the Git repository")?;
        let gix_repository_head = gix_repository
            .head()
            .wrap_err("Getting the HEAD revision of the repository")?;

        let revision = match gix_repository_head.kind {
            gix::head::Kind::Symbolic(gix_ref::Reference { name: _, target, .. }) => {
                match target {
                    gix_ref::Target::Peeled(object_id) => object_id,
                    gix_ref::Target::Symbolic(_) => return Err(eyre!("Recieved a symbolic Git revision pointing to a symbolic Git revision, this is not supported at this time"))
                }
            }
            gix::head::Kind::Detached {
                target: object_id, ..
            } => object_id,
            gix::head::Kind::Unborn(_) => {
                return Err(eyre!(
                    "Newly initialized repository detected, at least one commit is necessary"
                ))
            }
        };

        let revision_string = revision.to_hex().to_string();
        span.record("revision_string", revision_string.clone());

        let revision_count = gix_repository.rev_walk([revision]).all()?.count();
        span.record("revision_count", revision_count);

        let description = if let Some(description) = flake_metadata.get("description") {
            Some(description
                .as_str()
                .ok_or_else(|| {
                    eyre!("`nix flake metadata --json` does not have a string `description` field")
                })?
                .to_string())
        } else {
            None
        };

        let readme_path = directory.join("README.md");
        let readme = if readme_path.exists() {
            Some(tokio::fs::read_to_string(readme_path).await?)
        } else {
            None
        };

        Ok(ReleaseMetadata {
            description,
            repo: format!("{project_owner}/{project_name}"),
            raw_flake_metadata: flake_metadata.clone(),
            readme,
            revision: revision_string,
            commit_count: revision_count,
            visibility,
            outputs: flake_outputs,
            mirrored,
        })
    }
}

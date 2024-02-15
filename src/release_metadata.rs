use color_eyre::eyre::{eyre, WrapErr};
use std::path::Path;

use crate::Visibility;

const README_FILENAME_LOWERCASE: &str = "readme.md";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ReleaseMetadata {
    pub(crate) commit_count: i64,
    pub(crate) description: Option<String>,
    pub(crate) outputs: serde_json::Value,
    pub(crate) raw_flake_metadata: serde_json::Value,
    pub(crate) readme: Option<String>,
    pub(crate) repo: String,
    pub(crate) revision: String,
    pub(crate) visibility: Visibility,
    pub(crate) mirrored: bool,
    pub(crate) source_subdirectory: Option<String>,
    pub(crate) project_id: i64,
    pub(crate) owner_id: i64,

    #[serde(
        deserialize_with = "option_string_to_spdx",
        serialize_with = "option_spdx_serialize"
    )]
    pub(crate) spdx_identifier: Option<spdx::Expression>,

    // A result of combining the labels specified on the CLI via the the GitHub Actions config
    // and the labels associated with the GitHub repo (they're called "topics" in GitHub parlance).
    pub(crate) labels: Vec<String>,
}

#[derive(Clone)]
pub(crate) struct RevisionInfo {
    pub(crate) local_revision_count: Option<usize>,
    pub(crate) revision: String,
}

impl RevisionInfo {
    pub(crate) fn from_git_root(git_root: &Path) -> color_eyre::Result<Self> {
        let gix_repository = gix::open(git_root).wrap_err("Opening the Git repository")?;
        let gix_repository_head = gix_repository
            .head()
            .wrap_err("Getting the HEAD revision of the repository")?;

        let revision = match gix_repository_head.kind {
            gix::head::Kind::Symbolic(gix_ref::Reference {
                name: _, target, ..
            }) => match target {
                gix_ref::Target::Peeled(object_id) => object_id,
                gix_ref::Target::Symbolic(_) => {
                    return Err(eyre!(
                "Symbolic revision pointing to a symbolic revision is not supported at this time"
            ))
                }
            },
            gix::head::Kind::Detached {
                target: object_id, ..
            } => object_id,
            gix::head::Kind::Unborn(_) => {
                return Err(eyre!(
                    "Newly initialized repository detected, at least one commit is necessary"
                ))
            }
        };

        let local_revision_count = gix_repository
            .rev_walk([revision])
            .all()
            .map(|rev_iter| rev_iter.count())
            .ok();
        let revision = revision.to_hex().to_string();

        Ok(Self {
            local_revision_count,
            revision,
        })
    }
}

impl ReleaseMetadata {
    // FIXME
    #[allow(clippy::too_many_arguments)]
    #[tracing::instrument(skip_all, fields(
        flake_store_path = %flake_store_path.display(),
        subdir = %subdir.display(),
        description = tracing::field::Empty,
        readme_path = tracing::field::Empty,
        revision = revision_info.revision,
        %revision_count,
        spdx_identifier = tracing::field::Empty,
        visibility = ?visibility,
        %project_id,
        %owner_id
    ))]
    pub(crate) async fn build(
        flake_store_path: &Path,
        subdir: &Path,
        revision_info: RevisionInfo,
        flake_metadata: serde_json::Value,
        flake_outputs: serde_json::Value,
        upload_name: String,
        mirror: bool,
        visibility: Visibility,
        revision_count: i64,
        labels: Vec<String>,
        spdx_identifier: Option<spdx::Expression>,
        project_id: i64,
        owner_id: i64,
    ) -> color_eyre::Result<ReleaseMetadata> {
        let span = tracing::Span::current();

        if let Some(spdx_identifier) = &spdx_identifier {
            span.record("spdx_identifier", tracing::field::display(spdx_identifier));
        }

        assert!(subdir.is_relative());

        let description = if let Some(description) = flake_metadata.get("description") {
            let description_value = description
                .as_str()
                .ok_or_else(|| {
                    eyre!("`nix flake metadata --json` does not have a string `description` field")
                })?
                .to_string();
            span.record("description", tracing::field::display(&description_value));
            Some(description_value)
        } else {
            None
        };

        let readme_path = get_readme(flake_store_path).await?;
        if let Some(readme_path) = &readme_path {
            span.record("readme_path", tracing::field::display(readme_path));
        }

        tracing::trace!("Collected ReleaseMetadata information");

        Ok(ReleaseMetadata {
            description,
            repo: upload_name.to_string(),
            raw_flake_metadata: flake_metadata.clone(),
            readme: readme_path,
            revision: revision_info.revision,
            commit_count: revision_count,
            visibility,
            outputs: flake_outputs,
            source_subdirectory: Some(
                subdir
                    .to_str()
                    .map(|d| d.to_string())
                    .ok_or(eyre!("Directory {:?} is not a valid UTF-8 string", subdir))?,
            ),
            mirrored: mirror,
            spdx_identifier,
            project_id: project_id,
            owner_id: owner_id,
            labels,
        })
    }
}

fn option_string_to_spdx<'de, D>(deserializer: D) -> Result<Option<spdx::Expression>, D::Error>
where
    D: serde::de::Deserializer<'de>,
{
    let spdx_identifier: Option<&str> = serde::Deserialize::deserialize(deserializer)?;

    if let Some(spdx_identifier) = spdx_identifier {
        spdx::Expression::parse(spdx_identifier)
            .map_err(serde::de::Error::custom)
            .map(Option::Some)
    } else {
        Ok(None)
    }
}

fn option_spdx_serialize<S>(
    spdx_identifier: &Option<spdx::Expression>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::ser::Serializer,
{
    if let Some(spdx_identifier) = spdx_identifier {
        let spdx_string = spdx_identifier.to_string();
        serializer.serialize_str(&spdx_string)
    } else {
        serializer.serialize_none()
    }
}

async fn get_readme(readme_dir: &Path) -> color_eyre::Result<Option<String>> {
    let mut read_dir = tokio::fs::read_dir(readme_dir).await?;

    while let Some(entry) = read_dir.next_entry().await? {
        if entry.file_name().to_ascii_lowercase() == README_FILENAME_LOWERCASE {
            return Ok(Some(tokio::fs::read_to_string(entry.path()).await?));
        }
    }

    Ok(None)
}

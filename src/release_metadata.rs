use color_eyre::eyre::{eyre, WrapErr};
use std::{collections::HashSet, path::Path};

use crate::{
    graphql::{GithubGraphqlDataResult, MAX_NUM_TOTAL_TAGS, MAX_TAG_LENGTH},
    Visibility,
};

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

    // A result of combining the tags specified on the CLI via the the GitHub Actions config
    // and the tags associated with the GitHub repo (they're called "topics" in GitHub parlance).
    pub(crate) tags: Vec<String>,
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
    #[tracing::instrument(skip_all, fields(
        flake_root = %flake_root.display(),
        subdir = %subdir.display(),
        description = tracing::field::Empty,
        readme_path = tracing::field::Empty,
        revision = tracing::field::Empty,
        revision_count = tracing::field::Empty,
        commit_count = tracing::field::Empty,
        spdx_identifier = tracing::field::Empty,
        visibility = ?visibility,
    ))]
    pub(crate) async fn build(
        flake_root: &Path,
        subdir: &Path,
        revision_info: RevisionInfo,
        flake_metadata: serde_json::Value,
        flake_outputs: serde_json::Value,
        upload_name: String,
        mirror: bool,
        visibility: Visibility,
        github_graphql_data_result: GithubGraphqlDataResult,
        extra_tags: Vec<String>,
        spdx_expression: Option<spdx::Expression>,
    ) -> color_eyre::Result<ReleaseMetadata> {
        let span = tracing::Span::current();

        span.record("revision_string", &revision_info.revision);

        assert!(subdir.is_relative());

        let revision_count = match revision_info.local_revision_count {
            Some(n) => n as i64,
            None => {
                tracing::debug!(
                    "Getting revision count locally failed, using data from github instead"
                );
                github_graphql_data_result.rev_count
            }
        };
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

        let readme_path = flake_root.join(subdir).join("README.md");
        let readme = if readme_path.exists() {
            Some(tokio::fs::read_to_string(readme_path).await?)
        } else {
            None
        };

        let spdx_identifier = if spdx_expression.is_some() {
            spdx_expression
        } else if let Some(spdx_string) = github_graphql_data_result.spdx_identifier {
            let parsed = spdx::Expression::parse(&spdx_string)
                .wrap_err("Invalid SPDX license identifier reported from the GitHub API, either you are using a non-standard license or GitHub has returned a value that cannot be validated")?;
            span.record("spdx_identifier", tracing::field::display(&parsed));
            Some(parsed)
        } else {
            None
        };

        tracing::trace!("Collected ReleaseMetadata information");

        // Here we merge explicitly user-supplied tags and the tags ("topics")
        // associated with the repo. Duplicates are excluded and all
        // are converted to lower case.
        let tags: Vec<String> = extra_tags
            .into_iter()
            .chain(github_graphql_data_result.topics.into_iter())
            .collect::<HashSet<String>>()
            .into_iter()
            .take(MAX_NUM_TOTAL_TAGS)
            .map(|s| s.trim().to_lowercase())
            .filter(|t: &String| {
                !t.is_empty()
                    && t.len() <= MAX_TAG_LENGTH
                    && t.chars().all(|c| c.is_alphanumeric() || c == '-')
            })
            .collect();

        Ok(ReleaseMetadata {
            description,
            repo: upload_name.to_string(),
            raw_flake_metadata: flake_metadata.clone(),
            readme,
            revision: revision_info.revision,
            commit_count: github_graphql_data_result.rev_count,
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
            project_id: github_graphql_data_result.project_id,
            owner_id: github_graphql_data_result.owner_id,
            tags,
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

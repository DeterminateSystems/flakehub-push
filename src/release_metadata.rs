use color_eyre::eyre::{eyre, WrapErr};
use std::path::Path;

use crate::graphql::GithubGraphqlDataQuery;

use crate::Visibility;

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
    #[serde(
        deserialize_with = "option_string_to_spdx",
        serialize_with = "option_spdx_serialize"
    )]
    pub(crate) spdx_identifier: Option<spdx::Expression>,
    #[cfg(debug_assertions)]
    dev_metadata: DevMetadata,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct DevMetadata {
    pub(crate) project_id: Option<String>,
    pub(crate) owner_id: Option<String>,
}

impl ReleaseMetadata {
    #[tracing::instrument(skip_all, fields(
        directory = %directory.display(),
        description = tracing::field::Empty,
        readme_path = tracing::field::Empty,
        revision = tracing::field::Empty,
        revision_count = tracing::field::Empty,
        commit_count = tracing::field::Empty,
        spdx_identifier = tracing::field::Empty,
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
        #[cfg(debug_assertions)] dev_metadata: DevMetadata,
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

        let github_graphql_data_result = GithubGraphqlDataQuery::get(
            reqwest_client,
            project_owner,
            project_name,
            &revision_string,
        )
        .await?;
        span.record("revision_count", github_graphql_data_result.rev_count);

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

        let spdx_identifier = if let Some(spdx_string) = github_graphql_data_result.spdx_identifier
        {
            let parsed = spdx::Expression::parse(&spdx_string)
                .wrap_err("Invalid SPDX license identifier reported from the GitHub API, either you are using a non-standard license or GitHub has returned a value that cannot be validated")?;
            span.record("spdx_identifier", tracing::field::display(&parsed));
            Some(parsed)
        } else {
            None
        };

        tracing::trace!("Collected ReleaseMetadata information");

        Ok(ReleaseMetadata {
            description,
            repo: format!("{project_owner}/{project_name}"),
            raw_flake_metadata: flake_metadata.clone(),
            readme,
            revision: revision_string,
            commit_count: github_graphql_data_result.rev_count,
            visibility,
            outputs: flake_outputs,
            mirrored,
            spdx_identifier,
            #[cfg(debug_assertions)]
            dev_metadata,
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

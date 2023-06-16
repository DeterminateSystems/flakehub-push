use color_eyre::eyre::{eyre, WrapErr};
use std::path::Path;

use crate::graphql::rev_count_query::RevCountQueryRepositoryObject;
use graphql_client::GraphQLQuery;

use crate::Visibility;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ReleaseMetadata {
    pub(crate) description: String,
    pub(crate) mirrored_for: Option<String>,
    pub(crate) raw_flake_metadata: serde_json::Value,
    pub(crate) readme: String,
    pub(crate) revision: String,
    pub(crate) commit_count: i64,
    pub(crate) visibility: Visibility,
    pub(crate) outputs: serde_json::Value,
}

impl ReleaseMetadata {
    #[tracing::instrument(skip_all, fields(
        directory = %directory.display(),
        description = tracing::field::Empty,
        mirrored_for = tracing::field::Empty,
        readme_path = tracing::field::Empty,
        revision = tracing::field::Empty,
        revision_count = tracing::field::Empty,
        commit_count = tracing::field::Empty,
        visibility = ?visibility,
    ))]
    pub(crate) async fn build(
        reqwest_client: reqwest::Client,
        directory: &Path,
        flake_metadata: serde_json::Value,
        flake_outputs: serde_json::Value,
        project_owner: &str,
        project_name: &str,
        mirrored_for: Option<&str>,
        visibility: Visibility,
    ) -> color_eyre::Result<ReleaseMetadata> {
        let span = tracing::Span::current();
        if let Some(mirrored_for) = &mirrored_for {
            span.record("mirrored_for", mirrored_for);
        }
        let gix_repository = gix::open(directory).wrap_err("Opening the Git repository")?;
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

        let revision_count = get_revision_count(
            reqwest_client,
            project_owner,
            project_name,
            &revision_string,
        )
        .await?;
        span.record("revision_count", revision_count);

        let description = flake_metadata
            .get("description")
            .ok_or_else(|| {
                eyre!("`nix flake metadata --json` does not have a `description` field")
            })?
            .as_str()
            .ok_or_else(|| {
                eyre!("`nix flake metadata --json` does not have a string `description` field")
            })?
            .to_string();

        let readme = tokio::fs::read_to_string(directory.join("README.md")).await?;

        Ok(ReleaseMetadata {
            description,
            mirrored_for: mirrored_for.map(|v| v.to_string()),
            raw_flake_metadata: flake_metadata.clone(),
            readme,
            revision: revision_string,
            commit_count: revision_count,
            visibility,
            outputs: flake_outputs,
        })
    }
}

#[tracing::instrument(skip_all)]
pub(crate) async fn get_revision_count(
    reqwest_client: reqwest::Client,
    project_owner: &str,
    project_name: &str,
    revision: &str,
) -> color_eyre::Result<i64> {
    // Schema from https://docs.github.com/public/schema.docs.graphql
    let graphql_response = {
        let variables = crate::graphql::rev_count_query::Variables {
            owner: project_owner.to_string(),
            name: project_name.to_string(),
            revision: revision.to_string(),
        };
        let query = crate::graphql::RevCountQuery::build_query(variables);
        let reqwest_response = reqwest_client
            .post(crate::graphql::GITHUB_ENDPOINT)
            .json(&query)
            .send()
            .await
            .wrap_err("Failed to issue RevCountQuery request to Github's GraphQL API")?;

        let response_status = reqwest_response.status();
        let response_data: serde_json::Value = reqwest_response
            .json()
            .await
            .wrap_err("Failed to retrieve RevCountQuery response from Github's GraphQL API")?;

        if response_status != 200 {
            tracing::error!(status = %response_status,
                "Recieved error:\n\
                {response_data:#?}\n\
            "
            );
            return Err(eyre!(
                "Got {response_status} status from Github's GraphQL API, expected 200"
            ));
        }

        let graphql_data = response_data.get("data").ok_or_else(|| {
            eyre!(
                "Did not recieve a `data` inside RevCountQuery response from Github's GraphQL API"
            )
        })?;
        let response_data: <crate::graphql::RevCountQuery as GraphQLQuery>::ResponseData =
            serde_json::from_value(graphql_data.clone())
                .wrap_err("Failed to retrieve RevCountQuery response from Github's GraphQL API")?;
        response_data
    };
    let graphql_repository_object = graphql_response
            .repository
            .ok_or_else(|| eyre!("Did not recieve a `repository` inside RevCountQuery response from Github's GraphQL API"))?
            .object
            .ok_or_else(|| eyre!("Did not recieve a `repository.object` inside RevCountQuery response from Github's GraphQL API"))?;

    let total_count = match graphql_repository_object {
            RevCountQueryRepositoryObject::Blob
            | RevCountQueryRepositoryObject::Tag
            | RevCountQueryRepositoryObject::Tree => {
                return Err(eyre!(
                "Retrieved a `repository.object` that was not a `Commit` in the RevCountQuery response from Github's GraphQL API"
            ))
            }
            RevCountQueryRepositoryObject::Commit(crate::graphql::rev_count_query::RevCountQueryRepositoryObjectOnCommit {
                history: crate::graphql::rev_count_query::RevCountQueryRepositoryObjectOnCommitHistory {
                    total_count,
                }
            }) => total_count,
        };
    Ok(total_count)
}

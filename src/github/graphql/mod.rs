// Get the schema from https://docs.github.com/public/schema.docs.graphql

use color_eyre::eyre::{eyre, WrapErr};
use graphql_client::GraphQLQuery;

pub(crate) const GITHUB_ENDPOINT: &str = "https://api.github.com/graphql";
pub(crate) const MAX_LABEL_LENGTH: usize = 50;
pub(crate) const MAX_NUM_TOTAL_LABELS: usize = 25;
const MAX_NUM_EXTRA_TOPICS: i64 = 20;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github/graphql/github_schema.graphql",
    query_path = "src/github/graphql/query/github_graphql_data_query.graphql",
    response_derives = "Debug",
    variables_derives = "Debug"
)]
pub(crate) struct GithubGraphqlDataQuery;

impl GithubGraphqlDataQuery {
    #[tracing::instrument(skip_all, fields(
        %project_owner,
        %project_name,
        %revision,
    ))]
    pub(crate) async fn get(
        reqwest_client: &reqwest::Client,
        bearer_token: &str,
        project_owner: &str,
        project_name: &str,
        revision: &str,
    ) -> color_eyre::Result<GithubGraphqlDataResult> {
        // Schema from https://docs.github.com/public/schema.docs.graphql
        let graphql_data = {
            let variables = github_graphql_data_query::Variables {
                owner: project_owner.to_string(),
                name: project_name.to_string(),
                revision: revision.to_string(),
                max_num_topics: MAX_NUM_EXTRA_TOPICS,
            };

            tracing::debug!(?variables); // TODO remove

            let query = GithubGraphqlDataQuery::build_query(variables);
            let reqwest_response = reqwest_client
                .post(GITHUB_ENDPOINT)
                .bearer_auth(bearer_token)
                .json(&query)
                .send()
                .await
                .wrap_err("Failed to issue RevCountQuery request to Github's GraphQL API")?;

            let response_status = reqwest_response.status();
            let response: graphql_client::Response<
                <crate::github::graphql::GithubGraphqlDataQuery as GraphQLQuery>::ResponseData,
            > = reqwest_response
                .json()
                .await
                .wrap_err("Failed to retrieve RevCountQuery response from Github's GraphQL API")?;

            if response_status != 200 {
                tracing::error!(status = %response_status,
                    "Recieved error:\n\
                    {response:#?}\n\
                "
                );
                return Err(eyre!(
                    "Got {response_status} status from Github's GraphQL API, expected 200"
                ));
            }

            if response.errors.is_some() {
                tracing::warn!(?response.errors, "Got errors from GraphQL query");
            }

            response.data.ok_or_else(|| {
                eyre!(
                    "Did not receive a `data` inside GithubGraphqlDataQuery response from Github's GraphQL API"
                )
            })?
        };
        tracing::debug!(?graphql_data, "Got response data");

        let graphql_repository = graphql_data
            .repository
            .ok_or_else(|| eyre!("Did not receive a `repository` inside GithubGraphqlDataQuery response from Github's GraphQL API. Does the repository {project_owner}/{project_name} exist on GitHub, and does your GitHub access token have access to it?"))?;

        let graphql_repository_object = graphql_repository
                .object
                .ok_or_else(|| eyre!("Did not receive a `repository.object` inside GithubGraphqlDataQuery response from Github's GraphQL API. Is the current commit {revision} pushed to GitHub?"))?;

        let rev_count = match graphql_repository_object {
                github_graphql_data_query::GithubGraphqlDataQueryRepositoryObject::Blob
                | github_graphql_data_query::GithubGraphqlDataQueryRepositoryObject::Tag
                | github_graphql_data_query::GithubGraphqlDataQueryRepositoryObject::Tree => {
                    return Err(eyre!(
                    "Retrieved a `repository.object` that was not a `Commit` in the GithubGraphqlDataQuery response from Github's GraphQL API. This shouldn't happen, because only commits can be checked out!"
                ))
                }
                github_graphql_data_query::GithubGraphqlDataQueryRepositoryObject::Commit(github_graphql_data_query::GithubGraphqlDataQueryRepositoryObjectOnCommit {
                    history: github_graphql_data_query::GithubGraphqlDataQueryRepositoryObjectOnCommitHistory {
                        total_count,
                    }
                }) => total_count,
            };

        let spdx_identifier = graphql_repository
            .license_info
            .and_then(|info| info.spdx_id);

        let project_id = graphql_repository
            .database_id
            .ok_or_else(|| eyre!("Did not receive a `repository.databaseId` inside GithubGraphqlDataQuery response from Github's GraphQL API. Is GitHub's API experiencing issues?"))?;
        let owner_id = match graphql_repository.owner {
            github_graphql_data_query::GithubGraphqlDataQueryRepositoryOwner::Organization(org) => {
                org.database_id
            }
            github_graphql_data_query::GithubGraphqlDataQueryRepositoryOwner::User(user) => {
                user.database_id
            }
        };
        let owner_id = owner_id
            .ok_or_else(|| eyre!("Did not receive a `repository.owner.databaseId` inside GithubGraphqlDataQuery response from Github's GraphQL API. Is GitHub's API experiencing issues?"))?;

        let topics: Vec<String> = graphql_repository
            .repository_topics
            .edges
            .unwrap_or(vec![])
            .iter()
            .flatten()
            .filter_map(|edge| edge.node.as_ref())
            .map(|node| node.topic.name.clone())
            .collect();

        Ok(GithubGraphqlDataResult {
            revision: revision.to_string(),
            rev_count,
            spdx_identifier,
            project_id,
            owner_id,
            topics,
        })
    }
}

#[derive(Debug)]
pub(crate) struct GithubGraphqlDataResult {
    pub(crate) revision: String,
    pub(crate) rev_count: i64,
    pub(crate) spdx_identifier: Option<String>,
    pub(crate) project_id: i64,
    pub(crate) owner_id: i64,
    pub(crate) topics: Vec<String>,
}

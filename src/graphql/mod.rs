// Get the schema from https://docs.github.com/public/schema.docs.graphql

use color_eyre::eyre::{eyre, WrapErr};
use graphql_client::GraphQLQuery;

pub(crate) const GITHUB_ENDPOINT: &str = "https://api.github.com/graphql";

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/github_schema.graphql",
    query_path = "src/graphql/query/github_graphql_data_query.graphql",
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
        reqwest_client: reqwest::Client,
        project_owner: &str,
        project_name: &str,
        revision: &str,
    ) -> color_eyre::Result<GithubGraphqlDataResult> {
        // Schema from https://docs.github.com/public/schema.docs.graphql
        let graphql_response = {
            let variables = github_graphql_data_query::Variables {
                owner: project_owner.to_string(),
                name: project_name.to_string(),
                revision: revision.to_string(),
            };
            let query = GithubGraphqlDataQuery::build_query(variables);
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
                    "Did not recieve a `data` inside GithubGraphqlDataQuery response from Github's GraphQL API"
                )
            })?;
            let response_data: <crate::graphql::GithubGraphqlDataQuery as GraphQLQuery>::ResponseData =
                serde_json::from_value(graphql_data.clone())
                    .wrap_err("Failed to retrieve GithubGraphqlDataQuery response from Github's GraphQL API")?;
            response_data
        };
        let graphql_repository = graphql_response
            .repository
            .ok_or_else(|| eyre!("Did not recieve a `repository` inside GithubGraphqlDataQuery response from Github's GraphQL API"))?;

        let graphql_repository_object = graphql_repository
                .object
                .ok_or_else(|| eyre!("Did not recieve a `repository.object` inside GithubGraphqlDataQuery response from Github's GraphQL API"))?;

        let rev_count = match graphql_repository_object {
                github_graphql_data_query::GithubGraphqlDataQueryRepositoryObject::Blob
                | github_graphql_data_query::GithubGraphqlDataQueryRepositoryObject::Tag
                | github_graphql_data_query::GithubGraphqlDataQueryRepositoryObject::Tree => {
                    return Err(eyre!(
                    "Retrieved a `repository.object` that was not a `Commit` in the GithubGraphqlDataQuery response from Github's GraphQL API"
                ))
                }
                github_graphql_data_query::GithubGraphqlDataQueryRepositoryObject::Commit(github_graphql_data_query::GithubGraphqlDataQueryRepositoryObjectOnCommit {
                    history: github_graphql_data_query::GithubGraphqlDataQueryRepositoryObjectOnCommitHistory {
                        total_count,
                    }
                }) => total_count,
            };

        let spdx_identifier = graphql_repository.license_info
            .and_then(|info| info.spdx_id);

        Ok(GithubGraphqlDataResult {
            rev_count,
            spdx_identifier,
        })
    }
}

pub(crate) struct GithubGraphqlDataResult {
    pub(crate) rev_count: i64,
    pub(crate) spdx_identifier: Option<String>,
}

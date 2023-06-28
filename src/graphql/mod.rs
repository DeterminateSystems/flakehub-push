// Get the schema from https://docs.github.com/public/schema.docs.graphql

use graphql_client::GraphQLQuery;

pub(crate) const GITHUB_ENDPOINT: &str = "https://api.github.com/graphql";

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/github_schema.graphql",
    query_path = "src/graphql/rev_count_query.graphql",
    response_derives = "Debug"
)]
pub(crate) struct RevCountQuery;

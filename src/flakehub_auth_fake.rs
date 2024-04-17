use color_eyre::eyre::{Context, Result};

use crate::github::graphql::GithubGraphqlDataResult;

pub async fn get_fake_bearer_token(
    jwt_issuer_uri: &str,
    project_owner: &str,
    repository: &str,
    github_graphql_data_result: GithubGraphqlDataResult,
) -> Result<String> {
    tracing::warn!("running outside github/gitlab - minting a dev-signed JWT");

    let client = reqwest::Client::new();

    let mut claims = github_actions_oidc_claims::Claims::make_dummy();
    // FIXME: we should probably fill in more of these claims.

    // TODO(review): on the contrary, I think we should ditch this, and we should basically use forge_login-esque functionality for this going forward
    // this would remove the entire need for the fake JWT server, since we are ourselves a JWT issuer
    claims.aud = "flakehub-localhost".to_string();
    claims.iss = "flakehub-push-dev".to_string();
    claims.repository = repository.to_string();
    claims.repository_owner = project_owner.to_string();

    claims.repository_id = github_graphql_data_result.project_id.to_string();
    claims.repository_owner_id = github_graphql_data_result.owner_id.to_string();

    tracing::debug!(?claims);

    let response = client
        .post(jwt_issuer_uri)
        .header("Content-Type", "application/json")
        .json(&claims)
        .send()
        .await
        .wrap_err("Sending request to JWT issuer")?;

    #[derive(serde::Deserialize)]
    struct Response {
        token: String,
    }
    let response_deserialized: Response = response
        .json()
        .await
        .wrap_err("Getting token from JWT issuer's response")?;
    Ok(response_deserialized.token)
}

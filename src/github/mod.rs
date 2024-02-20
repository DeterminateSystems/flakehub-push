pub(crate) mod graphql;

use color_eyre::eyre::{eyre, WrapErr};

use crate::build_http_client;

#[tracing::instrument(skip_all, fields(audience = tracing::field::Empty))]
pub(crate) async fn get_actions_id_bearer_token(host: &url::Url) -> color_eyre::Result<String> {
    let span = tracing::Span::current();
    let audience = host.host_str().ok_or_else(|| eyre!("`host` must contain a valid host (eg `https://api.flakehub.com` contains `api.flakehub.com`)"))?;
    span.record("audience", audience);

    let actions_id_token_request_token = std::env::var("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
        // We do want to preserve the whitespace here  
        .wrap_err("\
No `ACTIONS_ID_TOKEN_REQUEST_TOKEN` found, `flakehub-push` requires a JWT. To provide this, add `permissions` to your job, eg:

# ...
jobs:
    example:
    runs-on: ubuntu-latest
    permissions:
        id-token: write # Authenticate against FlakeHub
        contents: read
    steps:
    - uses: actions/checkout@v3
    # ...\n\
        ")?;
    let actions_id_token_request_url = std::env::var("ACTIONS_ID_TOKEN_REQUEST_URL").wrap_err("`ACTIONS_ID_TOKEN_REQUEST_URL` required if `ACTIONS_ID_TOKEN_REQUEST_TOKEN` is also present")?;
    let actions_id_token_client = build_http_client().build()?;
    let response = actions_id_token_client
        .get(format!(
            "{actions_id_token_request_url}&audience={audience}"
        ))
        .bearer_auth(actions_id_token_request_token)
        .send()
        .await
        .wrap_err("Getting Actions ID bearer token")?;

    let response_json: serde_json::Value = response
        .json()
        .await
        .wrap_err("Getting JSON from Actions ID bearer token response")?;

    let response_bearer_token = response_json
        .get("value")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| eyre!("Getting value from Actions ID bearer token response"))?;

    Ok(response_bearer_token.to_string())
}

pub(crate) mod graphql;

use color_eyre::eyre::{eyre, WrapErr};
use serde::{Deserialize, Serialize};

use crate::build_http_client;

const GITHUB_ACTOR_TYPE_USER: &str = "User";
const GITHUB_ACTOR_TYPE_ORGANIZATION: &str = "Organization";

#[derive(Serialize, Deserialize)]
pub struct WorkflowData {
    event: WorkflowDataEvent,
}

#[derive(Serialize, Deserialize)]
pub struct WorkflowDataEvent {
    repository: WorkflowDataEventRepo,
}

#[derive(Serialize, Deserialize)]
pub struct WorkflowDataEventRepo {
    owner: WorkflowDataEventRepoOwner,
}

#[derive(Serialize, Deserialize)]
pub struct WorkflowDataEventRepoOwner {
    login: String,
    #[serde(rename = "type")]
    kind: String,
}

pub(crate) fn get_actions_event_data() -> color_eyre::Result<WorkflowData> {
    let github_context = std::env::var("GITHUB_CONTEXT")?;
    let workflow_data: WorkflowData = serde_json::from_str::<WorkflowData>(&github_context)?;

    Ok(workflow_data)
}

pub(crate) fn print_unauthenticated_error() {
    let mut msg = "::error title=FlakeHub registration required.::Unable to authenticate to FlakeHub. Individuals must register at FlakeHub.com; Organizations must create an organization at FlakeHub.com.".to_string();
    if let Ok(workflow_data) = get_actions_event_data() {
        let owner = workflow_data.event.repository.owner;
        if owner.kind == GITHUB_ACTOR_TYPE_USER {
            msg = format!(
                "::error title=FlakeHub registration required.::Please create an account for {} on FlakeHub.com to publish flakes.",
                &owner.login
            );
        } else if owner.kind == GITHUB_ACTOR_TYPE_ORGANIZATION {
            msg = format!(
                "::error title=FlakeHub registration required.::Please create an organization for {} on FlakeHub.com to publish flakes.",
                &owner.login
            );
        }
    };
    println!("{}", msg);
}

#[tracing::instrument(skip_all, fields(audience = tracing::field::Empty))]
pub(crate) async fn get_actions_id_bearer_token(host: &url::Url) -> color_eyre::Result<String> {
    let span = tracing::Span::current();
    let audience = host.host_str().ok_or_else(|| eyre!("`--host` must contain a valid host (eg `https://api.flakehub.com` contains `api.flakehub.com`)"))?;
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

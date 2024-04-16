use std::str::FromStr;

use color_eyre::eyre::{eyre, Context, Result};
use http::StatusCode;
use reqwest::{header::HeaderMap, Response};
use uuid::Uuid;

use crate::release_metadata::ReleaseMetadata;

pub struct FlakeHubClient {
    host: url::Url,
    bearer_token: String,
    client: reqwest::Client,
}

pub struct Tarball {
    pub hash_base64: String,
    pub bytes: Vec<u8>,
}

// TODO(colemickens): static init
pub fn flakehub_headers() -> HeaderMap {
    let mut header_map = HeaderMap::new();

    header_map.insert(
        reqwest::header::CONTENT_TYPE,
        reqwest::header::HeaderValue::from_str("application/json").unwrap(),
    );
    // TODO(colemickens): tube > ngrok, remove
    header_map.insert(
        reqwest::header::HeaderName::from_static("ngrok-skip-browser-warning"),
        reqwest::header::HeaderValue::from_str("please").unwrap(),
    );
    header_map
}

impl FlakeHubClient {
    pub fn new(host: url::Url, token: String) -> Result<Self> {
        let builder = reqwest::ClientBuilder::new().user_agent("flakehub-push");

        let client = builder.build()?;

        let client = Self {
            client: client,
            bearer_token: token,
            host: host,
        };

        Ok(client)
    }
    pub async fn release_stage(
        &self,
        upload_name: &str,
        release_version: &str,
        release_metadata: &ReleaseMetadata,
        tarball: &Tarball,
    ) -> Result<Response> {
        let flake_tarball_len = tarball.bytes.len();
        let flake_tarball_hash_base64 = &tarball.hash_base64;
        let relative_url = &format!("upload/{upload_name}/{release_version}/{flake_tarball_len}/{flake_tarball_hash_base64}");

        let release_metadata_post_url = format!("{}/{}", self.host, relative_url);
        // TODO(colemickens): better join

        tracing::debug!(
            url = %release_metadata_post_url,
            "Computed release metadata POST URL"
        );

        let response = self
            .client
            .post(release_metadata_post_url)
            .bearer_auth(&self.bearer_token)
            .headers(flakehub_headers())
            .json(&release_metadata)
            .send()
            .await
            .unwrap();

        Ok(response)
    }

    pub async fn release_publish(&self, release_uuidv7: Uuid) -> Result<()> {
        let publish_post_url = format!("{}/publish/{}", self.host, release_uuidv7);
        // TODO(colemickens): fix url joining

        tracing::debug!(url = %publish_post_url, "Computed publish POST URL");

        let publish_response = self
            .client
            .post(publish_post_url)
            .bearer_auth(&self.bearer_token)
            .headers(flakehub_headers())
            .send()
            .await
            .wrap_err("Publishing release")?;

        let publish_response_status = publish_response.status();
        tracing::trace!(
            status = tracing::field::display(publish_response_status),
            "Got publish POST response"
        );

        if publish_response_status != 200 {
            return Err(eyre!(
                "\
                    Status {publish_response_status} from publish POST\n\
                    {}\
                ",
                String::from_utf8_lossy(&publish_response.bytes().await.unwrap())
            ));
        }

        // TODO: return the actual response object?
        Ok(())
    }
}

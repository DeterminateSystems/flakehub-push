use color_eyre::eyre::{eyre, Context, Result};
use http::StatusCode;
use reqwest::header::HeaderMap;
use reqwest::Response;
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

#[derive(serde::Deserialize)]
pub(crate) struct StageResult {
    pub(crate) s3_upload_url: String,
    pub(crate) uuid: Uuid,
}

// TODO(future): static init
pub fn flakehub_headers() -> HeaderMap {
    let mut header_map = HeaderMap::new();

    header_map.insert(
        reqwest::header::CONTENT_TYPE,
        reqwest::header::HeaderValue::from_str("application/json").unwrap(),
    );
    header_map
}

impl FlakeHubClient {
    pub fn new(host: url::Url, bearer_token: String) -> Result<Self> {
        let builder = reqwest::ClientBuilder::new().user_agent("flakehub-push");

        let client = builder.build()?;

        let client = Self {
            client,
            bearer_token,
            host,
        };

        Ok(client)
    }

    pub async fn token_status(&self) -> Result<Response> {
        let status_url = self.host.join("token/")?.join("status")?;

        self.client
            .get(status_url)
            .bearer_auth(&self.bearer_token)
            .headers(flakehub_headers())
            .send()
            .await
            .wrap_err("Checking token status")
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
        let relative_url: &String = &format!("upload/{upload_name}/{release_version}/{flake_tarball_len}/{flake_tarball_hash_base64}");

        let release_metadata_post_url = self.host.join(relative_url)?;

        tracing::debug!(
            url = %release_metadata_post_url,
            "Computed release metadata POST URL"
        );

        self.client
            .post(release_metadata_post_url)
            .bearer_auth(&self.bearer_token)
            .headers(flakehub_headers())
            .json(&release_metadata)
            .send()
            .await
            .wrap_err("Publishing release")
    }

    pub async fn release_publish(&self, release_uuidv7: Uuid) -> Result<()> {
        let relative_url = format!("publish/{}", release_uuidv7);
        let publish_post_url = self.host.join(&relative_url)?;

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

        if publish_response_status != StatusCode::OK {
            return Err(eyre!(
                "\
                    Status {publish_response_status} from publish POST\n\
                    {}\
                ",
                String::from_utf8_lossy(&publish_response.bytes().await.unwrap())
            ));
        }

        Ok(())
    }
}

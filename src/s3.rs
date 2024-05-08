use color_eyre::eyre::{eyre, Result, WrapErr};
use reqwest::header::HeaderMap;

use crate::flakehub_client::Tarball;

pub async fn upload_release_to_s3(presigned_s3_url: String, tarball: Tarball) -> Result<()> {
    let client = reqwest::Client::new();
    let tarball_put_response = client
        .put(presigned_s3_url)
        .headers({
            let mut header_map = HeaderMap::new();
            header_map.insert(
                reqwest::header::CONTENT_LENGTH,
                reqwest::header::HeaderValue::from_str(&format!("{}", tarball.bytes.len()))
                    .unwrap(),
            );
            header_map.insert(
                reqwest::header::HeaderName::from_static("x-amz-checksum-sha256"),
                reqwest::header::HeaderValue::from_str(&tarball.hash_base64).unwrap(),
            );
            header_map.insert(
                reqwest::header::CONTENT_TYPE,
                reqwest::header::HeaderValue::from_str("application/gzip").unwrap(),
            );
            header_map
        })
        .body(tarball.bytes)
        .send()
        .await
        .wrap_err("Sending tarball PUT")?;

    let tarball_put_response_status = tarball_put_response.status();
    tracing::trace!(
        status = tracing::field::display(tarball_put_response_status),
        "Got tarball PUT response"
    );
    if !tarball_put_response_status.is_success() {
        return Err(eyre!(
            "Got {tarball_put_response_status} status from PUT request"
        ));
    }

    Ok(())
}

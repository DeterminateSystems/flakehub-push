use std::collections::HashMap;

use color_eyre::eyre::{eyre, Result, WrapErr};
use http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use http::{HeaderName, HeaderValue};
use reqwest::header::HeaderMap;

use crate::flakehub_client::Tarball;

pub async fn upload_release_to_s3(
    presigned_s3_url: String,
    s3_headers: HashMap<String, String>,
    tarball: Tarball,
) -> Result<()> {
    let overrides: HashMap<&str, String> = HashMap::from_iter([
        (CONTENT_LENGTH.as_str(), tarball.bytes.len().to_string()),
        (CONTENT_TYPE.as_str(), "application/gzip".to_string()),
        ("x-amz-checksum-sha256", tarball.hash_base64),
    ]);

    let headers = build_headers(&s3_headers, &overrides)?;

    let client = reqwest::Client::new();
    let tarball_put_response = client
        .put(presigned_s3_url)
        .headers(headers)
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

fn build_headers(
    caller_headers: &HashMap<String, String>,
    overrides: &HashMap<&str, String>,
) -> Result<HeaderMap> {
    let mut header_map = HeaderMap::with_capacity(caller_headers.len() + overrides.len());

    for (name, value) in caller_headers {
        if overrides.keys().any(|k| k.eq_ignore_ascii_case(name)) {
            continue;
        }
        let header_name = HeaderName::from_bytes(name.as_bytes())
            .wrap_err_with(|| format!("Invalid header name `{name}`"))?;
        let header_value = HeaderValue::from_str(value)
            .wrap_err_with(|| format!("Invalid header value for `{name}`"))?;
        header_map.insert(header_name, header_value);
    }

    for (name, value) in overrides {
        let header_name = HeaderName::from_bytes(name.as_bytes())
            .wrap_err_with(|| format!("Invalid header name `{name}`"))?;
        let header_value = HeaderValue::from_str(value)
            .wrap_err_with(|| format!("Invalid header value for `{name}`"))?;
        header_map.insert(header_name, header_value);
    }

    Ok(header_map)
}

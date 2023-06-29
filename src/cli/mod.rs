mod instrumentation;

use color_eyre::eyre::{eyre, WrapErr};
use reqwest::header::HeaderMap;
use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};
use tokio::io::AsyncWriteExt;

use crate::{
    flake_info::{get_flake_metadata, get_flake_tarball, get_flake_tarball_outputs},
    release_metadata::ReleaseMetadata,
    Visibility,
};

#[derive(Debug, clap::Parser)]
#[clap(version)]
pub(crate) struct NixfrPushCli {
    #[clap(long, env = "NXFR_PUSH_HOST", default_value = "https://nxfr.fly.dev")]
    pub(crate) host: String,
    #[clap(long, env = "NXFR_PUSH_VISIBLITY")]
    pub(crate) visibility: crate::Visibility,
    // Will also detect `GITHUB_REF_NAME`
    #[clap(long, env = "NXFR_PUSH_TAG")]
    pub(crate) tag: Option<String>,
    #[clap(long, env = "NXFR_PUSH_ROLLING_PREFIX")]
    pub(crate) rolling_prefix: Option<String>,
    // Also detects `GITHUB_TOKEN`
    #[clap(long, env = "NXFR_PUSH_GITHUB_TOKEN")]
    pub(crate) github_token: Option<String>,
    /// Will also detect `GITHUB_REPOSITORY`
    #[clap(long, env = "NXFR_PUSH_UPLOAD_NAME")]
    pub(crate) upload_name: Option<String>,
    /// Override the detected repo name, e.g. in case you're uploading multiple subflakes in a single repo as their own flake.
    ///
    /// In the format of [org]/[repo].
    #[clap(long, env = "NXFR_PUSH_REPO")]
    pub(crate) repo: Option<String>,
    // Also detects `GITHUB_WORKSPACE`
    #[clap(long, env = "NXFR_PUSH_DIRECTORY")]
    pub(crate) directory: Option<PathBuf>,
    // Also detects `GITHUB_WORKSPACE`
    #[clap(long, env = "NXFR_PUSH_GIT_ROOT")]
    pub(crate) git_root: Option<PathBuf>,

    // A specific bearer token string which bypasses the normal authentication check in when pushing an upload
    // This is intended for development at this time.
    #[cfg(debug_assertions)]
    #[clap(long, env = "NXFR_PUSH_DEV_BEARER_TOKEN")]
    pub(crate) dev_bearer_token: Option<String>,

    #[clap(flatten)]
    pub instrumentation: instrumentation::Instrumentation,
}

fn empty_to_none(v: String) -> Option<String> {
    if v.trim().is_empty() {
        None
    } else {
        Some(v)
    }
}

impl NixfrPushCli {
    #[tracing::instrument(
        name = "nxfr_push"
        skip_all,
    )]
    pub(crate) async fn execute(self) -> color_eyre::Result<std::process::ExitCode> {
        tracing::trace!(?self, "Executing");
        let Self {
            host,
            visibility,
            upload_name,
            tag,
            rolling_prefix,
            github_token,
            directory,
            repo,
            git_root,
            #[cfg(debug_assertions)]
            dev_bearer_token,
            instrumentation: _,
        } = self;
        let repo = repo.and_then(empty_to_none);
        let upload_name = upload_name.and_then(empty_to_none);
        let tag = tag.and_then(empty_to_none);
        let rolling_prefix = rolling_prefix.and_then(empty_to_none);
        let github_token = github_token.and_then(empty_to_none);
        #[cfg(debug_assertions)]
        let dev_bearer_token = dev_bearer_token.and_then(empty_to_none);

        let github_token = if let Some(github_token) = &github_token.and_then(empty_to_none) {
            github_token.clone()
        } else {
            std::env::var("GITHUB_TOKEN")
                .wrap_err("Could not determine Github token, pass `--github-token`, or set either `NXFR_PUSH_GITHUB_TOKEN` or `GITHUB_TOKEN`")?
        };

        let directory = if let Some(directory) = &directory {
            directory.clone()
        } else if let Ok(github_workspace) = std::env::var("GITHUB_WORKSPACE") {
            tracing::trace!(%github_workspace, "Got $GITHUB_WORKSPACE");
            PathBuf::from(github_workspace)
        } else {
            std::env::current_dir().map(PathBuf::from).wrap_err("Could not determine current directory. Pass `--directory` or set `NXFR_PUSH_DIRECTORY`")?
        };
        let git_root = if let Some(git_root) = &git_root {
            git_root.clone()
        } else if let Ok(github_workspace) = std::env::var("GITHUB_WORKSPACE") {
            tracing::trace!(%github_workspace, "Got $GITHUB_WORKSPACE");
            PathBuf::from(github_workspace)
        } else {
            std::env::current_dir().map(PathBuf::from).wrap_err("Could not determine current git_root. Pass `--git-root` or set `NXFR_PUSH_GIT_ROOT`")?
        };

        let owner_and_repository = if let Some(repo) = &repo {
            tracing::trace!(%repo, "Got `--repo` argument");
            repo.clone()
        } else if let Ok(github_repository) = std::env::var("GITHUB_REPOSITORY") {
            tracing::trace!(
                %github_repository,
                "Got `GITHUB_REPOSITORY`"
            );
            github_repository
        } else {
            return Err(eyre!("Could not determine repository name, pass `--repo` or the `GITHUB_REPOSITORY` formatted like `determinatesystems/nxfr-push`"));
        };
        let mut owner_and_repository_split = owner_and_repository.split('/');
        let project_owner = owner_and_repository_split
                .next()
                .ok_or_else(|| eyre!("Could not determine owner, pass `--name`, `--mirrored-for` or the `GITHUB_REPOSITORY` formatted like `determinatesystems/nxfr-push`"))?
                .to_string();
        let project_name = owner_and_repository_split.next()
            .ok_or_else(|| eyre!("Could not determine project, pass `--name`, `--mirrored-for` or the `GITHUB_REPOSITORY` formatted like `determinatesystems/nxfr-push`"))?
            .to_string();

        let tag = if let Some(tag) = &tag {
            Some(tag.clone())
        } else {
            std::env::var("GITHUB_REF_NAME").ok()
        };

        push_new_release(
            &host,
            &github_token,
            &directory,
            &git_root,
            &project_owner,
            &project_name,
            upload_name.as_deref(),
            visibility,
            tag.as_deref(),
            rolling_prefix.as_deref(),
            #[cfg(debug_assertions)]
            dev_bearer_token.as_deref(),
        )
        .await?;

        Ok(ExitCode::SUCCESS)
    }
}

#[tracing::instrument(
    skip_all,
    fields(
        name = tracing::field::Empty,
        owner = tracing::field::Empty,
        tag = tracing::field::Empty,
        source = tracing::field::Empty,
    )
)]
#[allow(clippy::too_many_arguments)]
async fn push_new_release(
    host: &str,
    github_token: &str,
    directory: &Path,
    git_root: &Path,
    project_owner: &str,
    project_name: &str,
    upload_name: Option<&str>,
    visibility: Visibility,
    tag: Option<&str>,
    rolling_prefix: Option<&str>,
    #[cfg(debug_assertions)] dev_bearer_token: Option<&str>,
) -> color_eyre::Result<()> {
    let span = tracing::Span::current();

    let rolling_prefix_or_tag = rolling_prefix.or(tag).ok_or_else(|| {
        eyre!("Could not determine tag or rolling prefix, `--tag`, `GITHUB_REF_NAME`, or `--rolling-prefix` must be set")
    })?;

    let upload_owner_repo_pair = if let Some(upload_name) = upload_name {
        upload_name.to_string()
    } else {
        format!("{project_owner}/{project_name}")
    };
    tracing::info!("Preparing release of {upload_owner_repo_pair}/{rolling_prefix_or_tag}");

    let tempdir = tempfile::Builder::new()
        .prefix("nxfr_push")
        .tempdir()
        .wrap_err("Creating tempdir")?;

    let github_api_client = reqwest::Client::builder()
        .user_agent("nxfr-push")
        .default_headers(
            std::iter::once((
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Bearer {}", github_token))
                    .unwrap(),
            ))
            .collect(),
        )
        .build()?;

    let flake_metadata = get_flake_metadata(directory)
        .await
        .wrap_err("Getting flake metadata")?;
    tracing::debug!("Got flake metadata");

    let flake_metadata_value_path = flake_metadata
        .get("path")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            eyre!("Could not get `path` attribute from `nix flake metadata --json` output")
        })?;
    let flake_metadata_value_resolved_dir = flake_metadata
        .pointer("/resolved/dir")
        .and_then(serde_json::Value::as_str);

    let source = match flake_metadata_value_resolved_dir {
        Some(flake_metadata_value_resolved_dir) => {
            Path::new(flake_metadata_value_path).join(flake_metadata_value_resolved_dir)
        }
        None => PathBuf::from(flake_metadata_value_path),
    };
    span.record("source", tracing::field::display(source.clone().display()));
    tracing::debug!("Found source");

    let flake_tarball = get_flake_tarball(&source)
        .await
        .wrap_err("Making release tarball")?;

    let flake_tarball_len: usize = flake_tarball.len();
    let flake_tarball_hash = {
        let mut context = ring::digest::Context::new(&ring::digest::SHA256);
        context.update(&flake_tarball);
        context.finish()
    };
    let flake_tarball_hash_base64 = {
        // TODO: Use URL_SAFE_NO_PAD
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        STANDARD.encode(flake_tarball_hash)
    };
    tracing::debug!(
        flake_tarball_len,
        flake_tarball_hash_base64,
        "Got tarball metadata"
    );

    let flake_tarball_path = tempdir.path().join("release.tar.gz");
    let mut tempfile = tokio::fs::File::create(&flake_tarball_path)
        .await
        .wrap_err("Creating release.tar.gz")?;
    tempfile
        .write_all(&flake_tarball)
        .await
        .wrap_err("Writing compressed tarball to tempfile")?;

    let get_flake_tarball_outputs = get_flake_tarball_outputs(&flake_tarball_path).await?;

    let release_metadata = ReleaseMetadata::build(
        github_api_client,
        directory,
        git_root,
        flake_metadata,
        get_flake_tarball_outputs,
        project_owner,
        project_name,
        visibility,
    )
    .await
    .wrap_err("Building release metadata")?;

    #[cfg(debug_assertions)]
    let upload_bearer_token = match &dev_bearer_token {
        None => get_actions_id_bearer_token()
            .await
            .wrap_err("Getting upload bearer token")?,
        Some(dev_token) => {
            tracing::warn!(dev_bearer_token = %dev_token, "This nxfr-push has `dev_bearer_token` set for upload. This is intended for development purposes only.");
            dev_token.to_string()
        }
    };
    #[cfg(not(debug_assertions))]
    let upload_bearer_token = get_actions_id_bearer_token()
        .await
        .wrap_err("Getting upload bearer token")?;

    let reqwest_client = reqwest::Client::builder().user_agent("nxfr-push").build()?;

    let rolling_prefix_with_postfix_or_tag = if let Some(rolling_prefix) = &rolling_prefix {
        format!(
            "{rolling_prefix}.{}+rev-{}",
            release_metadata.commit_count, release_metadata.revision
        )
    } else {
        rolling_prefix_or_tag.to_string() // This will always be the tag since `self.rolling_prefix` was empty.
    };

    let release_metadata_post_url = format!(
        "{host}/upload/{upload_owner_repo_pair}/{rolling_prefix_with_postfix_or_tag}/{flake_tarball_len}/{flake_tarball_hash_base64}"
    );
    tracing::debug!(
        url = release_metadata_post_url,
        "Got release metadata POST URL"
    );

    let release_metadata_post_response = reqwest_client
        .post(release_metadata_post_url)
        .headers({
            let mut header_map = HeaderMap::new();

            header_map.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("bearer {}", upload_bearer_token))
                    .unwrap(),
            );
            header_map.insert(
                reqwest::header::CONTENT_TYPE,
                reqwest::header::HeaderValue::from_str("application/json").unwrap(),
            );
            header_map.insert(
                reqwest::header::HeaderName::from_static("ngrok-skip-browser-warning"),
                reqwest::header::HeaderValue::from_str("please").unwrap(),
            );
            header_map
        })
        .json(&release_metadata)
        .send()
        .await
        .wrap_err("Sending release metadata")?;

    let release_metadata_post_response_status = release_metadata_post_response.status();
    tracing::trace!(
        status = tracing::field::display(release_metadata_post_response_status),
        "Got release metadata POST response"
    );

    let release_metadata_post_response_bytes = release_metadata_post_response
        .bytes()
        .await
        .wrap_err("Could not get bytes from release metadata POST response")?;
    let release_metadata_put_string =
        String::from_utf8_lossy(&release_metadata_post_response_bytes).to_string();

    if release_metadata_post_response_status != 200 {
        return Err(eyre!(
            "\
                Status {release_metadata_post_response_status} from metadata POST\n\
                {release_metadata_put_string}\
            "
        ));
    }

    let tarball_put_response = reqwest_client
        .put(release_metadata_put_string)
        .headers({
            let mut header_map = HeaderMap::new();
            header_map.insert(
                reqwest::header::CONTENT_LENGTH,
                reqwest::header::HeaderValue::from_str(&format!("{}", flake_tarball_len)).unwrap(),
            );
            header_map.insert(
                reqwest::header::HeaderName::from_static("x-amz-checksum-sha256"),
                reqwest::header::HeaderValue::from_str(&flake_tarball_hash_base64).unwrap(),
            );
            header_map
        })
        .body(flake_tarball)
        .send()
        .await
        .wrap_err("Sending tarball PUT")?;

    let tarball_put_response_status = tarball_put_response.status();
    tracing::trace!(
        status = tracing::field::display(release_metadata_post_response_status),
        "Got tarball PUT response"
    );
    if tarball_put_response_status != 200 {
        return Err(eyre!(
            "Got {tarball_put_response_status} status from PUT request"
        ));
    }

    tracing::info!("Successfully released new version of {project_owner}/{project_name}/{rolling_prefix_with_postfix_or_tag}");

    Ok(())
}

#[tracing::instrument(skip_all)]
async fn get_actions_id_bearer_token() -> color_eyre::Result<String> {
    let actions_id_token_request_token = std::env::var("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
        .wrap_err("\
            No `ACTIONS_ID_TOKEN_REQUEST_TOKEN` found, `nxfr-push` requires a JWT. To provide this, add `permissions` to your job, eg:\n\
            \n\
            # ...\n\
            jobs:\n\
              example:\n\
               runs-on: ubuntu-latest\n\
               permissions:\n\
                 id-token: write # In order to request a JWT for AWS auth\n\
                 contents: read # Specifying id-token wiped this out, so manually specify that this action is allowed to checkout this private repo\n\
               steps:\n\
               - uses: actions/checkout@v3\n\
               # ...\n\
        ")?;
    let actions_id_token_request_url = std::env::var("ACTIONS_ID_TOKEN_REQUEST_URL").wrap_err("`ACTIONS_ID_TOKEN_REQUEST_URL` required if `ACTIONS_ID_TOKEN_REQUEST_TOKEN` is also present")?;
    let actions_id_token_client = reqwest::Client::builder()
        .user_agent("nxfr-push")
        .default_headers(
            std::iter::once((
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!(
                    "Bearer {}",
                    actions_id_token_request_token
                ))
                .unwrap(),
            ))
            .collect(),
        )
        .build()?;
    let response = actions_id_token_client
        .get(format!(
            "{actions_id_token_request_url}&audience=api://AzureADTokenExchange"
        ))
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

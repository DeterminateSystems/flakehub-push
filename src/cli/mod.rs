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
    release_metadata::{DevMetadata, ReleaseMetadata},
    Visibility,
};

#[derive(Debug, clap::Parser)]
#[clap(version)]
pub(crate) struct NixfrPushCli {
    #[clap(
        long,
        env = "FLAKEHUB_PUSH_HOST",
        default_value = "https://api.flakehub.com"
    )]
    pub(crate) host: String,
    #[clap(long, env = "FLAKEHUB_PUSH_VISIBLITY")]
    pub(crate) visibility: crate::Visibility,
    // Will also detect `GITHUB_REF_NAME`
    #[clap(long, env = "FLAKEHUB_PUSH_TAG", value_parser = StringToNoneParser, default_value = "")]
    pub(crate) tag: OptionString,
    #[clap(long, env = "FLAKEHUB_PUSH_ROLLING_PREFIX", value_parser = StringToNoneParser, default_value = "")]
    pub(crate) rolling_prefix: OptionString,
    // Also detects `GITHUB_TOKEN`
    #[clap(long, env = "FLAKEHUB_PUSH_GITHUB_TOKEN", value_parser = StringToNoneParser, default_value = "")]
    pub(crate) github_token: OptionString,
    #[clap(long, env = "FLAKEHUB_PUSH_UPLOAD_NAME", value_parser = StringToNoneParser, default_value = "")]
    pub(crate) upload_name: OptionString,
    /// Will also detect `GITHUB_REPOSITORY`
    #[clap(long, env = "FLAKEHUB_PUSH_REPOSITORY", value_parser = StringToNoneParser, default_value = "")]
    pub(crate) repository: OptionString,
    // Also detects `GITHUB_WORKSPACE`
    #[clap(long, env = "FLAKEHUB_PUSH_DIRECTORY", value_parser = PathBufToNoneParser, default_value = "")]
    pub(crate) directory: OptionPathBuf,
    // Also detects `GITHUB_WORKSPACE`
    #[clap(long, env = "FLAKEHUB_PUSH_GIT_ROOT", value_parser = PathBufToNoneParser, default_value = "")]
    pub(crate) git_root: OptionPathBuf,
    // If the repository is mirrored via DeterminateSystems' mirror functionality
    //
    // This should only be used by DeterminateSystems
    #[clap(long, env = "FLAKEHUB_PUSH_MIRROR", default_value_t = false)]
    pub(crate) mirror: bool,
    #[cfg(debug_assertions)]
    #[clap(flatten)]
    pub(crate) dev_config: DevConfig,

    #[clap(flatten)]
    pub instrumentation: instrumentation::Instrumentation,
}

#[cfg(debug_assertions)]
#[derive(Debug, clap::Parser)]
pub struct DevConfig {
    // A specific bearer token string which bypasses the normal authentication check in when pushing an upload
    // This is intended for development at this time.
    #[clap(long, env = "FLAKEHUB_PUSH_DEV_BEARER_TOKEN", value_parser = StringToNoneParser, default_value = "")]
    pub(crate) dev_bearer_token: OptionString,
    // A manually-specified project id (a la GitHub's `databaseId`)
    #[clap(long)]
    pub(crate) dev_project_id: Option<String>,
    // A manually-specified owner id (a la GitHub's `databaseId`)
    #[clap(long)]
    pub(crate) dev_owner_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct OptionString(pub Option<String>);

#[derive(Clone)]
struct StringToNoneParser;

impl clap::builder::TypedValueParser for StringToNoneParser {
    type Value = OptionString;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let inner = clap::builder::StringValueParser::new();
        let val = inner.parse_ref(cmd, arg, value)?;

        if val.is_empty() {
            Ok(OptionString(None))
        } else {
            Ok(OptionString(Some(Into::<String>::into(val))))
        }
    }
}

#[derive(Clone, Debug)]
pub struct OptionPathBuf(pub Option<PathBuf>);

#[derive(Clone)]
struct PathBufToNoneParser;

impl clap::builder::TypedValueParser for PathBufToNoneParser {
    type Value = OptionPathBuf;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let inner = clap::builder::StringValueParser::new();
        let val = inner.parse_ref(cmd, arg, value)?;

        if val.is_empty() {
            Ok(OptionPathBuf(None))
        } else {
            Ok(OptionPathBuf(Some(Into::<PathBuf>::into(val))))
        }
    }
}

impl NixfrPushCli {
    #[tracing::instrument(
        name = "flakehub_push"
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
            repository,
            git_root,
            mirror,
            #[cfg(debug_assertions)]
            dev_config,
            instrumentation: _,
        } = self;

        let github_token = if let Some(github_token) = &github_token.0 {
            github_token.clone()
        } else {
            std::env::var("GITHUB_TOKEN")
                .wrap_err("Could not determine Github token, pass `--github-token`, or set either `FLAKEHUB_PUSH_GITHUB_TOKEN` or `GITHUB_TOKEN`")?
        };

        let git_root = if let Some(git_root) = &git_root.0 {
            git_root.clone()
        } else if let Ok(github_workspace) = std::env::var("GITHUB_WORKSPACE") {
            tracing::trace!(%github_workspace, "Got `GITHUB_WORKSPACE`");
            PathBuf::from(github_workspace)
        } else {
            std::env::current_dir().map(PathBuf::from).wrap_err("Could not determine current git_root. Pass `--git-root` or set `FLAKEHUB_PUSH_GIT_ROOT`")?
        };

        let directory = if let Some(directory) = &directory.0 {
            directory.clone()
        } else {
            git_root.clone()
        };

        let repository = if let Some(repository) = &repository.0 {
            tracing::trace!(%repository, "Got `--repository` argument");
            repository.clone()
        } else if let Ok(github_repository) = std::env::var("GITHUB_REPOSITORY") {
            tracing::trace!(
                %github_repository,
                "Got `GITHUB_REPOSITORY` environment"
            );
            github_repository
        } else {
            return Err(eyre!("Could not determine repository name, pass `--repository` or the `GITHUB_REPOSITORY` formatted like `determinatesystems/flakehub-push`"));
        };

        let tag = if let Some(tag) = &tag.0 {
            Some(tag.clone())
        } else {
            std::env::var("GITHUB_REF_NAME").ok()
        };

        push_new_release(
            &host,
            &github_token,
            &directory,
            &git_root,
            &repository,
            upload_name.0.as_deref(),
            mirror,
            visibility,
            tag.as_deref(),
            rolling_prefix.0.as_deref(),
            #[cfg(debug_assertions)]
            dev_config,
        )
        .await?;

        Ok(ExitCode::SUCCESS)
    }
}

#[tracing::instrument(
    skip_all,
    fields(
        repository = %repository,
        upload_name = tracing::field::Empty,
        mirror = %mirror,
        tag = tracing::field::Empty,
        source = tracing::field::Empty,
        mirrored = tracing::field::Empty,
    )
)]
#[allow(clippy::too_many_arguments)]
async fn push_new_release(
    host: &str,
    github_token: &str,
    directory: &Path,
    git_root: &Path,
    repository: &str,
    upload_name: Option<&str>,
    mirror: bool,
    visibility: Visibility,
    tag: Option<&str>,
    rolling_prefix: Option<&str>,
    #[cfg(debug_assertions)] dev_config: DevConfig,
) -> color_eyre::Result<()> {
    let span = tracing::Span::current();
    let upload_name = upload_name.unwrap_or(repository);
    span.record("upload_name", tracing::field::display(upload_name));

    let rolling_prefix_or_tag = rolling_prefix.or(tag).ok_or_else(|| {
        eyre!("Could not determine tag or rolling prefix, `--tag`, `GITHUB_REF_NAME`, or `--rolling-prefix` must be set")
    })?;

    tracing::info!("Preparing release of {upload_name}/{rolling_prefix_or_tag}");

    let tempdir = tempfile::Builder::new()
        .prefix("flakehub_push")
        .tempdir()
        .wrap_err("Creating tempdir")?;

    let github_api_client = reqwest::Client::builder()
        .user_agent("flakehub-push")
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
        repository,
        upload_name,
        mirror,
        visibility,
        #[cfg(debug_assertions)]
        DevMetadata {
            project_id: dev_config.dev_project_id,
            owner_id: dev_config.dev_owner_id,
        },
    )
    .await
    .wrap_err("Building release metadata")?;

    #[cfg(debug_assertions)]
    let upload_bearer_token = match &dev_config.dev_bearer_token.0 {
        None => "bearer bogus".to_string(),
        Some(dev_token) => {
            tracing::warn!(dev_bearer_token = %dev_token, "This flakehub-push has `dev_bearer_token` set for upload. This is intended for development purposes only.");
            dev_token.to_string()
        }
    };
    #[cfg(not(debug_assertions))]
    let upload_bearer_token = get_actions_id_bearer_token()
        .await
        .wrap_err("Getting upload bearer token")?;

    let reqwest_client = reqwest::Client::builder()
        .user_agent("flakehub-push")
        .build()?;

    let rolling_prefix_with_postfix_or_tag = if let Some(rolling_prefix) = &rolling_prefix {
        format!(
            "{rolling_prefix}.{}+rev-{}",
            release_metadata.commit_count, release_metadata.revision
        )
    } else {
        rolling_prefix_or_tag.to_string() // This will always be the tag since `self.rolling_prefix` was empty.
    };

    let release_metadata_post_url = format!(
        "{host}/upload/{upload_name}/{rolling_prefix_with_postfix_or_tag}/{flake_tarball_len}/{flake_tarball_hash_base64}"
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

    tracing::info!(
        "Successfully released new version of {upload_name}/{rolling_prefix_with_postfix_or_tag}"
    );

    Ok(())
}

#[tracing::instrument(skip_all)]
async fn get_actions_id_bearer_token() -> color_eyre::Result<String> {
    let actions_id_token_request_token = std::env::var("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
        .wrap_err("\
            No `ACTIONS_ID_TOKEN_REQUEST_TOKEN` found, `flakehub-push` requires a JWT. To provide this, add `permissions` to your job, eg:\n\
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
        .user_agent("flakehub-push")
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

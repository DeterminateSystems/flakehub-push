mod instrumentation;

use color_eyre::eyre::{eyre, WrapErr};
use reqwest::{header::HeaderMap, StatusCode};
use std::{
    path::{Path, PathBuf},
    process::ExitCode,
    str::FromStr,
};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{
    error::Error, flake_info::{check_flake_evaluates, get_flake_metadata, get_flake_outputs, get_flake_tarball}, graphql::{GithubGraphqlDataQuery, GithubGraphqlDataResult}, release_metadata::{ReleaseMetadata, RevisionInfo}, Visibility
};

const DEFAULT_ROLLING_PREFIX: &str = "0.1";

#[derive(Debug, clap::Parser)]
#[clap(version)]
pub(crate) struct FlakeHubPushCli {
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
    #[clap(long, env = "FLAKEHUB_PUSH_ROLLING_MINOR", value_parser = U64ToNoneParser, default_value = "")]
    pub(crate) rolling_minor: OptionU64,
    #[clap(long, env = "FLAKEHUB_PUSH_ROLLING", value_parser = EmptyBoolParser, default_value_t = false)]
    pub(crate) rolling: bool,
    // Also detects `GITHUB_TOKEN`
    #[clap(long, env = "FLAKEHUB_PUSH_GITHUB_TOKEN", value_parser = StringToNoneParser, default_value = "")]
    pub(crate) github_token: OptionString,
    #[clap(long, env = "FLAKEHUB_PUSH_NAME", value_parser = StringToNoneParser, default_value = "")]
    pub(crate) name: OptionString,
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
    /// URL of a JWT mock server (like https://github.com/ruiyang/jwt-mock-server) which can issue tokens.
    ///
    /// Used instead of ACTIONS_ID_TOKEN_REQUEST_URL/ACTIONS_ID_TOKEN_REQUEST_TOKEN when developing locally.
    #[clap(long, env = "FLAKEHUB_PUSH_JWT_ISSUER_URI", value_parser = StringToNoneParser, default_value = "")]
    pub(crate) jwt_issuer_uri: OptionString,

    /// User-supplied labels beyond those associated with the GitHub repository.
    #[clap(
        long,
        short = 'l',
        env = "FLAKEHUB_PUSH_EXTRA_LABELS",
        use_value_delimiter = true,
        value_delimiter = ','
    )]
    pub(crate) extra_labels: Vec<String>,

    /// DEPRECATED: Please use `extra-labels` instead.
    #[clap(
        long,
        short = 't',
        env = "FLAKEHUB_PUSH_EXTRA_TAGS",
        use_value_delimiter = true,
        value_delimiter = ','
    )]
    pub(crate) extra_tags: Vec<String>,

    /// An SPDX expression that overrides that which is returned from GitHub.
    #[clap(
        long,
        env = "FLAKEHUB_PUSH_SPDX_EXPRESSION",
        value_parser = SpdxToNoneParser,
        default_value = ""
    )]
    pub(crate) spdx_expression: OptionSpdxExpression,

    #[clap(
        long,
        env = "FLAKEHUB_PUSH_ERROR_ON_CONFLICT",
        value_parser = EmptyBoolParser,
        default_value_t = false
    )]
    pub(crate) error_on_conflict: bool,

    #[clap(flatten)]
    pub instrumentation: instrumentation::Instrumentation,

    #[clap(long, env = "FLAKEHUB_PUSH_INCLUDE_OUTPUT_PATHS", value_parser = EmptyBoolParser, default_value_t = false)]
    pub(crate) include_output_paths: bool,
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

#[derive(Clone, Debug)]
pub struct OptionSpdxExpression(pub Option<spdx::Expression>);

#[derive(Clone)]
struct SpdxToNoneParser;

impl clap::builder::TypedValueParser for SpdxToNoneParser {
    type Value = OptionSpdxExpression;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let inner = clap::builder::StringValueParser::new();
        let val = inner.parse_ref(cmd, arg, value)?;

        if val.is_empty() {
            Ok(OptionSpdxExpression(None))
        } else {
            let expression = spdx::Expression::parse(&val).map_err(|e| {
                clap::Error::raw(clap::error::ErrorKind::ValueValidation, format!("{e}"))
            })?;
            Ok(OptionSpdxExpression(Some(expression)))
        }
    }
}

#[derive(Clone)]
struct EmptyBoolParser;

impl clap::builder::TypedValueParser for EmptyBoolParser {
    type Value = bool;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let inner = clap::builder::StringValueParser::new();
        let val = inner.parse_ref(cmd, arg, value)?;

        if val.is_empty() {
            Ok(false)
        } else {
            let val = match val.as_ref() {
                "true" => true,
                "false" => false,
                v => {
                    return Err(clap::Error::raw(
                        clap::error::ErrorKind::InvalidValue,
                        format!("`{v}` was not `true` or `false`\n"),
                    ))
                }
            };
            Ok(val)
        }
    }
}

#[derive(Clone, Debug)]
pub struct OptionU64(pub Option<u64>);

#[derive(Clone)]
struct U64ToNoneParser;

impl clap::builder::TypedValueParser for U64ToNoneParser {
    type Value = OptionU64;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let inner = clap::builder::StringValueParser::new();
        let val = inner.parse_ref(cmd, arg, value)?;

        if val.is_empty() {
            Ok(OptionU64(None))
        } else {
            let expression = val.parse::<u64>().map_err(|e| {
                clap::Error::raw(clap::error::ErrorKind::ValueValidation, format!("{e}\n"))
            })?;
            Ok(OptionU64(Some(expression)))
        }
    }
}

fn build_http_client() -> reqwest::ClientBuilder {
    reqwest::Client::builder().user_agent("flakehub-push")
}

impl FlakeHubPushCli {
    #[tracing::instrument(
        name = "flakehub_push"
        skip_all,
    )]
    pub(crate) async fn execute(self) -> color_eyre::Result<std::process::ExitCode> {
        tracing::trace!(?self, "Executing");
        let Self {
            host,
            visibility,
            name,
            tag,
            rolling,
            rolling_minor,
            github_token,
            directory,
            repository,
            git_root,
            mirror,
            jwt_issuer_uri,
            instrumentation: _,
            extra_labels,
            spdx_expression,
            extra_tags,
            error_on_conflict,
            include_output_paths,
        } = self;

        let mut extra_labels: Vec<_> = extra_labels.into_iter().filter(|v| !v.is_empty()).collect();
        let extra_tags: Vec<_> = extra_tags.into_iter().filter(|v| !v.is_empty()).collect();

        let is_github_actions = std::env::var("GITHUB_ACTION").ok().is_some();
        if !extra_tags.is_empty() {
            let message = "`extra-tags` is deprecated and will be removed in the future. Please use `extra-labels` instead.";
            tracing::warn!("{message}");

            if is_github_actions {
                println!("::warning::{message}");
            }

            if extra_labels.is_empty() {
                extra_labels = extra_tags;
            } else {
                let message =
                    "Both `extra-tags` and `extra-labels` were set; `extra-tags` will be ignored.";
                tracing::warn!("{message}");

                if is_github_actions {
                    println!("::warning::{message}");
                }
            }
        }

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

        let git_root = git_root
            .canonicalize()
            .wrap_err("Failed to canonicalize `--git-root` argument")?;

        let subdir = if let Some(directory) = &directory.0 {
            let absolute_directory = if directory.is_absolute() {
                directory.clone()
            } else {
                git_root.join(directory)
            };
            let canonical_directory = absolute_directory
                .canonicalize()
                .wrap_err("Failed to canonicalize `--directory` argument")?;

            Path::new(
                canonical_directory
                    .strip_prefix(git_root.clone())
                    .wrap_err(
                        "Specified `--directory` was not a directory inside the `--git-root`",
                    )?,
            )
            .into()
        } else {
            PathBuf::new()
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

        // If the upload name is supplied by the user, ensure that it contains exactly
        // one slash and no whitespace. Default to the repository name.
        let upload_name = if let Some(name) = name.0 {
            let num_slashes = name.matches('/').count();

            if num_slashes == 0
                || num_slashes > 1
                || !name.is_ascii()
                || name.contains(char::is_whitespace)
            {
                return Err(eyre!("The `upload-name` must be in the format of `owner-name/repo-name` and cannot contain whitespace or other special characters"));
            } else {
                name
            }
        } else {
            repository.clone()
        };

        let tag = if let Some(tag) = &tag.0 {
            Some(tag.clone())
        } else {
            std::env::var("GITHUB_REF_NAME").ok()
        };

        let mut repository_split = repository.split('/');
        let project_owner = repository_split
            .next()
            .ok_or_else(|| eyre!("Could not determine owner, pass `--repository` or the `GITHUB_REPOSITORY` formatted like `determinatesystems/flakehub-push`"))?
            .to_string();
        let project_name = repository_split.next()
            .ok_or_else(|| eyre!("Could not determine project, pass `--repository` or `GITHUB_REPOSITORY` formatted like `determinatesystems/flakehub-push`"))?
            .to_string();
        if repository_split.next().is_some() {
            Err(eyre!("Could not determine the owner/project, pass `--repository` or `GITHUB_REPOSITORY` formatted like `determinatesystems/flakehub-push`. The passed value has too many slashes (/) to be a valid repository"))?;
        }

        let github_api_client = build_http_client().build()?;

        let revision_info = RevisionInfo::from_git_root(&git_root)?;
        let github_graphql_data_result = GithubGraphqlDataQuery::get(
            &github_api_client,
            &github_token,
            &project_owner,
            &project_name,
            &revision_info.revision,
        )
        .await?;

        let upload_bearer_token = match jwt_issuer_uri.0 {
            None => get_actions_id_bearer_token()
                .await
                .wrap_err("Getting upload bearer token from GitHub")?,

            Some(jwt_issuer_uri) => {
                let client = build_http_client().build()?;
                let mut claims = github_actions_oidc_claims::Claims::make_dummy();
                // FIXME: we should probably fill in more of these claims.
                claims.aud = "flakehub-localhost".to_string();
                claims.iss = "flakehub-push-dev".to_string();
                claims.repository = repository.clone();
                claims.repository_owner = project_owner.to_string();
                claims.repository_id = github_graphql_data_result.project_id.to_string();
                claims.repository_owner_id = github_graphql_data_result.owner_id.to_string();

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
                response_deserialized.token
            }
        };

        push_new_release(
            &host,
            &upload_bearer_token,
            &git_root,
            &subdir,
            revision_info,
            &repository,
            upload_name,
            mirror,
            visibility,
            tag,
            rolling,
            rolling_minor.0,
            github_graphql_data_result,
            extra_labels,
            spdx_expression.0,
            error_on_conflict,
            include_output_paths,
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
    upload_bearer_token: &str,
    flake_root: &Path,
    subdir: &Path,
    revision_info: RevisionInfo,
    repository: &str,
    upload_name: String,
    mirror: bool,
    visibility: Visibility,
    tag: Option<String>,
    rolling: bool,
    rolling_minor: Option<u64>,
    github_graphql_data_result: GithubGraphqlDataResult,
    extra_labels: Vec<String>,
    spdx_expression: Option<spdx::Expression>,
    error_if_release_conflicts: bool,
    include_output_paths: bool,
) -> color_eyre::Result<()> {
    let span = tracing::Span::current();
    span.record("upload_name", tracing::field::display(upload_name.clone()));

    let rolling_prefix_or_tag = match (rolling_minor.as_ref(), tag) {
        (Some(_), _) if !rolling => {
            return Err(eyre!(
                "You must enable `rolling` to upload a release with a specific `rolling-minor`."
            ));
        }
        (Some(minor), _) => format!("0.{minor}"),
        (None, _) if rolling => DEFAULT_ROLLING_PREFIX.to_string(),
        (None, Some(tag)) => {
            let version_only = tag.strip_prefix('v').unwrap_or(&tag);
            // Ensure the version respects semver
            semver::Version::from_str(version_only).wrap_err_with(|| eyre!("Failed to parse version `{tag}` as semver, see https://semver.org/ for specifications"))?;
            tag
        }
        (None, None) => {
            return Err(eyre!("Could not determine tag or rolling minor version, `--tag`, `GITHUB_REF_NAME`, or `--rolling-minor` must be set"));
        }
    };

    tracing::info!("Preparing release of {upload_name}/{rolling_prefix_or_tag}");

    let tempdir = tempfile::Builder::new()
        .prefix("flakehub_push")
        .tempdir()
        .wrap_err("Creating tempdir")?;

    let flake_dir = flake_root.join(subdir);

    check_flake_evaluates(&flake_dir)
        .await
        .wrap_err("Checking flake evaluates")?;
    let flake_metadata = get_flake_metadata(&flake_dir)
        .await
        .wrap_err("Getting flake metadata")?;
    tracing::debug!("Got flake metadata: {:?}", flake_metadata);

    // FIXME: bail out if flake_metadata denotes a dirty tree.

    let flake_locked_url = flake_metadata
        .get("url")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            eyre!("Could not get `url` attribute from `nix flake metadata --json` output")
        })?;
    tracing::debug!("Locked URL = {}", flake_locked_url);
    let flake_metadata_value_path = flake_metadata
        .get("path")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            eyre!("Could not get `path` attribute from `nix flake metadata --json` output")
        })?;
    let flake_metadata_value_resolved_dir = flake_metadata
        .pointer("/resolved/dir")
        .and_then(serde_json::Value::as_str);

    let flake_outputs = get_flake_outputs(flake_locked_url, include_output_paths).await?;
    tracing::debug!("Got flake outputs: {:?}", flake_outputs);

    let source = match flake_metadata_value_resolved_dir {
        Some(flake_metadata_value_resolved_dir) => {
            Path::new(flake_metadata_value_path).join(flake_metadata_value_resolved_dir)
        }
        None => PathBuf::from(flake_metadata_value_path),
    };
    span.record("source", tracing::field::display(source.clone().display()));
    tracing::debug!("Found source");

    if flake_dir.join("flake.lock").exists() {
        let output = tokio::process::Command::new("nix")
            .arg("flake")
            .arg("metadata")
            .arg("--json")
            .arg("--no-update-lock-file")
            .arg(&flake_dir)
            .output()
            .await
            .wrap_err_with(|| {
                eyre!(
                    "Failed to execute `nix flake metadata --json --no-update-lock-file {}`",
                    flake_dir.display()
                )
            })?;

        if !output.status.success() {
            let command = format!(
                "nix flake metadata --json --no-update-lock-file {}",
                flake_dir.display(),
            );
            let msg = format!(
                "\
                Failed to execute command `{command}`{maybe_status} \n\
                stdout: {stdout}\n\
                stderr: {stderr}\n\
                ",
                stdout = String::from_utf8_lossy(&output.stdout),
                stderr = String::from_utf8_lossy(&output.stderr),
                maybe_status = if let Some(status) = output.status.code() {
                    format!(" with status {status}")
                } else {
                    String::new()
                }
            );
            return Err(eyre!(msg))?;
        }
    }

    let last_modified = if let Some(last_modified) = flake_metadata.get("lastModified") {
        last_modified.as_u64().ok_or_else(|| {
            eyre!("`nix flake metadata --json` does not have a integer `lastModified` field")
        })?
    } else {
        return Err(eyre!(
            "`nix flake metadata` did not return a `lastModified` attribute"
        ));
    };
    tracing::debug!("lastModified = {}", last_modified);

    let flake_tarball = get_flake_tarball(&source, last_modified)
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

    let release_metadata = ReleaseMetadata::build(
        &source,
        subdir,
        revision_info,
        flake_metadata,
        flake_outputs,
        upload_name.clone(),
        mirror,
        visibility,
        github_graphql_data_result,
        extra_labels,
        spdx_expression,
    )
    .await
    .wrap_err("Building release metadata")?;

    let flakehub_client = build_http_client().build()?;

    let rolling_minor_with_postfix_or_tag = if rolling_minor.is_some() || rolling {
        format!(
            "{rolling_prefix_or_tag}.{}+rev-{}",
            release_metadata.commit_count, release_metadata.revision
        )
    } else {
        rolling_prefix_or_tag.to_string() // This will always be the tag since `self.rolling_prefix` was empty.
    };

    let release_metadata_post_url = format!(
        "{host}/upload/{upload_name}/{rolling_minor_with_postfix_or_tag}/{flake_tarball_len}/{flake_tarball_hash_base64}"
    );
    tracing::debug!(
        url = release_metadata_post_url,
        "Computed release metadata POST URL"
    );

    let flakehub_headers = {
        let mut header_map = HeaderMap::new();

        header_map.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_str("application/json").unwrap(),
        );
        header_map.insert(
            reqwest::header::HeaderName::from_static("ngrok-skip-browser-warning"),
            reqwest::header::HeaderValue::from_str("please").unwrap(),
        );
        header_map
    };

    let release_metadata_post_response = flakehub_client
        .post(release_metadata_post_url)
        .bearer_auth(upload_bearer_token)
        .headers(flakehub_headers.clone())
        .json(&release_metadata)
        .send()
        .await
        .wrap_err("Sending release metadata")?;

    let release_metadata_post_response_status = release_metadata_post_response.status();
    tracing::trace!(
        status = tracing::field::display(release_metadata_post_response_status),
        "Got release metadata POST response"
    );

    match release_metadata_post_response_status {
        StatusCode::OK => (),
        StatusCode::CONFLICT => {
            tracing::info!(
                "Release for revision `{revision}` of {upload_name}/{rolling_prefix_or_tag} already exists; flakehub-push will not upload it again",
                revision = release_metadata.revision
            );
            if error_if_release_conflicts {
                return Err(Error::Conflict { upload_name, rolling_prefix_or_tag })?;
            } else {
                return Ok(());
            }
        },
        StatusCode::UNAUTHORIZED => {
            let body = &release_metadata_post_response.bytes().await?;
            let message = serde_json::from_slice::<String>(body)?;
            return Err(Error::Unauthorized(message.into()))?;
        },
        _ => {
            let body = &release_metadata_post_response.bytes().await?;
            let message = serde_json::from_slice::<String>(body)?;
            return Err(eyre!(
                "\
                Status {release_metadata_post_response_status} from metadata POST\n\
                {}\
            ",
                message
            ));
        }
    }

    #[derive(serde::Deserialize)]
    struct Result {
        s3_upload_url: String,
        uuid: Uuid,
    }

    let release_metadata_post_result: Result = release_metadata_post_response
        .json()
        .await
        .wrap_err("Decoding release metadata POST response")?;

    let tarball_put_response = flakehub_client
        .put(release_metadata_post_result.s3_upload_url)
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
            header_map.insert(
                reqwest::header::CONTENT_TYPE,
                reqwest::header::HeaderValue::from_str("application/gzip").unwrap(),
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
    if !tarball_put_response_status.is_success() {
        return Err(eyre!(
            "Got {tarball_put_response_status} status from PUT request"
        ));
    }

    // Make the release we just uploaded visible.
    let publish_post_url = format!("{host}/publish/{}", release_metadata_post_result.uuid);
    tracing::debug!(url = publish_post_url, "Computed publish POST URL");

    let publish_response = flakehub_client
        .post(publish_post_url)
        .bearer_auth(upload_bearer_token)
        .headers(flakehub_headers)
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

    tracing::info!(
        "Successfully released new version of {upload_name}/{rolling_minor_with_postfix_or_tag}"
    );

    Ok(())
}

#[tracing::instrument(skip_all)]
async fn get_actions_id_bearer_token() -> color_eyre::Result<String> {
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
            "{actions_id_token_request_url}&audience=api.flakehub.com"
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

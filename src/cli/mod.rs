mod instrumentation;

use color_eyre::eyre::{eyre, WrapErr};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::ExitCode,
};

use crate::{
    build_http_client,
    github::{
        get_actions_id_bearer_token,
        graphql::{GithubGraphqlDataQuery, MAX_LABEL_LENGTH, MAX_NUM_TOTAL_LABELS},
    },
    push::push_new_release,
    release_metadata::RevisionInfo,
};

#[derive(Debug, clap::Parser)]
#[clap(version)]
pub(crate) struct FlakeHubPushCli {
    #[clap(
        long,
        env = "FLAKEHUB_PUSH_HOST",
        default_value = "https://api.flakehub.com"
    )]
    pub(crate) host: url::Url,
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

impl FlakeHubPushCli {
    #[tracing::instrument(skip_all)]
    pub(crate) fn populate_missing_from_github(&mut self) {
        if self.git_root.0.is_none() {
            let env_key = "GITHUB_WORKSPACE";
            if let Ok(env_val) = std::env::var(env_key) {
                tracing::debug!(git_root = %env_val, "Set via `${env_key}`");
                self.git_root.0 = Some(PathBuf::from(env_val));
            }
        }

        if self.repository.0.is_none() {
            let env_key = "GITHUB_REPOSITORY";
            if let Ok(env_val) = std::env::var(env_key) {
                tracing::debug!(repository = %env_val, "Set via `${env_key}`");
                self.repository.0 = Some(env_val);
            }
        }

        if self.tag.0.is_none() {
            let env_key = "GITHUB_REF_NAME";
            if let Ok(env_val) = std::env::var(env_key) {
                tracing::debug!(repository = %env_val, "Set via `${env_key}`");
                self.tag.0 = Some(env_val);
            }
        }
    }

    #[tracing::instrument(
        name = "flakehub_push"
        skip_all,
        fields(
            host = %self.host,
            visibility = ?self.visibility,
            name = self.name.0,
            tag = tracing::field::Empty,
            rolling_minor = tracing::field::Empty,
            rolling = self.rolling,
            directory = tracing::field::Empty,
            repository = tracing::field::Empty,
            git_root = tracing::field::Empty,
            commit_count = tracing::field::Empty,
            mirror = self.mirror,
            jwt_issuer_uri = tracing::field::Empty,
            extra_labels = self.extra_labels.join(","),
            spdx_expression = tracing::field::Empty,
            error_on_conflict = self.error_on_conflict,
            include_output_paths = self.include_output_paths,
        )
    )]
    pub(crate) async fn execute(mut self) -> color_eyre::Result<std::process::ExitCode> {
        let span = tracing::Span::current();
        tracing::trace!("Executing");

        let is_github_actions =
            self.github_token.0.is_some() || std::env::var("GITHUB_ACTION").ok().is_some();
        if is_github_actions {
            tracing::debug!("Running inside Github Actions, will enrich with GitHub API data and push with authorized Github bearer token");
            self.populate_missing_from_github()
        }

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

        let mut labels: HashSet<_> = extra_labels.into_iter().filter(|v| !v.is_empty()).collect();
        let extra_tags: HashSet<_> = extra_tags.into_iter().filter(|v| !v.is_empty()).collect();

        if !extra_tags.is_empty() {
            let message = "`extra-tags` is deprecated and will be removed in the future. Please use `extra-labels` instead.";
            tracing::warn!("{message}");

            if is_github_actions {
                println!("::warning::{message}");
            }

            if labels.is_empty() {
                labels = extra_tags;
            } else {
                let message =
                    "Both `extra-tags` and `extra-labels` were set; `extra-tags` will be ignored.";
                tracing::warn!("{message}");

                if is_github_actions {
                    println!("::warning::{message}");
                }
            }
        }

        let git_root = if let Some(git_root) = git_root.0 {
            git_root.clone()
        } else {
            std::env::current_dir().map(PathBuf::from).wrap_err("Could not determine current `git_root`. Pass `--git-root` or set `FLAKEHUB_PUSH_GIT_ROOT`, or run `flakehub-push` with the git root as the current working directory")?
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

        let Some(repository) = repository.0 else {
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

        let mut spdx_expression = spdx_expression.0;

        #[allow(unused_assignments)]
        // Since we return an error outside github actions right now, `commit_count` throws an unused warning since we don't actually use it.
        let RevisionInfo {
            mut commit_count,
            revision,
        } = RevisionInfo::from_git_root(&git_root)?;

        let upload_bearer_token = if is_github_actions {
            let github_token = if let Some(github_token) = &github_token.0 {
                github_token.clone()
            } else {
                std::env::var("GITHUB_TOKEN")
                    .wrap_err("Could not determine Github token, pass `--github-token`, or set either `FLAKEHUB_PUSH_GITHUB_TOKEN` or `GITHUB_TOKEN`")?
            };
            let github_api_client = build_http_client().build()?;

            // Take the opportunity to be able to populate/encrich data from the GitHub API since we need it for project/owner_id anywys
            let github_graphql_data_result = GithubGraphqlDataQuery::get(
                &github_api_client,
                &github_token,
                &project_owner,
                &project_name,
                &revision,
            )
            .await?;

            // On GitHub Actions, typically shallow clones are used which would report 1 for the commit count. Override it with the result from the API.
            // Since users can't set this as a command line flag, that's fine.
            tracing::trace!(
                "Updating `commit_count` from {} to {} via GitHub API",
                commit_count
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "<none>".into()),
                github_graphql_data_result.rev_count as usize
            );
            commit_count = Some(github_graphql_data_result.rev_count as usize);

            // If the user didn't pass `--spdx-expression` from command line, enrich it with Github's data.
            spdx_expression = if spdx_expression.is_none() {
                if let Some(spdx_string) = &github_graphql_data_result.spdx_identifier {
                    tracing::debug!("Recieved SPDX identifier `{}` from GitHub API", spdx_string);
                    let parsed = spdx::Expression::parse(spdx_string)
                        .wrap_err("Invalid SPDX license identifier reported from the GitHub API, either you are using a non-standard license or GitHub has returned a value that cannot be validated")?;
                    span.record("spdx_expression", tracing::field::display(&parsed));
                    Some(parsed)
                } else {
                    None
                }
            } else {
                // Provide the user notice if the SPDX expression passed differs from the one detected on GitHub -- It's probably something they care about.
                if github_graphql_data_result.spdx_identifier
                    != spdx_expression.as_ref().map(|v| v.to_string())
                {
                    tracing::warn!(
                        "SPDX identifier `{}` was passed via argument, but GitHub's API suggests it may be `{}`",
                        spdx_expression.as_ref().map(|v| v.to_string()).unwrap_or_else(|| "None".to_string()),
                        github_graphql_data_result.spdx_identifier.unwrap_or_else(|| "None".to_string()),
                    )
                }
                spdx_expression
            };

            // Extend the labels provided by the user with those from GitHub.
            labels = labels
                .into_iter()
                .chain(github_graphql_data_result.topics.into_iter())
                .collect::<HashSet<String>>();

            match jwt_issuer_uri.0 {
                None => get_actions_id_bearer_token(&host)
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
            }
        } else {
            return Err(eyre!(
                "`flakehub-push` currently only runs inside Github Actions"
            ));
        };

        // Here we merge explicitly user-supplied labels and the labels ("topics")
        // associated with the repo. Duplicates are excluded and all
        // are converted to lower case.
        let labels: Vec<String> = labels
            .into_iter()
            .take(MAX_NUM_TOTAL_LABELS)
            .map(|s| s.trim().to_lowercase())
            .filter(|t: &String| {
                !t.is_empty()
                    && t.len() <= MAX_LABEL_LENGTH
                    && t.chars().all(|c| c.is_alphanumeric() || c == '-')
            })
            .collect();

        let Some(commit_count) = commit_count else {
            return Err(eyre!("Did not get `commit_count`"));
        };

        push_new_release(
            &host,
            &upload_bearer_token,
            &git_root,
            &subdir,
            revision,
            commit_count,
            upload_name,
            mirror,
            visibility,
            tag.0,
            rolling,
            rolling_minor.0,
            labels,
            spdx_expression,
            error_on_conflict,
            include_output_paths,
        )
        .await?;

        Ok(ExitCode::SUCCESS)
    }
}

mod instrumentation;

use std::path::{Path, PathBuf};
use std::str::FromStr as _;

use color_eyre::eyre::{eyre, Context as _, Result};

use crate::git_context::GitContext;
use crate::push_context::ExecutionEnvironment;
use crate::{Visibility, DEFAULT_ROLLING_PREFIX};

#[derive(Debug, clap::Parser)]
#[clap(version)]
pub(crate) struct FlakeHubPushCli {
    #[clap(
        long,
        env = "FLAKEHUB_PUSH_HOST",
        default_value = "https://api.flakehub.com"
    )]
    pub(crate) host: url::Url,

    #[clap(long, env = "FLAKEHUB_PUSH_VISIBILITY")]
    pub(crate) visibility: Option<crate::Visibility>,
    // This was the original env var to set this value. As you can see, we previously misspelled it.
    // We need to continue to support it just in case.
    #[clap(long, env = "FLAKEHUB_PUSH_VISIBLITY")]
    pub(crate) visibility_alt: Option<crate::Visibility>,

    // Will also detect `GITHUB_REF_NAME`
    #[clap(long, env = "FLAKEHUB_PUSH_TAG", value_parser = StringToNoneParser, default_value = "")]
    pub(crate) tag: OptionString,
    #[clap(long, env = "FLAKEHUB_PUSH_REV", value_parser = StringToNoneParser, default_value = "")]
    pub(crate) rev: OptionString,
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

    /// URL of a JWT mock server (like https://github.com/spectare/fakeidp) which can issue tokens.
    #[clap(long)]
    pub(crate) jwt_issuer_uri: Option<String>,

    /// User-supplied labels, merged with any associated with GitHub repository (if possible)
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

    /// An SPDX identifier from https://spdx.org/licenses/, inferred from GitHub (if possible)
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

    /// Do less work on extremely large flakes.
    ///
    /// This flag is intended to limit the scope of evaluations which are too large to complete on one machine.
    /// This flag should NOT be used to paper over evaluation errors across different architectures.
    ///
    /// Please do not turn this flag on without opening an issue to decide if it applies to your scenario.
    ///
    /// Note: the behavior of this flag could change at any time, please don't count on it for anything specific.
    #[clap(
      long,
      env = "FLAKEHUB_PUSH_MY_FLAKE_IS_TOO_BIG",
      value_parser = EmptyBoolParser,
      default_value_t = false
    )]
    pub(crate) my_flake_is_too_big: bool,

    #[clap(flatten)]
    pub instrumentation: instrumentation::Instrumentation,

    #[clap(long, env = "FLAKEHUB_PUSH_INCLUDE_OUTPUT_PATHS", value_parser = EmptyBoolParser, default_value_t = false)]
    pub(crate) include_output_paths: bool,

    // Gitlab has a concept of subgroups, which enables repo names like https://gitlab.com/a/b/c/d/e/f/g. By default,
    // flakehub-push would parse that to flake name `a/b-c-d-e-f-g`. This flag/environment variable provides a
    // mechanism to disable this behavior.
    #[clap(
        long,
        env = "FLAKEHUB_PUSH_DISABLE_RENAME_SUBGROUPS",
        default_value_t = false
    )]
    pub(crate) disable_rename_subgroups: bool,

    /// Write the tarball to a directory instead of pushing it to FlakeHub.
    #[clap(long, env = "FLAKEHUB_DEST_DIR", value_parser = PathBufToNoneParser, default_value = "")]
    pub(crate) dest_dir: OptionPathBuf,
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
    pub(crate) fn backfill_from_github_env(&mut self) {
        // https://docs.github.com/en/actions/learn-github-actions/variables

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

    pub(crate) fn backfill_from_gitlab_env(&mut self) {
        // https://docs.gitlab.com/ee/ci/variables/predefined_variables.html

        if self.git_root.0.is_none() {
            let env_key: &str = "CI_PROJECT_DIR";
            if let Ok(env_val) = std::env::var(env_key) {
                tracing::debug!(git_root = %env_val, "Set via `${env_key}`");
                self.git_root.0 = Some(PathBuf::from(env_val));
            }
        }

        if self.repository.0.is_none() {
            let env_key = "CI_PROJECT_ID";
            if let Ok(env_val) = std::env::var(env_key) {
                tracing::debug!(repository = %env_val, "Set via `${env_key}`");
                self.repository.0 = Some(env_val);
            }
        }

        // TODO(review): this... isn't really a "tag" for github either, but I think maybe that's intentional?
        if self.tag.0.is_none() {
            let env_key = "CI_COMMIT_REF_NAME";
            if let Ok(env_val) = std::env::var(env_key) {
                tracing::debug!(repository = %env_val, "Set via `${env_key}`");
                self.tag.0 = Some(env_val);
            }
        }
    }

    pub(crate) fn execution_environment(&self) -> ExecutionEnvironment {
        if std::env::var("GITHUB_ACTION").ok().is_some() {
            ExecutionEnvironment::GitHub
        } else if std::env::var("GITLAB_CI").ok().is_some() {
            ExecutionEnvironment::GitLab
        } else if std::env::var("FLAKEHUB_PUSH_OIDC_TOKEN").ok().is_some() {
            ExecutionEnvironment::Generic
        } else {
            ExecutionEnvironment::LocalGitHub
        }
    }

    pub(crate) fn visibility(&self) -> Result<Visibility> {
        match (self.visibility_alt, self.visibility) {
            (Some(v), _) => Ok(v),
            (None, Some(v)) => Ok(v),
            (None, None) =>  Err(color_eyre::eyre::eyre!(
                "Could not determine the flake's desired visibility. Use `--visibility` to set this to one of the following: public, unlisted, private.",
            )),
        }
    }

    pub(crate) fn resolve_local_git_root(&self) -> Result<PathBuf> {
        let maybe_git_root = match &self.git_root.0 {
            Some(gr) => Ok(gr.to_owned()),
            None => std::env::current_dir(),
        };

        let local_git_root = maybe_git_root.wrap_err("Could not determine current `git_root`. Pass `--git-root` or set `FLAKEHUB_PUSH_GIT_ROOT`, or run `flakehub-push` with the git root as the current working directory")?;
        let local_git_root = local_git_root
            .canonicalize()
            .wrap_err("Failed to canonicalize `--git-root` argument")?;

        Ok(local_git_root)
    }

    pub(crate) fn subdir_from_git_root(&self, local_git_root: &Path) -> Result<PathBuf> {
        let subdir =
            if let Some(directory) = &self.directory.0 {
                let absolute_directory = if directory.is_absolute() {
                    directory.clone()
                } else {
                    local_git_root.join(directory)
                };
                let canonical_directory = absolute_directory
                    .canonicalize()
                    .wrap_err("Failed to canonicalize `--directory` argument")?;

                PathBuf::from(canonical_directory.strip_prefix(local_git_root).wrap_err(
                    "Specified `--directory` was not a directory inside the `--git-root`",
                )?)
            } else {
                PathBuf::new()
            };

        Ok(subdir)
    }

    pub(crate) fn release_version(&self, git_ctx: &GitContext) -> Result<String> {
        let rolling_prefix_or_tag = match (self.rolling_minor.0.as_ref(), &self.tag.0) {
            (Some(_), _) if !self.rolling => {
                return Err(eyre!(
                    "You must enable `rolling` to upload a release with a specific `rolling-minor`."
                ));
            }
            (Some(minor), _) => format!("0.{minor}"),
            (None, _) if self.rolling => DEFAULT_ROLLING_PREFIX.to_string(),
            (None, Some(tag)) => {
                let version_only = tag.strip_prefix('v').unwrap_or(tag);
                // Ensure the version respects semver
                semver::Version::from_str(version_only).wrap_err_with(|| eyre!("Failed to parse version `{tag}` as semver, see https://semver.org/ for specifications"))?;
                tag.to_string()
            }
            (None, None) => {
                return Err(eyre!("Could not determine tag or rolling minor version, `--tag`, `GITHUB_REF_NAME`, or `--rolling-minor` must be set"));
            }
        };

        let Some(commit_count) = git_ctx.revision_info.commit_count else {
            return Err(eyre!("Could not determine commit count, this is normally determined via the `--git-root` argument or via the GitHub API"));
        };

        let rolling_minor_with_postfix_or_tag = if self.rolling_minor.0.is_some() || self.rolling {
            format!(
                "{rolling_prefix_or_tag}.{}+rev-{}",
                commit_count, git_ctx.revision_info.revision
            )
        } else {
            rolling_prefix_or_tag.to_string() // This will always be the tag since `self.rolling_prefix` was empty.
        };

        Ok(rolling_minor_with_postfix_or_tag)
    }
}

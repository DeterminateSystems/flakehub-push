use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    str::FromStr,
};

use color_eyre::eyre::{eyre, Context, Result};

use crate::{
    build_http_client,
    cli::FlakeHubPushCli,
    flake_info, flakehub_auth_fake,
    flakehub_client::Tarball,
    git_context::GitContext,
    github::graphql::{GithubGraphqlDataQuery, MAX_LABEL_LENGTH, MAX_NUM_TOTAL_LABELS},
    release_metadata::ReleaseMetadata,
    revision_info::RevisionInfo,
    DEFAULT_ROLLING_PREFIX,
};

#[derive(Clone)]
pub enum ExecutionEnvironment {
    GitHub,
    GitLab,
    Local,
}

pub(crate) struct PushContext {
    pub(crate) flakehub_host: url::Url,
    pub(crate) auth_token: String,

    // url components
    pub(crate) upload_name: String, // {org}/{project}
    pub(crate) release_version: String,

    // internal behavior changes
    pub(crate) error_if_release_conflicts: bool,

    // the goods
    pub(crate) metadata: ReleaseMetadata,
    pub(crate) tarball: Tarball,
}

impl PushContext {
    pub async fn from_cli_and_env(cli: &mut FlakeHubPushCli) -> Result<Self> {
        // Take the opportunity to be able to populate/encrich data from the GitHub API
        // this is used to augment user/discovered data, and is used for the faked JWT for local flakehub-push testing

        let client = build_http_client().build()?;

        let exec_env = if std::env::var("GITHUB_ACTION").ok().is_some() {
            ExecutionEnvironment::GitHub
        } else if std::env::var("GITLAB_CI").ok().is_some() {
            ExecutionEnvironment::GitLab
        } else {
            ExecutionEnvironment::Local
        };

        match exec_env.clone() {
            ExecutionEnvironment::GitHub => {
                cli.backfill_from_github_env();
            }
            ExecutionEnvironment::GitLab => {
                cli.backfill_from_gitlab_env();
            }
            _ => {}
        };

        let visibility = match (cli.visibility_alt, cli.visibility) {
            (Some(v), _) => v,
            (None, Some(v)) => v,
            (None, None) => return Err(eyre!(
                "Could not determine the flake's desired visibility. Use `--visibility` to set this to one of the following: public, unlisted, private.",
            )),
        };

        // STEP: determine and check 'repository' and 'upload_name'
        // If the upload name is supplied by the user, ensure that it contains exactly
        // one slash and no whitespace. Default to the repository name.
        // notes for future readers:
        // upload_name is derived from repository, unless set
        // upload_name is then used for upload_name (and repository) there-after
        // *except* in GitHub paths, where it's used to query the authoritative git_ctx and locally to fill the fake jwt

        let Some(ref repository) = cli.repository.0 else {
            return Err(eyre!("Could not determine repository name, pass `--repository` formatted like `determinatesystems/flakehub-push`"));
        };

        // If the upload name is supplied by the user, ensure that it contains exactly
        // one slash and no whitespace. Default to the repository name.
        let upload_name = if let Some(ref name) = cli.name.0 {
            let num_slashes = name.matches('/').count();

            if num_slashes == 0
                || num_slashes > 1
                || !name.is_ascii()
                || name.contains(char::is_whitespace)
            {
                return Err(eyre!("The argument `--name` must be in the format of `owner-name/repo-name` and cannot contain whitespace or other special characters"));
            } else {
                name.to_string()
            }
        } else {
            repository.clone()
        };
        let mut repository_split = repository.split('/');
        let project_owner = repository_split
            .next()
            .ok_or_else(|| eyre!("Could not determine owner, pass `--repository` formatted like `determinatesystems/flakehub-push`"))?
            .to_string();
        let project_name = repository_split.next()
            .ok_or_else(|| eyre!("Could not determine project, pass `--repository` formatted like `determinatesystems/flakehub-push`"))?
            .to_string();
        if repository_split.next().is_some() {
            Err(eyre!("Could not determine the owner/project, pass `--repository` formatted like `determinatesystems/flakehub-push`. The passed value has too many slashes (/) to be a valid repository"))?;
        }

        let maybe_git_root = match &cli.git_root.0 {
            Some(gr) => Ok(gr.to_owned()),
            None => std::env::current_dir().map(PathBuf::from),
        };
        let local_git_root = maybe_git_root.wrap_err("Could not determine current `git_root`. Pass `--git-root` or set `FLAKEHUB_PUSH_GIT_ROOT`, or run `flakehub-push` with the git root as the current working directory")?;

        let local_git_root = local_git_root
            .canonicalize()
            .wrap_err("Failed to canonicalize `--git-root` argument")?;
        let local_rev_info = RevisionInfo::from_git_root(&local_git_root)?;

        // "cli" and "git_ctx" are the user/env supplied info, augmented with data we might have fetched from github/gitlab apis

        let (token, git_ctx) = match (exec_env.clone(), &cli.jwt_issuer_uri) {
            (ExecutionEnvironment::GitHub, None) => {
                // GITHUB CI
                let github_token = cli
                    .github_token
                    .0
                    .clone()
                    .expect("failed to get github token when running in GitHub Actions");

                let github_graphql_data_result = GithubGraphqlDataQuery::get(
                    &client,
                    &github_token,
                    &project_owner,
                    &project_name,
                    &local_rev_info.revision,
                )
                .await?;

                let git_ctx = GitContext::from_cli_and_github(cli, &github_graphql_data_result)?;

                let token = crate::github::get_actions_id_bearer_token(&cli.host)
                    .await
                    .wrap_err("Getting upload bearer token from GitHub")?;

                (token, git_ctx)
            }
            (ExecutionEnvironment::GitLab, None) => {
                // GITLAB CI
                let token = crate::gitlab::get_runner_bearer_token()
                    .await
                    .wrap_err("Getting upload bearer token from GitLab")?;

                let git_ctx = GitContext::from_cli_and_gitlab(cli, local_rev_info).await?;

                (token, git_ctx)
            }
            (ExecutionEnvironment::Local, Some(u)) => {
                // LOCAL, DEV (aka emulating GITHUB)
                let github_token = cli
                    .github_token
                    .0
                    .clone()
                    .expect("failed to get github token when running locally");

                let github_graphql_data_result = GithubGraphqlDataQuery::get(
                    &client,
                    &github_token,
                    &project_owner,
                    &project_name,
                    &local_rev_info.revision,
                )
                .await?;

                let git_ctx: GitContext =
                    GitContext::from_cli_and_github(cli, &github_graphql_data_result)?;

                let token = flakehub_auth_fake::get_fake_bearer_token(
                    u,
                    &project_owner,
                    repository,
                    github_graphql_data_result,
                )
                .await?;
                (token, git_ctx)
            }
            (_, Some(_)) => {
                // we're in (GitHub|GitLab) and jwt_issuer_uri was specified, invalid
                return Err(eyre!(
                    "specifying the jwt_issuer_uri when running in GitHub or GitLab is invalid"
                ));
            }
            _ => {
                // who knows what's going on, invalid
                return Err(eyre!("can't determine execution environment"));
            }
        };

        // STEP: resolve/canonicalize "subdir"
        let subdir = if let Some(directory) = &cli.directory.0 {
            let absolute_directory = if directory.is_absolute() {
                directory.clone()
            } else {
                local_git_root.join(directory)
            };
            let canonical_directory = absolute_directory
                .canonicalize()
                .wrap_err("Failed to canonicalize `--directory` argument")?;

            Path::new(
                canonical_directory
                    .strip_prefix(local_git_root.clone())
                    .wrap_err(
                        "Specified `--directory` was not a directory inside the `--git-root`",
                    )?,
            )
            .into()
        } else {
            PathBuf::new()
        };

        let rolling_prefix_or_tag = match (cli.rolling_minor.0.as_ref(), &cli.tag.0) {
            (Some(_), _) if !cli.rolling => {
                return Err(eyre!(
                    "You must enable `rolling` to upload a release with a specific `rolling-minor`."
                ));
            }
            (Some(minor), _) => format!("0.{minor}"),
            (None, _) if cli.rolling => DEFAULT_ROLLING_PREFIX.to_string(),
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

        // TODO(future): (FH-282): change this so commit_count is only set authoritatively, is an explicit error if not set, when rolling, for gitlab
        let Some(commit_count) = git_ctx.revision_info.commit_count else {
            return Err(eyre!("Could not determine commit count, this is normally determined via the `--git-root` argument or via the GitHub API"));
        };

        let rolling_minor_with_postfix_or_tag = if cli.rolling_minor.0.is_some() || cli.rolling {
            format!(
                "{rolling_prefix_or_tag}.{}+rev-{}",
                commit_count, git_ctx.revision_info.revision
            )
        } else {
            rolling_prefix_or_tag.to_string() // This will always be the tag since `self.rolling_prefix` was empty.
        };

        // STEP: calculate labels
        let merged_labels = {
            let mut labels: HashSet<_> = cli
                .extra_labels
                .clone()
                .into_iter()
                .filter(|v| !v.is_empty())
                .collect();
            let extra_tags: HashSet<_> = cli
                .extra_tags
                .clone()
                .into_iter()
                .filter(|v| !v.is_empty())
                .collect();

            if !extra_tags.is_empty() {
                let message = "`extra-tags` is deprecated and will be removed in the future. Please use `extra-labels` instead.";
                tracing::warn!("{message}");

                if matches!(&exec_env, ExecutionEnvironment::GitHub) {
                    println!("::warning::{message}");
                }

                if labels.is_empty() {
                    labels = extra_tags;
                } else {
                    let message =
                        "Both `extra-tags` and `extra-labels` were set; `extra-tags` will be ignored.";
                    tracing::warn!("{message}");

                    if matches!(exec_env, ExecutionEnvironment::GitHub) {
                        println!("::warning::{message}");
                    }
                }
            }

            // Get the "topic" labels from git_ctx, extend local mut labels
            let topics = git_ctx.repo_topics;
            labels = labels
                .into_iter()
                .chain(topics.iter().cloned())
                .collect::<HashSet<String>>();

            // Here we merge explicitly user-supplied labels and the labels ("topics")
            // associated with the repo. Duplicates are excluded and all
            // are converted to lower case.
            let merged_labels: Vec<String> = labels
                .into_iter()
                .take(MAX_NUM_TOTAL_LABELS)
                .map(|s| s.trim().to_lowercase())
                .filter(|t: &String| {
                    !t.is_empty()
                        && t.len() <= MAX_LABEL_LENGTH
                        && t.chars().all(|c| c.is_alphanumeric() || c == '-')
                })
                .collect();

            merged_labels
        };

        // flake_dir is an absolute path of flake_root(aka git_root)/subdir
        let flake_dir = local_git_root.join(&subdir);

        // FIXME: bail out if flake_metadata denotes a dirty tree.
        let flake_metadata = flake_info::FlakeMetadata::from_dir(&flake_dir)
            .await
            .wrap_err("Getting flake metadata")?;
        tracing::debug!("Got flake metadata: {:?}", flake_metadata);

        // sanity checks
        flake_metadata
            .check_evaluates()
            .await
            .wrap_err("failed to evaluate all system attrs of the flake")?;
        flake_metadata
            .check_lock_if_exists()
            .await
            .wrap_err("failed to evaluate all system attrs of the flake")?;

        let flake_outputs = flake_metadata.outputs(cli.include_output_paths).await?;
        tracing::debug!("Got flake outputs: {:?}", flake_outputs);

        let description = flake_metadata
            .metadata_json
            .get("description")
            .and_then(serde_json::Value::as_str)
            .map(|s| s.to_string());

        let readme = flake_metadata.get_readme_contents().await?;

        let release_metadata = ReleaseMetadata {
            commit_count,
            description,
            outputs: flake_outputs.0,
            raw_flake_metadata: flake_metadata.metadata_json.clone(),
            readme,
            // TODO(colemickens): remove this confusing, redundant field (FH-267)
            repo: upload_name.to_string(),
            revision: git_ctx.revision_info.revision,
            visibility,
            mirrored: cli.mirror,
            source_subdirectory: Some(
                subdir
                    .to_str()
                    .map(|d| d.to_string())
                    .ok_or(eyre!("Directory {:?} is not a valid UTF-8 string", subdir))?,
            ),
            spdx_identifier: git_ctx.spdx_expression,
            labels: merged_labels,
        };

        let flake_tarball = flake_metadata
            .flake_tarball()
            .wrap_err("Making release tarball")?;

        let ctx = Self {
            flakehub_host: cli.host.clone(),
            auth_token: token,

            upload_name,
            release_version: rolling_minor_with_postfix_or_tag,

            error_if_release_conflicts: cli.error_on_conflict,

            metadata: release_metadata,
            tarball: flake_tarball,
        };

        Ok(ctx)
    }
}

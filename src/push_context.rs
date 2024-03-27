use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    str::FromStr,
};

use color_eyre::eyre::{eyre, Context, Result};
use spdx::Expression;

use crate::{
    cli::FlakeHubPushCli,
    flake_info, flakehub_auth_fake,
    flakehub_client::Tarball,
    github::graphql::{
        GithubGraphqlDataQuery, GithubGraphqlDataResult, MAX_LABEL_LENGTH, MAX_NUM_TOTAL_LABELS,
    },
    release_metadata::{ReleaseMetadata, RevisionInfo},
    DEFAULT_ROLLING_PREFIX,
};

pub struct GitContext {
    pub spdx_expression: Option<Expression>,
    pub repo_topics: Vec<String>,
    pub revision_info: RevisionInfo,
}

impl GitContext {
    pub fn from_cli_and_github(
        cli: &FlakeHubPushCli,
        github_graphql_data_result: &GithubGraphqlDataResult,
    ) -> Result<Self> {
        // do everything we need to get data from github here

        // step: validate spdx, backfill from GitHub API
        let spdx_expression = if cli.spdx_expression.0.is_none() {
            if let Some(spdx_string) = &github_graphql_data_result.spdx_identifier {
                tracing::debug!("Recieved SPDX identifier `{}` from GitHub API", spdx_string);
                let parsed = spdx::Expression::parse(spdx_string)
                    .wrap_err("Invalid SPDX license identifier reported from the GitHub API, either you are using a non-standard license or GitHub has returned a value that cannot be validated")?;
                //span.record("spdx_expression", tracing::field::display(&parsed));
                Some(parsed)
            } else {
                None
            }
        } else {
            // Provide the user notice if the SPDX expression passed differs from the one detected on GitHub -- It's probably something they care about.
            if github_graphql_data_result.spdx_identifier
                != cli.spdx_expression.0.as_ref().map(|v| v.to_string())
            {
                tracing::warn!(
                    "SPDX identifier `{}` was passed via argument, but GitHub's API suggests it may be `{}`",
                    cli.spdx_expression.0.as_ref().map(|v| v.to_string()).unwrap_or_else(|| "None".to_string()),
                    github_graphql_data_result.spdx_identifier.clone().unwrap_or_else(|| "None".to_string()),
                )
            }
            cli.spdx_expression.0
        };

        let ctx = GitContext {
            spdx_expression: spdx_expression,
            repo_topics: github_graphql_data_result.topics,
            revision_info: RevisionInfo {
                // TODO(colemickens): type coherency here... :/ (as is bad)
                commit_count: Some(github_graphql_data_result.rev_count as usize),
                revision: github_graphql_data_result.revision,
            },
        };
        Ok(ctx)
    }
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

        let client = reqwest::Client::new();

        let is_github = std::env::var("GITHUB_ACTION").ok().is_some();
        let is_gitlab = std::env::var("GITLAB_CI").ok().is_some();

        // "backfill" env vars from the environment, for the first time, anyway...
        if is_github {
            cli.backfill_from_github_env();
        }
        if is_gitlab {
            cli.backfill_from_gitlab_env();
        }

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
        let Some(ref repository) = cli.repository.0 else {
            return Err(eyre!("Could not determine repository name, pass `--repository` formatted like `determinatesystems/flakehub-push`"));
        };
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

        // TODO(colemickens): pretty sure there's a better way to write this:
        let local_git_root = (match &cli.git_root.0 {
            Some(gr) => Ok(gr.to_owned()),
            None => std::env::current_dir().map(PathBuf::from)
        }).wrap_err("Could not determine current `git_root`. Pass `--git-root` or set `FLAKEHUB_PUSH_GIT_ROOT`, or run `flakehub-push` with the git root as the current working directory")?;

        let local_git_root = local_git_root
            .canonicalize()
            .wrap_err("Failed to canonicalize `--git-root` argument")?;
        let local_rev_info = RevisionInfo::from_git_root(&local_git_root)?;

        let (token, git_ctx) = match (is_github, is_gitlab, &cli.jwt_issuer_uri.0) {
            (true, false, None) => {
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

                let git_ctx = GitContext::from_cli_and_github(&cli, &github_graphql_data_result)?;

                let token = crate::github::get_actions_id_bearer_token(&cli.host)
                    .await
                    .wrap_err("Getting upload bearer token from GitHub")?;

                (token, git_ctx)
            }
            (false, true, None) => {
                let token = crate::gitlab::get_runner_bearer_token(&cli.host)
                    .await
                    .wrap_err("Getting upload bearer token from GitLab")?;
                // let git_ctx = GitContext::from_cli_and_gitlab(cli); // TODO: !!!!!
                let git_ctx = todo!();
                (token, git_ctx)
            }
            (false, false, Some(u)) => {
                // local, fake github, we need the ... github ids to get a faked token
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
                    GitContext::from_cli_and_github(&cli, &github_graphql_data_result)?;

                let token = flakehub_auth_fake::get_fake_bearer_token(
                    u,
                    &project_owner,
                    &project_name,
                    github_graphql_data_result,
                )
                .await?;
                (token, git_ctx)
            }
            (_, _, Some(_)) => {
                // we're in GitHub or GitLab and jwt_issuer_uri was specified, invalid
                return Err(eyre!(
                    "specifying the jwt_issuer_uri when running in GitHub or GitLab is invalid"
                ));
            }
            _ => {
                // who knows what's going on, invalid
                return Err(eyre!("can't determine execution environment"));
            }
        };

        // STEP: resolve "subdir" (use --directory flag from cli)
        // TODO(colemickens): do we really need both in our source, "subdir" is part of release_metadata tho
        // NOTE(colemickens,self): flake_dir can probably be evne more "intenral" only used for tarring/flake_info
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
                let version_only = tag.strip_prefix('v').unwrap_or(&tag);
                // Ensure the version respects semver
                semver::Version::from_str(version_only).wrap_err_with(|| eyre!("Failed to parse version `{tag}` as semver, see https://semver.org/ for specifications"))?;
                tag.to_string()
            }
            (None, None) => {
                return Err(eyre!("Could not determine tag or rolling minor version, `--tag`, `GITHUB_REF_NAME`, or `--rolling-minor` must be set"));
            }
        };

        // TODO(don't use revision_info, maybe, so we don't have this extra shadowy local var)
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
        // - they can specify: extra_labels on cli
        // TODO: merge execution_environment's labels + cli extra_labels
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

                // TODO(colemickens): restore this cleanly, have something on git_ctx that logs? rename git_ctx?
                // if is_github_actions {
                //     println!("::warning::{message}");
                // }

                if labels.is_empty() {
                    labels = extra_tags;
                } else {
                    let message =
                        "Both `extra-tags` and `extra-labels` were set; `extra-tags` will be ignored.";
                    tracing::warn!("{message}");

                    // TODO(colemickens): restore this cleanly, have something on git_ctx that logs? rename git_ctx?
                    // if is_github_actions {
                    //     println!("::warning::{message}");
                    // }
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
        // TODO: depending on what the user called us with, this flake_dir isn't even necessarily canonicalized, is this a sec/traversal issue?

        // FIXME: bail out if flake_metadata denotes a dirty tree.
        // (Todo make sure invariant checks are right, de-duped properly)
        let flake_metadata = flake_info::FlakeMetadata::from_dir(&flake_dir)
            .await
            .wrap_err("Getting flake metadata")?;
        tracing::debug!("Got flake metadata: {:?}", flake_metadata);

        let flake_outputs = flake_metadata.outputs(cli.include_output_paths).await?;
        tracing::debug!("Got flake outputs: {:?}", flake_outputs);

        let description = flake_metadata
            .metadata_json
            .get("description")
            .and_then(serde_json::Value::as_str)
            .map(|s| s.to_string());

        let readme = flake_metadata.get_readme_contents().await?;

        let release_metadata = ReleaseMetadata {
            commit_count: commit_count,
            description: description,
            outputs: flake_outputs.0,
            raw_flake_metadata: flake_metadata.metadata_json.clone(),
            readme: readme,
            repo: repository.to_string(),
            revision: git_ctx.revision_info.revision,
            visibility: visibility,
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
            //.await //weird, tar is not async?
            .wrap_err("Making release tarball")?;

        let ctx = Self {
            flakehub_host: cli.host.clone(),
            auth_token: token,

            upload_name: upload_name,
            release_version: rolling_minor_with_postfix_or_tag,

            error_if_release_conflicts: cli.error_on_conflict,

            // this is the payload we send
            metadata: release_metadata,
            tarball: flake_tarball,
        };
        Ok(ctx)
    }
}

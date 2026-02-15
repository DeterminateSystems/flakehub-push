use color_eyre::eyre::{eyre, Context, Result};

use crate::{
    build_http_client, cli::FlakeHubPushCli, flakehub_auth_fake, flakehub_client::Tarball,
    git_context::GitContext, github::graphql::GithubGraphqlDataQuery,
    release_metadata::ReleaseMetadata, revision_info::RevisionInfo,
};

#[derive(Clone)]
pub enum ExecutionEnvironment {
    GitHub,
    GitLab,
    LocalGitHub,
    Generic,
}

/// Captures the data needed to acquire an auth token for a given execution
/// environment. Token acquisition is deferred until after Nix evaluation
/// completes, so that short-lived OIDC tokens (e.g. GitHub's ~5 min JWTs)
/// are not stale by the time they are used.
pub(crate) enum TokenContext {
    GitHub {
        host: url::Url,
    },
    GitLab,
    Generic,
    LocalGitHub {
        jwt_issuer_uri: String,
        project_owner: String,
        repository: String,
        github_graphql_data_result: crate::github::graphql::GithubGraphqlDataResult,
    },
}

pub(crate) struct PushContext {
    pub(crate) flakehub_host: url::Url,
    pub(crate) token_context: TokenContext,

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

        let exec_env = cli.execution_environment();

        match exec_env.clone() {
            ExecutionEnvironment::GitHub => {
                cli.backfill_from_github_env();
            }
            ExecutionEnvironment::GitLab => {
                cli.backfill_from_gitlab_env();
            }
            _ => {}
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

        let (upload_name, project_owner, project_name) =
            determine_names(&cli.name.0, repository, cli.disable_rename_subgroups)?;

        let local_git_root = cli.resolve_local_git_root()?;
        let local_rev_info = RevisionInfo::from_git_root(&local_git_root)?;

        // "cli" and "git_ctx" are the user/env supplied info, augmented with data we might have fetched from github/gitlab apis

        let (token_context, git_ctx) = match (&exec_env, &cli.jwt_issuer_uri) {
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
                    cli.rev.0.as_ref().unwrap_or(&local_rev_info.revision),
                )
                .await?;

                let git_ctx = GitContext::from_cli_and_github(cli, &github_graphql_data_result)?;

                let token_ctx = TokenContext::GitHub {
                    host: cli.host.clone(),
                };

                (token_ctx, git_ctx)
            }
            (ExecutionEnvironment::GitLab, None) => {
                // GITLAB CI
                let git_ctx = GitContext::from_cli_and_gitlab(cli, local_rev_info).await?;

                (TokenContext::GitLab, git_ctx)
            }
            (ExecutionEnvironment::Generic, None) => {
                // Generic CI (Semaphore, ...)
                let git_ctx = GitContext::from_cli(cli, local_rev_info).await?;

                (TokenContext::Generic, git_ctx)
            }
            (ExecutionEnvironment::LocalGitHub, Some(u)) => {
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
                    cli.rev.0.as_ref().unwrap_or(&local_rev_info.revision),
                )
                .await?;

                let git_ctx: GitContext =
                    GitContext::from_cli_and_github(cli, &github_graphql_data_result)?;

                let token_ctx = TokenContext::LocalGitHub {
                    jwt_issuer_uri: u.clone(),
                    project_owner: project_owner.clone(),
                    repository: repository.clone(),
                    github_graphql_data_result,
                };

                (token_ctx, git_ctx)
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

        let release_version = cli.release_version(&git_ctx)?;

        let (release_metadata, flake_tarball) =
            ReleaseMetadata::new(cli, &git_ctx, Some(&exec_env)).await?;

        let ctx = Self {
            flakehub_host: cli.host.clone(),
            token_context,

            upload_name,
            release_version,

            error_if_release_conflicts: cli.error_on_conflict,

            metadata: release_metadata,
            tarball: flake_tarball,
        };

        Ok(ctx)
    }

    /// Acquire the auth token for the current execution environment.
    ///
    /// This is intentionally called *after* PushContext construction (which
    /// includes expensive Nix evaluation) so that short-lived OIDC tokens
    /// are fresh when first used by FlakeHubClient.
    pub async fn acquire_auth_token(self) -> Result<(String, Self)> {
        // Destructure up front so we own all fields and can match on
        // token_context by value (required for LocalGitHub which moves
        // github_graphql_data_result into get_fake_bearer_token).
        let PushContext {
            flakehub_host,
            token_context,
            upload_name,
            release_version,
            error_if_release_conflicts,
            metadata,
            tarball,
        } = self;

        let (token, token_context) = match token_context {
            TokenContext::GitHub { ref host } => {
                let t = crate::github::get_actions_id_bearer_token(host)
                    .await
                    .wrap_err("Getting upload bearer token from GitHub")?;
                (t, token_context)
            }
            TokenContext::GitLab => {
                let t = crate::gitlab::get_runner_bearer_token()
                    .await
                    .wrap_err("Getting upload bearer token from GitLab")?;
                (t, TokenContext::GitLab)
            }
            TokenContext::Generic => {
                let t = std::env::var("FLAKEHUB_PUSH_OIDC_TOKEN")
                    .with_context(|| "missing FLAKEHUB_PUSH_OIDC_TOKEN environment variable")?;
                (t, TokenContext::Generic)
            }
            TokenContext::LocalGitHub {
                jwt_issuer_uri,
                project_owner,
                repository,
                github_graphql_data_result,
            } => {
                let t = flakehub_auth_fake::get_fake_bearer_token(
                    &jwt_issuer_uri,
                    &project_owner,
                    &repository,
                    github_graphql_data_result,
                )
                .await?;
                // Use a sentinel since the token has been acquired and the
                // graphql data has been consumed.
                (t, TokenContext::Generic)
            }
        };

        let ctx = PushContext {
            flakehub_host,
            token_context,
            upload_name,
            release_version,
            error_if_release_conflicts,
            metadata,
            tarball,
        };

        Ok((token, ctx))
    }
}

pub(crate) fn determine_names(
    explicitly_provided_name: &Option<String>,
    repository: &str,
    subgroup_renaming_explicitly_disabled: bool,
) -> Result<(String, String, String)> {
    let error_msg = if subgroup_renaming_explicitly_disabled {
        "Could not determine project owner and name; pass `--repository` formatted like `determinatesystems/flakehub-push`"
    } else {
        "Could not determine project owner and name; pass `--repository` formatted like `determinatesystems/flakehub-push` or `determinatesystems/subgroup-segments.../flakehub-push`)"
    };

    let mut repository_split = repository.split('/');
    let project_owner = repository_split
        .next()
        .ok_or_else(|| eyre!(error_msg))?
        .to_string();
    let project_name = repository_split
        .next()
        .ok_or_else(|| eyre!(error_msg))?
        .to_string();
    if subgroup_renaming_explicitly_disabled && repository_split.next().is_some() {
        Err(eyre!(error_msg))?;
    }
    // If subgroup renaming is disabled, the project name is just the originally provided
    // name (and we've already determined that the name is of the form `{owner}/{project}`.
    // But if subgroup renaming is disabled, then a repo name like `a/b/c/d/e` is converted
    // to `a/b-c-d-e`.
    let project_name = if subgroup_renaming_explicitly_disabled {
        project_name
    } else {
        repository_split.fold(project_name, |mut acc, segment| {
            acc.push_str(&format!("-{segment}"));
            acc
        })
    };

    // If a flake name is explicitly provided, validate that name, otherwise use the
    // inferred repository name
    let upload_name = if let Some(name) = explicitly_provided_name {
        let num_slashes = name.matches('/').count();

        if num_slashes == 0
            || !name.is_ascii()
            || name.contains(char::is_whitespace)
            || num_slashes > 1
        {
            return Err(eyre!("The argument `--name` must be in the format of `owner-name/flake-name` and cannot contain whitespace or other special characters"));
        } else {
            name.to_string()
        }
    } else {
        format!("{project_owner}/{project_name}")
    };

    Ok((upload_name, project_owner, project_name))
}

#[cfg(test)]
mod tests {
    use crate::push_context::determine_names;

    #[test]
    fn project_owner_and_name() {
        struct Expected {
            upload_name: &'static str,
            project_owner: &'static str,
            project_name: &'static str,
        }

        struct SuccessTestCase {
            explicit_upload_name: Option<&'static str>,
            repository: &'static str,
            disable_subgroup_renaming: bool,
            expected: Expected,
        }

        struct FailureTestCase {
            explicit_upload_name: Option<&'static str>,
            repository: &'static str,
            disable_subgroup_renaming: bool,
            error_msg: &'static str,
        }

        let success_cases: Vec<SuccessTestCase> = vec![
            SuccessTestCase {
                explicit_upload_name: None,
                repository: "DeterminateSystems/testing/flakehub-push-test-subrepo",
                disable_subgroup_renaming: false,
                expected: Expected {
                    upload_name: "DeterminateSystems/testing-flakehub-push-test-subrepo",
                    project_owner: "DeterminateSystems",
                    project_name: "testing-flakehub-push-test-subrepo",
                },
            },
            SuccessTestCase {
                explicit_upload_name: Some("DeterminateSystems/flakehub-test"),
                repository: "DeterminateSystems/flakehub",
                disable_subgroup_renaming: false,
                expected: Expected {
                    upload_name: "DeterminateSystems/flakehub-test",
                    project_owner: "DeterminateSystems",
                    project_name: "flakehub",
                },
            },
            SuccessTestCase {
                explicit_upload_name: None,
                repository: "DeterminateSystems/flakehub",
                disable_subgroup_renaming: false,
                expected: Expected {
                    upload_name: "DeterminateSystems/flakehub",
                    project_owner: "DeterminateSystems",
                    project_name: "flakehub",
                },
            },
            SuccessTestCase {
                explicit_upload_name: Some("a/my-flake"),
                disable_subgroup_renaming: false,
                repository: "a/b/c",
                expected: Expected {
                    upload_name: "a/my-flake",
                    project_owner: "a",
                    project_name: "b-c",
                },
            },
            SuccessTestCase {
                explicit_upload_name: None,
                repository: "a/b/c/d/e/f/g/h",
                disable_subgroup_renaming: false,
                expected: Expected {
                    upload_name: "a/b-c-d-e-f-g-h",
                    project_owner: "a",
                    project_name: "b-c-d-e-f-g-h",
                },
            },
            SuccessTestCase {
                explicit_upload_name: None,
                repository: "a/b/c/d/e/f/g/h/i/j/k/l",
                disable_subgroup_renaming: false,
                expected: Expected {
                    upload_name: "a/b-c-d-e-f-g-h-i-j-k-l",
                    project_owner: "a",
                    project_name: "b-c-d-e-f-g-h-i-j-k-l",
                },
            },
            SuccessTestCase {
                explicit_upload_name: None,
                repository: "DeterminateSystems/subgroup/flakehub",
                disable_subgroup_renaming: false,
                expected: Expected {
                    upload_name: "DeterminateSystems/subgroup-flakehub",
                    project_owner: "DeterminateSystems",
                    project_name: "subgroup-flakehub",
                },
            },
            SuccessTestCase {
                explicit_upload_name: None,
                repository: "DeterminateSystems/subgroup/subsubgroup/flakehub",
                disable_subgroup_renaming: false,
                expected: Expected {
                    upload_name: "DeterminateSystems/subgroup-subsubgroup-flakehub",
                    project_owner: "DeterminateSystems",
                    project_name: "subgroup-subsubgroup-flakehub",
                },
            },
        ];

        for SuccessTestCase {
            explicit_upload_name,
            repository,
            disable_subgroup_renaming,
            expected:
                Expected {
                    upload_name: expected_upload_name,
                    project_owner: expected_project_owner,
                    project_name: expected_project_name,
                },
        } in success_cases
        {
            let (upload_name, owner, name) = determine_names(
                &explicit_upload_name.map(String::from),
                repository,
                disable_subgroup_renaming,
            )
            .unwrap();
            assert_eq!(
                (String::from(expected_upload_name), String::from(expected_project_owner), String::from(expected_project_name)),
                (upload_name.clone(), owner.clone(), name.clone()),
                "expected {expected_project_owner}/{expected_project_name} from repository {repository} but got {owner}/{name} instead"
            );
        }

        let failure_cases: Vec<FailureTestCase> = vec![

            FailureTestCase {
                explicit_upload_name: None,
                // Two slashes in repository with subgroup renaming disabled
                repository: "a/b/c",
                disable_subgroup_renaming: true,
                error_msg: "Could not determine project owner and name; pass `--repository` formatted like `determinatesystems/flakehub-push`",
            },

            FailureTestCase {
                explicit_upload_name: None,
                // No slashes in repository
                repository: "a",
                disable_subgroup_renaming: false,
                error_msg: "Could not determine project owner and name; pass `--repository` formatted like `determinatesystems/flakehub-push` or `determinatesystems/subgroup-segments.../flakehub-push`)",
            },
            FailureTestCase {
                // No slashes in explicit name
                explicit_upload_name: Some("zero-slashes"),
                repository: "doesnt-matter",
                disable_subgroup_renaming: true,
                error_msg: "Could not determine project owner and name; pass `--repository` formatted like `determinatesystems/flakehub-push`",
            },
            FailureTestCase {
                // Two slashes in explicitly provided name
                explicit_upload_name: Some("a/b/c"),
                repository: "a/b",
                disable_subgroup_renaming: true,
                error_msg: "The argument `--name` must be in the format of `owner-name/flake-name` and cannot contain whitespace or other special characters",
            },

            FailureTestCase {
                // Five slashes in explicit name wit subgroup renaming disabled
                explicit_upload_name: Some("a/b/c/d/e/f"),
                repository: "doesnt-matter",
                disable_subgroup_renaming: true,
                // The repository name is invalid so that error gets thrown first
                error_msg: "Could not determine project owner and name; pass `--repository` formatted like `determinatesystems/flakehub-push`",
            },
        ];

        for FailureTestCase {
            explicit_upload_name,
            repository,
            disable_subgroup_renaming,
            error_msg: expected_error_msg,
        } in failure_cases
        {
            let error_msg = determine_names(
                &explicit_upload_name.map(String::from),
                repository,
                disable_subgroup_renaming,
            )
            .err()
            .unwrap()
            .to_string();

            assert_eq!(
                error_msg,
                String::from(expected_error_msg),
                "expected {} and `{repository}` to produce error message `{expected_error_msg}` but produced message `{error_msg}` instead", if let Some(ref explicit_upload_name) = &explicit_upload_name { format!("explicit upload name `{}`", explicit_upload_name) } else { String::from("no explicit upload name") },
            );
        }
    }
}

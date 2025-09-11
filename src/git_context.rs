use color_eyre::eyre::{Context, Result};
use spdx::Expression;

use crate::{
    cli::FlakeHubPushCli, github::graphql::GithubGraphqlDataResult, revision_info::RevisionInfo,
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
        // step: validate spdx, backfill from GitHub API
        let spdx_expression = if cli.spdx_expression.0.is_none() {
            if let Some(spdx_string) = &github_graphql_data_result.spdx_identifier {
                tracing::debug!("Recieved SPDX identifier `{}` from GitHub API", spdx_string);
                let parsed = spdx::Expression::parse(spdx_string)
                    .wrap_err("Invalid SPDX license identifier reported from the GitHub API, either you are using a non-standard license or GitHub has returned a value that cannot be validated")?;
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
            cli.spdx_expression.0.clone()
        };

        let rev = cli
            .rev
            .0
            .as_ref()
            .unwrap_or(&github_graphql_data_result.revision);

        let ctx = GitContext {
            spdx_expression,
            repo_topics: github_graphql_data_result.topics.clone(),
            revision_info: RevisionInfo {
                commit_count: Some(github_graphql_data_result.rev_count as usize),
                revision: rev.to_string(),
            },
        };
        Ok(ctx)
    }

    pub async fn from_cli_and_gitlab(
        cli: &FlakeHubPushCli,
        local_revision_info: RevisionInfo,
    ) -> Result<Self> {
        // TODO(future): investigate library to sniff out SPDX expression based on repo contents
        // spdx_expression: can't find any evidence GitLab tries to surface this info
        let spdx_expression = &cli.spdx_expression.0;

        let rev = cli.rev.0.as_ref().unwrap_or(&local_revision_info.revision);

        let ctx = GitContext {
            spdx_expression: spdx_expression.clone(),
            repo_topics: vec![],
            revision_info: RevisionInfo {
                commit_count: local_revision_info.commit_count,
                revision: rev.to_string(),
            },
        };
        Ok(ctx)
    }

    pub async fn from_cli(
        cli: &FlakeHubPushCli,
        local_revision_info: RevisionInfo,
    ) -> Result<Self> {
        let spdx_expression = &cli.spdx_expression.0;

        let rev = cli.rev.0.as_ref().unwrap_or(&local_revision_info.revision);

        let ctx = GitContext {
            spdx_expression: spdx_expression.clone(),
            repo_topics: vec![],
            revision_info: RevisionInfo {
                commit_count: local_revision_info.commit_count,
                revision: rev.to_string(),
            },
        };
        Ok(ctx)
    }
}

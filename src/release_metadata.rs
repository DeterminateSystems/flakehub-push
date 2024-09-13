use std::collections::HashSet;

use color_eyre::eyre::{eyre, Context as _, Result};

use crate::cli::FlakeHubPushCli;
use crate::flake_info::FlakeMetadata;
use crate::flakehub_client::Tarball;
use crate::git_context::GitContext;
use crate::github::graphql::{MAX_LABEL_LENGTH, MAX_NUM_TOTAL_LABELS};
use crate::push_context::ExecutionEnvironment;
use crate::Visibility;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ReleaseMetadata {
    pub(crate) commit_count: usize,
    pub(crate) description: Option<String>,
    pub(crate) outputs: serde_json::Value,
    pub(crate) raw_flake_metadata: serde_json::Value,
    pub(crate) readme: Option<String>,
    pub(crate) repo: String,
    pub(crate) revision: String,
    pub(crate) visibility: Visibility,
    pub(crate) mirrored: bool,
    pub(crate) source_subdirectory: Option<String>,

    #[serde(
        deserialize_with = "option_string_to_spdx",
        serialize_with = "option_spdx_serialize"
    )]
    pub(crate) spdx_identifier: Option<spdx::Expression>,

    // A result of combining the labels specified on the CLI via the the GitHub Actions config
    // and the labels associated with the GitHub repo (they're called "topics" in GitHub parlance).
    pub(crate) labels: Vec<String>,
}

impl ReleaseMetadata {
    pub async fn new(
        cli: &FlakeHubPushCli,
        git_ctx: &GitContext,
        exec_env: Option<&ExecutionEnvironment>,
    ) -> Result<(Self, Tarball)> {
        let local_git_root = cli.resolve_local_git_root()?;
        let subdir = cli.subdir_from_git_root(&local_git_root)?;

        // flake_dir is an absolute path of flake_root(aka git_root)/subdir
        let flake_dir = local_git_root.join(&subdir);

        let flake_metadata = FlakeMetadata::from_dir(&flake_dir, cli.my_flake_is_too_big)
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

        let Some(commit_count) = git_ctx.revision_info.commit_count else {
            return Err(eyre!("Could not determine commit count, this is normally determined via the `--git-root` argument or via the GitHub API"));
        };

        let description = flake_metadata
            .metadata_json
            .get("description")
            .and_then(serde_json::Value::as_str)
            .map(|s| s.to_string());

        let flake_outputs = flake_metadata.outputs(cli.include_output_paths).await?;
        tracing::debug!("Got flake outputs: {:?}", flake_outputs);

        let readme = flake_metadata.get_readme_contents().await?;

        let Some(ref repository) = cli.repository.0 else {
            return Err(eyre!("Could not determine repository name, pass `--repository` formatted like `determinatesystems/flakehub-push`"));
        };

        let (upload_name, _project_owner, _project_name) = crate::push_context::determine_names(
            &cli.name.0,
            repository,
            cli.disable_rename_subgroups,
        )?;

        let visibility = cli.visibility()?;

        let labels = if let Some(exec_env) = exec_env {
            Self::merged_labels(cli, git_ctx, exec_env)
        } else {
            Vec::new()
        };

        let release_metadata = ReleaseMetadata {
            commit_count,
            description,
            outputs: flake_outputs.0,
            raw_flake_metadata: flake_metadata.metadata_json.clone(),
            readme,
            // TODO(colemickens): remove this confusing, redundant field (FH-267)
            repo: upload_name.to_string(),
            revision: git_ctx.revision_info.revision.clone(),
            visibility,
            mirrored: cli.mirror,
            source_subdirectory: Some(subdir.to_str().map(|d| d.to_string()).ok_or(
                color_eyre::eyre::eyre!("Directory {:?} is not a valid UTF-8 string", subdir),
            )?),
            spdx_identifier: git_ctx.spdx_expression.clone(),
            labels,
        };

        let flake_tarball = flake_metadata
            .flake_tarball()
            .wrap_err("Making release tarball")?;

        Ok((release_metadata, flake_tarball))
    }

    fn merged_labels(
        cli: &FlakeHubPushCli,
        git_ctx: &GitContext,
        exec_env: &ExecutionEnvironment,
    ) -> Vec<String> {
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
        let topics = &git_ctx.repo_topics;
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
    }
}

// TODO(review,colemickens): I don't really undersatnd why these are nededed??? we don't need the OptionString-y stuff since this isn't GHA adjacent?

fn option_string_to_spdx<'de, D>(deserializer: D) -> Result<Option<spdx::Expression>, D::Error>
where
    D: serde::de::Deserializer<'de>,
{
    let spdx_identifier: Option<&str> = serde::Deserialize::deserialize(deserializer)?;

    if let Some(spdx_identifier) = spdx_identifier {
        spdx::Expression::parse(spdx_identifier)
            .map_err(serde::de::Error::custom)
            .map(Option::Some)
    } else {
        Ok(None)
    }
}

fn option_spdx_serialize<S>(
    spdx_identifier: &Option<spdx::Expression>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::ser::Serializer,
{
    if let Some(spdx_identifier) = spdx_identifier {
        let spdx_string = spdx_identifier.to_string();
        serializer.serialize_str(&spdx_string)
    } else {
        serializer.serialize_none()
    }
}

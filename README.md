# `flakehub-push`

A GitHub Action for pushing [Nix flakes][flakes] to [FlakeHub].
Create a [YAML configuration](#configuration), push it to your repo, and you're ready to go.

## Configuration

There are two ways to get started configuring this Action:

1. Use our [wizard](#guided-wizard) to create a configuration.
1. Configure the Action [manually](#manual-configuration).

### Guided wizard

Although the `flakehub-push` Action requires little configuration, you may benefit from assembling it with our friendly UI at [flakehub.com/new][wizard].

## Integration

This action sets outputs for integrating into continuous delivery pipelines:

| Output              | Description                                                                                                                                                                                                                    | Example                                                                                 |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------- |
| `flake_name`        | Name of the flake.                                                                                                                                                                                                             | `DeterminateSystems/flakehub-push`                                                      |
| `flake_version`     | Version of the published flake.                                                                                                                                                                                                | `0.1.99+rev-2075013a3f3544d45a96f4b35df4ed03cd53779c`                                   |
| `flakeref_exact`    | A precise reference that always resolves to this to this exact release.                                                                                                                                                        | `DeterminateSystems/flakehub-push/=0.1.99+rev-2075013a3f3544d45a96f4b35df4ed03cd53779c` |
| `flakeref_at_least` | A loose reference to this release. Depending on this reference will require at least this version, and will also resolve to newer releases. This output is not sufficient for deployment pipelines, use `flake_exact` instead. | `DeterminateSystems/flakehub-push/0.1.99+rev-2075013a3f3544d45a96f4b35df4ed03cd53779c`  |

## More Information

### Manual configuration

The example workflow configuration below pushes new tags matching the conventional format&mdash;such as `v1.0.0` or `v0.1.0-rc4`&mdash;to [Flakehub]:

```yaml
# .github/workflows/flakehub-publish-tagged.yml
name: Publish tags to FlakeHub

on:
  push:
    tags:
      - "v?[0-9]+.[0-9]+.[0-9]+*"

jobs:
  flakehub:
    runs-on: ubuntu-22.04
    permissions:
      id-token: write # Necessary for authenticating against FlakeHub
      contents: read
    steps:
      - uses: actions/checkout@v4
        with:
          ref: "${{ (inputs.tag != null) && format('refs/tags/{0}', inputs.tag) || '' }}"
      - name: Install Nix
        uses: DeterminateSystems/nix-installer-action@main
      - name: Push to flakehub
        uses: determinatesystems/flakehub-push@main
        with:
          # For the flake's visibility, you can also select "unlisted" if you don't want
          # it to show up in search results and general listings on flakehub.com
          visibility: "public"
```

#### Available parameters

| Parameter              | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                             | Type          | Required? | Default                    |
| :--------------------- | :---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :------------ | :-------- | :------------------------- |
| `visibility`           | `public`, `unlisted`, or `private`. Private flakes are in private beta, contact support@flakehub.com to sign up.                                                                                                                                                                                                                                                                                                                                                        | enum          | ✅        |                            |
| `repository`           | The GitHub repository containing your flake in the format of `{org}/{repo}`.                                                                                                                                                                                                                                                                                                                                                                                            | string        | ✅        | `${{ github.repository }}` |
| `name`                 | The name of your published flake in the format of `{org}/{name}`. The `{org}` must match your organization's GitHub root name or the publish will fail. Specify this only if you want to publish under a different name from the `{org}/{repo}`.                                                                                                                                                                                                                        | string        |           |                            |
| `include-output-paths` | Whether to expose store paths for the flake's outputs via the FlakeHub API. This is most useful when used in conjunction with [FlakeHub Cache][cache].                                                                                                                                                                                                                                                                                                                  | Boolean       |           | `false`                    |
| `mirror`               | Whether the repository is mirrored via DeterminateSystems' mirror functionality. This is only usable by DeterminateSystems.                                                                                                                                                                                                                                                                                                                                             | Boolean       |           | `false`                    |
| `directory`            | The path of your flake relative to the root of the repository. Useful for subflakes.                                                                                                                                                                                                                                                                                                                                                                                    | relative path |           |                            |
| `tag`                  | The Git tag to use for non-rolling releases. This must be the character `v` followed by a SemVer version, such as `v0.1.1`.                                                                                                                                                                                                                                                                                                                                             | string        |           |                            |
| `rolling`              | For untagged releases, use a rolling versioning scheme. When this is enabled, the default versioning scheme is 0.1.[commit count]+rev-[git sha]. To customize the SemVer minor version, set the `rolling-minor` option.                                                                                                                                                                                                                                                 | Boolean       |           | `false`                    |
| `rolling-minor`        | Specify the SemVer minor version of your rolling releases. All releases will follow the versioning scheme `0.[rolling-minor].[commit count]+rev-[git sha]`.                                                                                                                                                                                                                                                                                                             | string        |           |                            |
| `git-root`             | The root directory of your Git repository.                                                                                                                                                                                                                                                                                                                                                                                                                              | relative path |           | `.`                        |
| `extra-labels`         | `flakehub-push` automatically uses the GitHub repo's topics as labels. This `extra-labels` parameter enables you to add extra labels beyond that as a comma-separated string. Only alphanumeric characters and hyphens are allowed in labels and the maximum length of labels is 50 characters. You can specify a maximum of 20 extra labels, and have a maximum of 25 labels, including those that we retrieve from GitHub. Any labels after the 25th will be ignored. | string        |           | `""`                       |
| `spdx-expression`      | A valid SPDX license expression. This will be used in place of what GitHub claims your repository's `spdxIdentifier` is.                                                                                                                                                                                                                                                                                                                                                | string        |           | `""`                       |
| `error-on-conflict`    | Whether to error if a release for the same version has already been uploaded.                                                                                                                                                                                                                                                                                                                                                                                           | Boolean       |           | `false`                    |
| `github-token`         | The GitHub token for making authenticated GitHub API requests.                                                                                                                                                                                                                                                                                                                                                                                                          | string        |           | `${{ github.token }}`      |
| `host`                 | The FlakeHub server to use.                                                                                                                                                                                                                                                                                                                                                                                                                                             | URL           |           | `https://api.flakehub.com` |
| `logger`               | The logger to use. Options are `pretty`, `json`, `full` and `compact`.                                                                                                                                                                                                                                                                                                                                                                                                  | enum          |           | `full`                     |
| `log-directives`       | A comma-separated list of [tracing directives](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html#directives). `-`s are replaced with `_`s (such as `nix_installer=trace`).                                                                                                                                                                                                                                                      | string        |           | `flakehub_push=info`       |
| `source-binary`        | Run a version of the `flakehub-push` binary from somewhere already on disk. Conflicts with all other `source-*` options.                                                                                                                                                                                                                                                                                                                                                | string        |           |                            |
| `source-branch`        | The branch of `flakehub-push` to use. Conflicts with all other `source-*` options.                                                                                                                                                                                                                                                                                                                                                                                      | string        |           | `main`                     |
| `source-pr`            | The pull request for `flakehub-push` to use. Conflicts with all other `source-*` options.                                                                                                                                                                                                                                                                                                                                                                               | integer       |           |                            |
| `source-revision`      | The revision of `flakehub-push` to use. Conflicts with all other `source-*` options.                                                                                                                                                                                                                                                                                                                                                                                    | string        |           |                            |
| `source-tag`           | The tag of `flakehub-push` to use. Conflicts with all other `source-*` options.                                                                                                                                                                                                                                                                                                                                                                                         | string        |           |                            |
| `source-url`           | A URL pointing to a `flakehub-push` binary. Overrides all other `source-*` options.                                                                                                                                                                                                                                                                                                                                                                                     | string        |           |                            |

## Developing `flakehub-push`

See the [development docs](./docs/development.md).

[cache]: https://determinate.systems/posts/flakehub-cache-beta
[flakehub]: https://flakehub.com
[flakes]: https://zero-to-nix.com/concepts/flakes
[wizard]: https://flakehub.com/new

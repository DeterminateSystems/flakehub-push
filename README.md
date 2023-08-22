# `flakehub-push`

A GitHub Action for pushing [Nix flakes][flakes] to [FlakeHub].
Write up a [YAML configuration](#configuration), push it to your repo, and you're ready to go.

## Example

This workflow pushes new tags matching the conventional format (eg.
`v1.0.0`, `v0.1.0-rc4`) to Flakehub.

```yaml
# .github/workflows/release.yml
name: Release

on:
  push:
    tags:
      - "v*.*.*"

jobs:
  flakehub:
    runs-on: ubuntu-22.04
    permissions:
      id-token: write # Necessary for authenticating against FlakeHub
      contents: read
    steps:
      - uses: DeterminateSystems/nix-installer-action@v4
      - uses: actions/checkout@v3
      - name: Push to flakehub
        uses: determinatesystems/flakehub-push@main
        with:
          visibility: "public" # or "unlisted" if you don't want it to show up in
                               # search results and general listings on flakehub.com
```

## Configuration

Parameter | Description | Type | Required? | Default
:---------|:------------|:-----|:----------|:-------
`visibility` | `public` or `unlisted` | enum | ✅ |
`repository` | The GitHub repository containing your flake in the format of `{org}/{repo}`. | string | ✅ | `${{ github.repository }}`
`name` | The name of your published flake in the format of `{org}/{name}`. The `{org}` must match your organization's GitHub root name or the publish will fail. Specify this only if you want to publish under a different name from the `{org}/{repo}`. | string | |
`mirror` | Whether the repository is mirrored via DeterminateSystems' mirror functionality. This is only usable by DeterminateSystems. | Boolean | | `false`
`directory` | The path of your flake relative to the root of the repository. Useful for subflakes. | relative path | |
`tag` | The Git tag to use for non-rolling releases. This must be the character `v` followed by a SemVer version, such as `v0.1.1`. | string | |
`rolling` | For untagged releases, use a rolling versioning scheme. When this is enabled, the default versioning scheme is 0.1.[commit count]+rev-[git sha]. To customize the SemVer minor version, set the `rolling-minor` option. | Boolean | | `false`
`rolling-minor` | Specify the SemVer minor version of your rolling releases. All releases will follow the versioning scheme '0.[rolling-minor].[commit count]+rev-[git sha]' | string | |
`git-root` | The root directory of your Git repository. | relative path | | `.`
`extra-tags` | `flakehub-push` automatically uses the GitHub repo's topics as tags. This `extra-tags` parameter enables you to add extra tags beyond that as a comma-separated string. Only alphanumeric characters and hyphens are allowed in tags and the maximum length of tags is 50 characters. You can specify a maximum of 20 extra tags, and have a maximum of 25 tags, including those that we retrieve from GitHub. Any tags after the 25th will be ignored. | string | | `""`
`spdx-expression` | A valid SPDX license expression. This will be used in place of what GitHub claims your repository's `spdxIdentifier` is. | string | | `""`
`github-token` | The GitHub token for making authenticated GitHub API requests. | `${{ github.token }}`
`host` | The FlakeHub server to use | URL | | `https://api.flakehub.com`
`logger` | The logger to use. Options are `pretty`, `json`, `full` and `compact`. | enum | | `full`
`log-directives` | A comma-separated list of [tracing directives](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html#directives). `-`s are replaced with `_`s (such as `nix_installer=trace`). | string | | `flakehub_push=info`
`flakehub-push-binary` | Run a version of the `flakehub-push` binary from somewhere already on disk. Conflicts with all other `flakehub-push-*` options. | string | |
`flakehub-push-branch` | The branch of `flakehub-push` to use. Conflicts with all other `flakehub-push-*` options. | string | | `main`
`flakehub-push-pr` | The pull request for `flakehub-push` to use. Conflicts with all other `flakehub-push-*` options. | integer | |
`flakehub-push-revision` | The revision of `flakehub-push` to use. Conflicts with all other `flakehub-push-*` options. | string | |
`flakehub-push-tag` | The tag of `flakehub-push` to use. Conflicts with all other `flakehub-push-*` options. | string | | |
`flakehub-push-url` | A URL pointing to a `flakehub-push` binary. Overrides all other `flakehub-push-*` options. | string | | |

## Development against a local Flakehub server

Assuming the dev environment is running as described in the flakehub repo:

```bash
export FLAKEHUB_PUSH_GITHUB_TOKEN="<secret>"
cargo run -- \
  --visibility public \
  --tag v0.1.0 \
  --repository DeterminateSystems/nix-installer \
  --git-root ../nix-installer \
  --jwt-issuer-uri http://localhost:8081/jwt/token \
  --host http://localhost:8080
```

[flakehub]: https://flakehub.com
[flakes]: https://zero-to-nix.com/concepts/flakes

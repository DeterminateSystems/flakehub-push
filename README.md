# `flakehub-push`

[![FlakeHub](https://img.shields.io/endpoint?url=https://flakehub.com/f/DeterminateSystems/flakehub-push/badge)](https://flakehub.com/flake/DeterminateSystems/flakehub-push)

A GitHub Action for publishing [Nix flakes][flakes] to [FlakeHub].
Create a [YAML configuration](#configuration), push it to your repo, and you're ready to go.

## Configuration

There are two ways to get started configuring this Action:

1. Use our [configuration wizard](#guided-wizard) to create a configuration.
1. Configure the Action [manually](#manual-configuration).

### Guided wizard

Although the `flakehub-push` Action requires little configuration, you may benefit from assembling it with our friendly wizard at [flakehub.com/new][wizard].

### Manual configuration

The example workflow configuration below pushes new tags matching the conventional format&mdash;such as `v1.0.0` or `v0.1.0-rc4`&mdash;to [Flakehub]:

```yaml
# .github/workflows/flakehub-publish-tagged.yml
name: Publish tags to FlakeHub

on:
  push:
    tags:
      - v?[0-9]+.[0-9]+.[0-9]+*

jobs:
  flakehub:
    runs-on: ubuntu-22.04
    permissions:
      id-token: write # Necessary for authenticating against FlakeHub
      contents: read
    steps:
      - uses: actions/checkout@v4
        with:
          ref: ${{ (inputs.tag != null) && format('refs/tags/{0}', inputs.tag) || '' }}
      - name: Install Nix
        uses: DeterminateSystems/nix-installer-action@main
      - name: Push to FlakeHub
        uses: DeterminateSystems/flakehub-push@main
        with:
          # For the flake's visibility, you can also select "unlisted" if you don't want
          # it to show up in search results and general listings on flakehub.com
          visibility: public
          # Release rolling versions of the form 0.1.* instead of tagged releases
          rolling: true
```

Some other common configuration use cases are described in the sections below, along with a full listing of [all available parameters](#available-parameters).

#### Set flake visibility to public, private, or unlisted

Whenever you configure the `flakehub-push` Action, you need to specify the flake's [visibility] using the `visibility` parameter.
This configuration would make the flake public:

```yaml
- uses: DeterminateSystems/flakehub-push@main
  with:
    visibility: public
```

The available options are:

| Option     | What it means                                                                                                                                                                      |
| :--------- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `public`   | The flake is viewable and usable if you know the URL for the flake and it shows up in search results and on the [flake listing][all-flakes].                                       |
| `private`  | The flake is viewable and usable only by users who are authenticated and granted access to the flake. [Private flakes][private-flakes] are available only on [paid plans][signup]. |
| `unlisted` | The flake is viewable and usable only if you know the URL for it. It shows up neither in search results nor on the [flake listing][all-flakes].                                    |

#### Rolling releases

For [rolling releases][rolling], as in the example above, set `rolling` to `true`:

```yaml
- uses: DeterminateSystems/flakehub-push@main
  with:
    rolling: true
```

By default, the rolling minor version is 1, meaning that versions are of the form `0.1.[commit count]+rev-[git sha]`.
An example rolling version would be `0.1.1924+rev-ebfe2c639111d7e82972a12711206afaeeda2450`.
You can set a different rolling minor using the `rolling-minor` setting.
This configuration sets the rolling minor to 2:

```yaml
- uses: DeterminateSystems/flakehub-push@main
  with:
    rolling: true
    rolling-minor: 2
```

#### Tagged releases

Publishing [tagged releases][tagged] is a little bit trickier because you need to tell `flakehub-push` which tag to use.
Here's an example configuration:

```yaml
on:
  push:
    tags:
      - v?[0-9]+.[0-9]+.[0-9]+*
  workflow_dispatch:
    inputs:
      tag:
        description: The existing tag to publish to FlakeHub
        type: string
        required: true

jobs:
  flakehub-publish:
    runs-on: ubuntu-latest
    permissions:
      id-token: write
      contents: read
    steps:
      - uses: actions/checkout@v4
        with:
          # Checking out only the tag isn't necessary but should speed things up
          ref: ${{ inputs.tag }}
      - uses: DeterminateSystems/nix-installer-action@main
      - uses: DeterminateSystems/flakehub-push@main
        with:
          visibility: private
          tag: ${{ inputs.tag }}
```

#### Store output paths

[FlakeHub] has a feature called [resolved store paths][store-paths] that, when activated, evaluates and stores all of the store paths associated with your flake outputs.
To activate resolved store paths, set `include-output-paths` to `true`:

```yaml
- uses: DeterminateSystems/flakehub-push@main
  with:
    include-output-paths: true
```

This setting only makes a difference if you're using [FlakeHub Cache][cache].
You can [sign up at any time][signup] to take advantage of this feature.

#### Handling multiple flakes in one repository

You can use `flakehub-push` to publish multiple flakes in the same repository by keeping different flakes in different directories and using the `directory` parameter to specify the root of those flakes.
Here's an example configuration that would publish separate flakes in the `my-subflake-1` and `my-subflake-2` subdirectories:

```yaml
name: Publish multiple flakes to FlakeHub

on:
  push:
    branches:
      - main

jobs:
  flakehub-publish:
    runs-on: ubuntu-latest
    permissions:
      id-token: write
      contents: read
    steps:
      - uses: actions/checkout@v3
      - uses: DeterminateSystems/nix-installer-action@main

      # Publish my-subflake-1
      - uses: DeterminateSystems/flakehub-push@main
        with:
          rolling: true
          directory: my-subflake-1
          visibility: public

      # Publish my-subflake-2
      - uses: DeterminateSystems/flakehub-push@main
        with:
          rolling: true
          directory: my-subflake-2
          visibility: public
```

In this case, `flakehub-push` publishes rolling releases for both flakes every time there's a push to `main`.
But in other cases you may need to structure your configuration differently.
If different flakes have different release strategies, for example one flake uses tagged releases and another one uses rolling releases, you may need to provide different configurations in separate YAML files to accommodate separate `on` blocks.

#### Available parameters

| Parameter              | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                             | Type          | Required? | Default                    |
| :--------------------- | :---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :------------ | :-------- | :------------------------- |
| `visibility`           | `public`, `unlisted`, or `private`. [Private flakes][private-flakes] are available only on a [FlakeHub paid plan][signup].                                                                                                                                                                                                                                                                                                                                              | enum          | ✅        |                            |
| `repository`           | The GitHub repository containing your flake in the format of `{org}/{repo}`.                                                                                                                                                                                                                                                                                                                                                                                            | string        | ✅        | `${{ github.repository }}` |
| `name`                 | The name of your published flake in the format of `{org}/{name}`. The `{org}` must match your organization's GitHub root name or the publish will fail. Specify this only if you want to publish under a different name from the `{org}/{repo}`.                                                                                                                                                                                                                        | string        |           |                            |
| `include-output-paths` | Whether to expose store paths for the flake's outputs via the FlakeHub API. This is most useful when used in conjunction with [FlakeHub Cache][cache].                                                                                                                                                                                                                                                                                                                  | Boolean       |           | `false`                    |
| `mirror`               | Whether the repository is mirrored via DeterminateSystems' mirror functionality. This is only usable by DeterminateSystems.                                                                                                                                                                                                                                                                                                                                             | Boolean       |           | `false`                    |
| `directory`            | The path of your flake relative to the root of the repository. Useful for subflakes.                                                                                                                                                                                                                                                                                                                                                                                    | relative path |           |                            |
| `tag`                  | The Git tag to use for non-rolling releases. This must be the character `v` followed by a SemVer version, such as `v0.1.1`.                                                                                                                                                                                                                                                                                                                                             | string        |           |                            |
| `rev`                  | The Git revision SHA to use for non-rolling releases.                                                                                                                                                                                                                                                                                                                                                                                                                   | string        |           |                            |
| `rolling`              | For untagged releases, use a [rolling versioning scheme][rolling]. When this is enabled, the default versioning scheme is `0.1.[commit count]+rev-[git sha]`. To customize the [SemVer] minor version, set the `rolling-minor` option.                                                                                                                                                                                                                                  | Boolean       |           | `false`                    |
| `rolling-minor`        | Specify the [SemVer] minor version of your [rolling releases][rolling]. All releases will follow the versioning scheme `0.[rolling-minor].[commit count]+rev-[git sha]`.                                                                                                                                                                                                                                                                                                | string        |           |                            |
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

## Integration

The `flakehub-push` Action sets a handful of [outputs][gha-outputs] for integrating into continuous delivery pipelines:

| Output              | Description                                                                                                                                                                                                                    | Example                                                                                 |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------- |
| `flake_name`        | Name of the flake.                                                                                                                                                                                                             | `DeterminateSystems/flakehub-push`                                                      |
| `flake_version`     | Version of the published flake.                                                                                                                                                                                                | `0.1.99+rev-2075013a3f3544d45a96f4b35df4ed03cd53779c`                                   |
| `flakeref_exact`    | A precise reference that always resolves to this to this exact release.                                                                                                                                                        | `DeterminateSystems/flakehub-push/=0.1.99+rev-2075013a3f3544d45a96f4b35df4ed03cd53779c` |
| `flakeref_at_least` | A loose reference to this release. Depending on this reference will require at least this version, and will also resolve to newer releases. This output is not sufficient for deployment pipelines, use `flake_exact` instead. | `DeterminateSystems/flakehub-push/0.1.99+rev-2075013a3f3544d45a96f4b35df4ed03cd53779c`  |

Here's an example Actions workflow that uses these outputs.
After the flake is published, the `Notify external system` step uses [cURL] to notify an external web service that the flake has been successfully published by including the flake's version in a JSON object:

```yaml
name: Notify external system that flake has been published

on:
  push:
    branches:
      - main

jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: DeterminateSystems/nix-installer-action@main # Install Nix
      - uses: DeterminateSystems/flakehub-push@main # Publish to FlakeHub
        id: flakehub_push
        with:
          visibility: private
          rolling: true
          include-output-paths: true

      - name: Notify external system
        run: |
          curl -XPOST https://my-recording-system.dev \
            -H "Content-Type: application/json" \
            -H "Bearer: ${{ secrets.RECORDING_SYSTEM_API_KEY }}" \
            -d '{"flake_version":"${{ steps.flakehub_push.outputs.version }}"}'
```

## Platform Support

This action supports publishing Apple Silicon, `aarch64-linux`, and `x86_64-linux`.
Your flake only needs to be published once from a single architecture to cover all architectures your flake supports.
In other words: all you need to do to publish a flake that supports `x86_64-darwin` is run the `flakehub-push` action from any other architecture, like `x86_64-linux`.

## Developing `flakehub-push`

See the [development docs](./docs/development.md).

[all-flakes]: https://flakehub.com/flakes
[cache]: https://flakehub.com/cache
[curl]: https://curl.se
[flakehub]: https://flakehub.com
[flakes]: https://zero-to-nix.com/concepts/flakes
[gha-outputs]: https://docs.github.com/en/actions/writing-workflows/choosing-what-your-workflow-does/passing-information-between-jobs
[private-flakes]: https://docs.determinate.systems/flakehub/private-flakes
[rolling]: https://docs.determinate.systems/flakehub/concepts/versioning#rolling
[semver]: https://docs.determinate.systems/flakehub/concepts/semver
[signup]: https://flakehub.com/signup
[store-paths]: https://docs.determinate.systems/flakehub/store-paths
[tagged]: https://docs.determinate.systems/flakehub/concepts/versioning#tagged
[visibility]: https://docs.determinate.systems/flakehub/concepts/visibility
[wizard]: https://flakehub.com/new

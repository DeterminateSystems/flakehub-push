# `flakehub-push`

A [flakehub](https://flakehub.com/) pusher.

## Example

The following workflow will push new tags matching the conventional format (eg.
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
      id-token: write # Authenticate against FlakeHub
      contents: read
    steps:
      - uses: DeterminateSystems/nix-installer-action@v4
      - uses: actions/checkout@v3
      - name: Push to flakehub
        uses: determinatesystems/flakehub-push@main
        with:
          visibility: "hidden" # or "public"
```

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

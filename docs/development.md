## Developing `flakehub-push`

You can run `flakehub-push` against a Flakehub server running locally.
Assuming the dev environment is running as described in the FlakeHub repo:

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

To test evaluation of a local flake without fetching anything from
GitHub, and writing the tarball and metadata to a local directory
instead of FlakeHub, do:

```bash
cargo run -- \
  --visibility public \
  --repository foo/bar \
  --tag v0.0.1 \
  --git-root /path/to/repo \
  --directory /path/to/repo/flake \
  --dest-dir /tmp/out
```

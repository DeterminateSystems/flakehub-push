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

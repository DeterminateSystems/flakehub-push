# `flakehub-push`

A [flakehub](https://flakehub.com/) pusher.

## Example

```yaml
name: flakehub

on:
  workflow_dispatch:
  push:
    branches:
      - "main"

jobs:
  production-test:
    runs-on: ubuntu-22.04
    permissions:
      id-token: write # In order to request a JWT for AWS auth
      contents: read # Specifying id-token wiped this out, so manually specify that this action is allowed to checkout this private repo
    steps:
      - uses: actions/checkout@v3
      - name: Push to flakehub
        uses: determinatesystems/flakehub
        with:
          visibility: "hidden" # or "public"
```

## Use with local Flakehub server

Assuming the dev environment is running as described in the flakehub repo:

```
GITHUB_REPOSITORY=determinatesystems/flakehub-push cargo run -- --visibility hidden --jwt-issuer-uri http://localhost:8081/jwt/token --tag v0.0.$RANDOM --host http://localhost:8080
```

TODO: autodetect more of this.

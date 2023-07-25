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
#!/bin/sh

host=https://nxfr.fly.dev
#host=http://127.0.0.1:8080
#host=https://6012-2603-7081-338-c252-0-e41d-2d20-3c20.ngrok-free.app/
set -eux

scratch=$(mktemp -d -t tmp.XXXXXXXXXX)
function finish {
  rm -rf "$scratch"
}
trap finish EXIT

visibility=$1
mirroredFrom=$1

src=$(nix flake metadata --json | nix run nixpkgs#jq -- -r .path)

(
    cd "$src/.."
    tar -czf "$scratch/source.tar.gz" "$(basename "$src")"
)

cp "$scratch/source.tar.gz" ./

hash=$(shasum -a 256 "$scratch/source.tar.gz" | cut -f1 -d\ | nix shell nixpkgs#vim -c xxd -r -p | base64)
len=$(wc --bytes < "$scratch/source.tar.gz")

token=$(curl -H "Authorization: bearer $ACTIONS_ID_TOKEN_REQUEST_TOKEN" "$ACTIONS_ID_TOKEN_REQUEST_URL&audience=api://AzureADTokenExchange" | nix run nixpkgs#jq -- -r .value)

metadata() (
  if [ -f ./README.md ]; then
    nix flake metadata --json \
        | nix run nixpkgs#jq -- '{ "description": .description, "visibility": $visibility, "readme": $readme, "mirrored_from": ($mirrored_from | select(. != "") // null) }' \
          --rawfile readme ./README.md \
          --arg visibility "$visibility" \
          --arg mirrored_from "$mirroredFrom"
  else
    nix flake metadata --json \
        | nix run nixpkgs#jq -- '{ "description": .description, "visibility": $visibility, "readme": null, "mirrored_from": ($mirrored_from | select(. != "") // null) }' \
          --arg visibility "$visibility" \
          --arg mirrored_from "$mirroredFrom"
  fi
)

url=$(
  metadata \
    | curl \
      --header "ngrok-skip-browser-warning: please" \
      --header "Authorization: bearer $token" \
      --header "Content-Type: application/json" \
      -X POST \
      -d @- \
      "$host/upload/$GITHUB_REPOSITORY/$GITHUB_REF_NAME/$len/$hash"
)
curl \
  -X PUT \
  --header "content-length: $len" \
  --header "x-amz-checksum-sha256: $hash" \
  -T "$scratch/source.tar.gz" \
  -L "$url"

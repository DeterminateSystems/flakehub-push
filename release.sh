#!/bin/bash

host=https://nxfr.fly.dev
#host=http://127.0.0.1:8080
#host=https://6012-2603-7081-338-c252-0-e41d-2d20-3c20.ngrok-free.app/
set -eux
set -o pipefail

scratch=$(mktemp -d -t tmp.XXXXXXXXXX)
function finish {
  rm -rf "$scratch"
}
trap finish EXIT

visibility=$1
name=$2
mirroredFrom=$3
chosenTag=$4
rollingPrefix=$5


reponame=$GITHUB_REPOSITORY
tag=$GITHUB_REF_NAME


if [ "$mirroredFrom" != "" ]; then
  reponame=$mirroredFrom
fi

if [ "$chosenTag" != "" ]; then
  tag=$chosenTag
fi

if [ "$name" != "" ]; then
  reponame="$name"
fi

owner=$(echo "$reponame" | sed -e 's#/.*$##');
repo=$(echo "$reponame" | sed -e 's#^.*/##');

revision=$(git rev-parse HEAD)
revisionshort=$(git rev-parse --short HEAD)
revCount=$((nix run nixpkgs#gh -- api graphql --paginate -f query='
query {
  repository(owner:"'"$owner"'", name:"'"$repo"'") {
    object(expression:"'"$revision"'") {
      ... on Commit {
        history {
          totalCount
        }
      }
    }
  }
}
' || true) | nix run nixpkgs#jq -- -r '.data.repository.object.history.totalCount // null'
)

if [ "$rollingPrefix" != "" ]; then
  tag="${rollingPrefix}.$revCount+rev-$revisionshort"
fi

# Generate a .json document with the readme, or null
(
  if [ -f ./README.md ]; then
    nix run nixpkgs#jq -- -n '$readme' --rawfile readme ./README.md
  else
    echo null
  fi
) > "$scratch/readme.json"

# Generate the overall release's metadata document
(
  nix flake metadata --json \
      | nix run nixpkgs#jq -- '
        {
          "description": .description,
          "raw_flake_metadata": .,
          "mirrored_from": ($mirrored_from | select(. != "") // null),
          "readme": ($readme | first),
          "revision": (.revision // null),
          "commit_count": $revCount,
          "visibility": $visibility
        }' \
        --arg mirrored_from "$mirroredFrom" \
        --arg revision "$revision" \
        --arg visibility "$visibility" \
        --argjson revCount "$revCount" \
        --slurpfile readme "$scratch/readme.json" \
        > "$scratch/metadata.json"
)

src=$(nix flake metadata --json | nix run nixpkgs#jq -- -r '.path + "/" + (.resolved.dir // "")')

(
    cd "$src/.."
    tar -czf "$scratch/source.tar.gz" "$(basename "$src")"
)

cat "$scratch/metadata.json" | nix run nixpkgs#jq -- -r '.'

echo "Checking your flake for evaluation safety..."
if nix flake show file://"$scratch/source.tar.gz"; then
  echo "...ok!"
else
  echo "failed!"
  exit 1
fi

hash=$(shasum -a 256 "$scratch/source.tar.gz" | cut -f1 -d\ | nix shell nixpkgs#vim -c xxd -r -p | base64)
len=$(wc --bytes < "$scratch/source.tar.gz")

token=$(curl \
  --fail \
  --header "Authorization: bearer $ACTIONS_ID_TOKEN_REQUEST_TOKEN" \
  "$ACTIONS_ID_TOKEN_REQUEST_URL&audience=api://AzureADTokenExchange" \
  | nix run nixpkgs#jq -- -r .value)

cat "$scratch/metadata.json" \
  | curl \
    --fail \
    --header "ngrok-skip-browser-warning: please" \
    --header "Authorization: bearer $token" \
    --header "Content-Type: application/json" \
    -X POST \
    -d @- \
    "$host/upload/$reponame/$tag/$len/$hash"


url=$(
  cat "$scratch/metadata.json" \
    | curl \
      --fail \
      --header "ngrok-skip-browser-warning: please" \
      --header "Authorization: bearer $token" \
      --header "Content-Type: application/json" \
      -X POST \
      -d @- \
      "$host/upload/$reponame/$tag/$len/$hash"
)
curl \
  --fail \
  -X PUT \
  --header "content-length: $len" \
  --header "x-amz-checksum-sha256: $hash" \
  -T "$scratch/source.tar.gz" \
  -L "$url"

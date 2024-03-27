#!/usr/bin/env bash
set -x
set -euo pipefail

DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
cd "${DIR}/.."

export CARGO_BUILD_TARGET="x86_64-unknown-linux-musl";
export CARGO_BUILD_RUSTFLAGS="-C target-feature=+crt-static";
# export CARGO_PROFILE_DEV_STRIP="debuginfo"

cargo watch -x build

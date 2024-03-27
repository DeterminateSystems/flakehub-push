#!/usr/bin/env bash
set -x
set -euo pipefail

DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
cd "${DIR}/.."

miniserve -i :: --port 9898 ./target/x86_64-unknown-linux-musl/debug

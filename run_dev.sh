#!/usr/bin/env bash
# Build and run a development (debug) build of ExeyVue.
#
#   ./run_dev.sh                  build + run
#   ./run_dev.sh photo.png        build + run, opening a file
#   NO_JXL=1 ./run_dev.sh         skip the libjxl build (much faster first compile)
#   CHECK=1 ./run_dev.sh          only type-check (cargo check), don't run
#
# Extra cargo flags can go in CARGO_FLAGS, e.g. CARGO_FLAGS="--release".
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found — install Rust from https://rustup.rs" >&2
  exit 1
fi

args=()
if [[ "${NO_JXL:-0}" == "1" ]]; then
  args+=(--no-default-features)
fi
# shellcheck disable=SC2206
args+=(${CARGO_FLAGS:-})

export RUST_BACKTRACE="${RUST_BACKTRACE:-1}"

if [[ "${CHECK:-0}" == "1" ]]; then
  exec cargo check ${args[@]+"${args[@]}"}
fi

exec cargo run ${args[@]+"${args[@]}"} -- "$@"

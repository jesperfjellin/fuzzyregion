#!/usr/bin/env bash

# Canonical full-suite test entrypoint for the repository.
# It runs the Rust test suite first and then the Docker-backed PostgreSQL 18 /
# PostGIS integration smoke test.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "$repo_root"

cargo test
"$repo_root/scripts/test-postgres.sh"

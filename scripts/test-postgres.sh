#!/usr/bin/env bash

# Starts an ephemeral PostgreSQL 18/PostGIS container, installs `fuzzyregion`,
# runs SQL smoke assertions, and always removes the container afterwards.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

postgis_image="postgis/postgis:18-3.6"
postgres_user="postgres"
postgres_password="postgres"
postgres_db="fuzzyregion_test"
smoke_sql="$repo_root/crates/fuzzyregion-pg/tests/sql/pg18_smoke.sql"

container_name="fuzzyregion-pg18-test-$$-$(date +%s)"

status() {
  echo "[fuzzyregion:test] $*"
}

dump_diagnostics() {
  local exit_code="$1"

  if [[ "$exit_code" -eq 0 ]]; then
    return
  fi

  {
    echo "Docker smoke test failed."
    docker ps -a --filter "name=^/${container_name}$"
    docker inspect "$container_name" --format 'status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} error={{.State.Error}}'
    echo "Container logs:"
    docker logs "$container_name"
  } >&2 || true
}

cleanup() {
  local exit_code="$1"
  dump_diagnostics "$exit_code"
  docker rm -f "$container_name" >/dev/null 2>&1 || true
}

trap 'cleanup $?' EXIT

cd "$repo_root"

status "Starting ephemeral PostGIS container: $postgis_image"
docker run \
  --detach \
  --name "$container_name" \
  --env "POSTGRES_USER=$postgres_user" \
  --env "POSTGRES_PASSWORD=$postgres_password" \
  --env "POSTGRES_DB=$postgres_db" \
  "$postgis_image" \
  >/dev/null

status "Waiting for PostgreSQL container initialization to complete."
for _ in $(seq 1 90); do
  if docker logs "$container_name" 2>&1 | grep -q "PostgreSQL init process complete; ready for start up."; then
    break
  fi
  sleep 1
done

docker logs "$container_name" 2>&1 | grep -q "PostgreSQL init process complete; ready for start up."
docker exec "$container_name" pg_isready -U "$postgres_user" -d "$postgres_db" >/dev/null

FUZZYREGION_CONTAINER_NAME="$container_name" \
  "$repo_root/scripts/install-postgres-extension.sh"

status "Running PostgreSQL smoke assertions."
docker exec -i "$container_name" psql -v ON_ERROR_STOP=1 -U "$postgres_user" -d "$postgres_db" -f - < "$smoke_sql"

status "PostgreSQL smoke test passed."

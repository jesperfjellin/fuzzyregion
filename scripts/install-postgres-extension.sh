#!/usr/bin/env bash

# Packages the fuzzyregion extension for the configured PostgreSQL major,
# copies the artifacts into an already-running PostgreSQL/PostGIS container,
# and creates the extension in the target database.
#
# Required:
# - FUZZYREGION_CONTAINER_NAME
#
# Optional:
# - FUZZYREGION_POSTGRES_USER (default: postgres)
# - FUZZYREGION_POSTGRES_DB (default: postgres)
# - FUZZYREGION_PG_CONFIG (default: /usr/lib/postgresql/18/bin/pg_config)
# - FUZZYREGION_LOG_PREFIX (default: [fuzzyregion:install])

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

container_name="${FUZZYREGION_CONTAINER_NAME:?FUZZYREGION_CONTAINER_NAME must be set}"
postgres_user="${FUZZYREGION_POSTGRES_USER:-postgres}"
postgres_db="${FUZZYREGION_POSTGRES_DB:-postgres}"
pg_config="${FUZZYREGION_PG_CONFIG:-/usr/lib/postgresql/18/bin/pg_config}"
log_prefix="${FUZZYREGION_LOG_PREFIX:-[fuzzyregion:install]}"

status() {
  echo "${log_prefix} $*"
}

cd "$repo_root"

pg_major="$("$pg_config" --version | sed -E 's/.* ([0-9]+)(\.[0-9]+)?.*/\1/')"
package_version="$(cargo pkgid -p fuzzyregion-pg | sed 's/.*#//')"
package_root="$repo_root/target/release/fuzzyregion-pg${pg_major}"
control_file="$package_root/usr/share/postgresql/${pg_major}/extension/fuzzyregion.control"
sql_file="$package_root/usr/share/postgresql/${pg_major}/extension/fuzzyregion--${package_version}.sql"
shared_library="$package_root/usr/lib/postgresql/${pg_major}/lib/fuzzyregion.so"

status "Packaging extension artifacts for PostgreSQL ${pg_major}."
cargo pgrx package --package fuzzyregion-pg --pg-config "$pg_config"

status "Installing packaged files into container ${container_name}."
docker cp "$control_file" "$container_name:/usr/share/postgresql/${pg_major}/extension/fuzzyregion.control" >/dev/null
docker cp "$sql_file" "$container_name:/usr/share/postgresql/${pg_major}/extension/fuzzyregion--${package_version}.sql" >/dev/null
docker cp "$shared_library" "$container_name:/usr/lib/postgresql/${pg_major}/lib/fuzzyregion.so" >/dev/null

status "Creating extension fuzzyregion in database ${postgres_db}."
docker exec "$container_name" psql -v ON_ERROR_STOP=1 -U "$postgres_user" -d "$postgres_db" -c \
  "CREATE EXTENSION IF NOT EXISTS fuzzyregion;"

status "Extension install completed."

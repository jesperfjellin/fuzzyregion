#!/usr/bin/env bash

# Loads the shipped Po Valley tree-cover transition demo assets into an
# already-running PostgreSQL/PostGIS container.
#
# Required:
# - FUZZYREGION_CONTAINER_NAME
#
# Optional:
# - FUZZYREGION_POSTGRES_USER (default: postgres)
# - FUZZYREGION_POSTGRES_DB (default: postgres)
# - FUZZYREGION_DEMO_DIR
# - FUZZYREGION_LOG_PREFIX

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

container_name="${FUZZYREGION_CONTAINER_NAME:?FUZZYREGION_CONTAINER_NAME must be set}"
postgres_user="${FUZZYREGION_POSTGRES_USER:-postgres}"
postgres_db="${FUZZYREGION_POSTGRES_DB:-postgres}"
demo_dir="${FUZZYREGION_DEMO_DIR:-$repo_root/examples/demo-data/po_valley_tree_cover_transition}"
log_prefix="${FUZZYREGION_LOG_PREFIX:-[fuzzyregion:demo]}"
container_demo_dir="/tmp/fuzzyregion-demo"

status() {
  echo "${log_prefix} $*"
}

require_file() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    echo "${log_prefix} Missing required file: $path" >&2
    exit 1
  fi
}

moderate_a02="$demo_dir/moderate_tree_alpha_0_2.geojson"
moderate_a04="$demo_dir/moderate_tree_alpha_0_4.geojson"
moderate_a06="$demo_dir/moderate_tree_alpha_0_6.geojson"
high_a02="$demo_dir/high_tree_alpha_0_2.geojson"
high_a04="$demo_dir/high_tree_alpha_0_4.geojson"
high_a06="$demo_dir/high_tree_alpha_0_6.geojson"

require_file "$moderate_a02"
require_file "$moderate_a04"
require_file "$moderate_a06"
require_file "$high_a02"
require_file "$high_a04"
require_file "$high_a06"

status "Copying shipped demo GeoJSON assets into ${container_name}."
docker exec "$container_name" mkdir -p "$container_demo_dir"
docker cp "$moderate_a02" "$container_name:$container_demo_dir/moderate_tree_alpha_0_2.geojson" >/dev/null
docker cp "$moderate_a04" "$container_name:$container_demo_dir/moderate_tree_alpha_0_4.geojson" >/dev/null
docker cp "$moderate_a06" "$container_name:$container_demo_dir/moderate_tree_alpha_0_6.geojson" >/dev/null
docker cp "$high_a02" "$container_name:$container_demo_dir/high_tree_alpha_0_2.geojson" >/dev/null
docker cp "$high_a04" "$container_name:$container_demo_dir/high_tree_alpha_0_4.geojson" >/dev/null
docker cp "$high_a06" "$container_name:$container_demo_dir/high_tree_alpha_0_6.geojson" >/dev/null

tmp_sql="$(mktemp)"
trap 'rm -f "$tmp_sql"' EXIT

cat >"$tmp_sql" <<SQL
\set ON_ERROR_STOP on

CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION IF NOT EXISTS fuzzyregion;

DROP SCHEMA IF EXISTS fuzzyregion_demo CASCADE;
CREATE SCHEMA fuzzyregion_demo;

CREATE OR REPLACE FUNCTION fuzzyregion_demo.load_union_multipolygon(path text)
RETURNS geometry
LANGUAGE SQL
AS \$\$
  WITH raw AS (
    SELECT pg_read_file(path)::jsonb AS doc
  ),
  geoms AS (
    SELECT ST_SetSRID(
             ST_GeomFromGeoJSON(feature.value -> 'geometry'),
             3035
           ) AS geom
    FROM raw
    CROSS JOIN LATERAL jsonb_array_elements(raw.doc -> 'features') AS feature(value)
  )
  SELECT ST_Multi(ST_CollectionExtract(ST_Union(geom), 3))
  FROM geoms
\$\$;

CREATE TABLE fuzzyregion_demo.tree_cover_class (
  id integer PRIMARY KEY,
  class_label text UNIQUE NOT NULL,
  area fuzzyregion NOT NULL
);

INSERT INTO fuzzyregion_demo.tree_cover_class (id, class_label, area)
VALUES (
  1,
  'moderate_tree_cover',
  fuzzyregion_from_geoms(
    ARRAY[0.2, 0.4, 0.6],
    ARRAY[
      fuzzyregion_demo.load_union_multipolygon('${container_demo_dir}/moderate_tree_alpha_0_2.geojson'),
      fuzzyregion_demo.load_union_multipolygon('${container_demo_dir}/moderate_tree_alpha_0_4.geojson'),
      fuzzyregion_demo.load_union_multipolygon('${container_demo_dir}/moderate_tree_alpha_0_6.geojson')
    ]
  )
);

INSERT INTO fuzzyregion_demo.tree_cover_class (id, class_label, area)
VALUES (
  2,
  'high_tree_cover',
  fuzzyregion_from_geoms(
    ARRAY[0.2, 0.4, 0.6],
    ARRAY[
      fuzzyregion_demo.load_union_multipolygon('${container_demo_dir}/high_tree_alpha_0_2.geojson'),
      fuzzyregion_demo.load_union_multipolygon('${container_demo_dir}/high_tree_alpha_0_4.geojson'),
      fuzzyregion_demo.load_union_multipolygon('${container_demo_dir}/high_tree_alpha_0_6.geojson')
    ]
  )
);

DROP FUNCTION fuzzyregion_demo.load_union_multipolygon(text);
SQL

status "Loading demo dataset into ${postgres_db} on container ${container_name}."
docker exec -i "$container_name" psql -v ON_ERROR_STOP=1 -U "$postgres_user" -d "$postgres_db" -f - < "$tmp_sql"

status "Demo dataset loaded into schema fuzzyregion_demo."

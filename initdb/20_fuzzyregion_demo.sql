\set ON_ERROR_STOP on

-- Connect to the fuzzyregion database (PostGIS is already loaded by 10_postgis.sh).
\c fuzzyregion

CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION IF NOT EXISTS fuzzyregion;

CREATE SCHEMA fuzzyregion_demo;

-- Helper: load a GeoJSON FeatureCollection and union its geometries into a
-- single MultiPolygon in EPSG:3035.
CREATE OR REPLACE FUNCTION fuzzyregion_demo.load_union_multipolygon(path text)
RETURNS geometry
LANGUAGE SQL
AS $$
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
$$;

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
      fuzzyregion_demo.load_union_multipolygon('/demo-data/moderate_tree_alpha_0_2.geojson'),
      fuzzyregion_demo.load_union_multipolygon('/demo-data/moderate_tree_alpha_0_4.geojson'),
      fuzzyregion_demo.load_union_multipolygon('/demo-data/moderate_tree_alpha_0_6.geojson')
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
      fuzzyregion_demo.load_union_multipolygon('/demo-data/high_tree_alpha_0_2.geojson'),
      fuzzyregion_demo.load_union_multipolygon('/demo-data/high_tree_alpha_0_4.geojson'),
      fuzzyregion_demo.load_union_multipolygon('/demo-data/high_tree_alpha_0_6.geojson')
    ]
  )
);

DROP FUNCTION fuzzyregion_demo.load_union_multipolygon(text);

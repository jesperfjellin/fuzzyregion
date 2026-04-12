# fuzzyregion

fuzzyregion is a PostgreSQL extension for storing and querying fuzzy regions on top of PostGIS.

The project is inspired by the 2025 paper [*Fuzzy Spatial Algebra (FUSA): Formal Specification of Fuzzy Spatial Data Types and Operations for Databases and GIS*](https://dl.acm.org/doi/10.1145/3722555), but it does not attempt to implement full FUSA from the start.

The MVP focuses on a single first-class SQL type, `fuzzyregion`, represented as a finite stack of nested alpha-cut polygons. This makes it possible to store vague areal phenomena in PostgreSQL, extract crisp views such as support, core, and alpha-cuts, and perform a small set of fuzzy set operations while relying on PostGIS as the underlying crisp geometry engine.

This is intentionally the `fregion` slice of the broader FUSA direction. First-class fuzzy points (`fpoint`) and fuzzy lines (`fline`) remain future work rather than discarded scope.

## Current status

The extension currently implements the core MVP surface:

- `fuzzyregion_from_geoms` and `fuzzyregion_from_ewkb`
- validation and inspection helpers
- `support`, `core`, `alpha_cut`, `membership_at`, `bbox`, and `area_at`
- standard `union`, `intersection`, and `difference`
- membership transforms: normalize, concentrate, and dilate
- JSON/text export helpers

## Testing

The full-suite test entrypoint is:

`./scripts/tests.sh`

That script runs the Rust test suite first and then executes the PostgreSQL 18/PostGIS smoke test in an ephemeral Docker container.

For a Rust-only pass without Docker, run:

`cargo test`

The lower-level Docker integration entrypoint remains available at `scripts/test-postgres.sh`, but `scripts/tests.sh` is the intended way to run the repository test suite end to end.

## Try it

Build and start a PostgreSQL 18 + PostGIS instance with `fuzzyregion` pre-installed:

```bash
docker compose up --build -d
```

Once the container is healthy, exec into it:

```bash
docker exec -it fuzzyregion-db-1 psql -U postgres -d fuzzyregion
```

The image ships with `fuzzyregion` and PostGIS already enabled and a demo dataset loaded into the `fuzzyregion_demo` schema — two `fuzzyregion` values representing moderate and high tree cover in the Po Valley (EPSG:3035). Data persists across container restarts.

```sql
-- A fuzzyregion is a single value in a single column
SELECT class_label, pg_column_size(area) AS bytes, pg_typeof(area) AS type
FROM fuzzyregion_demo.tree_cover_class
WHERE class_label = 'moderate_tree_cover';

-- That single value contains three nested alpha-cut levels
SELECT class_label, alpha, fuzzyregion_area_at(area, alpha) / 1e6 AS level_area_km2
FROM fuzzyregion_demo.tree_cover_class,
     LATERAL unnest(ARRAY[0.2, 0.4, 0.6]) AS alpha
WHERE class_label = 'moderate_tree_cover';

-- Compute a fuzzy intersection and store it as a new row
INSERT INTO fuzzyregion_demo.tree_cover_class (id, class_label, area)
VALUES (
  3,
  'transition_zone',
  fuzzyregion_intersection(
    (SELECT area FROM fuzzyregion_demo.tree_cover_class WHERE class_label = 'moderate_tree_cover'),
    (SELECT area FROM fuzzyregion_demo.tree_cover_class WHERE class_label = 'high_tree_cover')
  )
);

-- The derived value is stored and queryable like any other column
SELECT class_label, pg_column_size(area) AS bytes, fuzzyregion_area_at(area, 0.2) / 1e6 AS area_km2
FROM fuzzyregion_demo.tree_cover_class;
```

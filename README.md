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

## Quickstart

The quickest way to try `fuzzyregion` is a single command:

```bash
./scripts/run_demo.py
```

That script:

- starts an ephemeral PostgreSQL 18 + PostGIS container
- installs `fuzzyregion`
- loads the shipped constructor inputs for two `fuzzyregion` values
- stores and queries each `fuzzyregion` value directly
- composes them with `fuzzyregion_intersection`
- writes a machine-readable demo summary
- exports each demo `fuzzyregion` value as a tuple-level JSON file
- exports each demo `fuzzyregion` value as a GeoJSON level view for inspection
- renders an SVG plot from the actual database result
- removes the container afterwards

If you want the smaller synthetic example instead:

```bash
docker run --detach \
  --name fuzzyregion-demo \
  --env POSTGRES_USER=postgres \
  --env POSTGRES_PASSWORD=postgres \
  --env POSTGRES_DB=fuzzyregion_demo \
  postgis/postgis:18-3.6

FUZZYREGION_CONTAINER_NAME=fuzzyregion-demo \
FUZZYREGION_POSTGRES_DB=fuzzyregion_demo \
./scripts/install-postgres-extension.sh

docker exec -i fuzzyregion-demo \
  psql -v ON_ERROR_STOP=1 -U postgres -d fuzzyregion_demo \
  -f /dev/stdin < examples/sample_dataset.sql

docker exec -i fuzzyregion-demo \
  psql -v ON_ERROR_STOP=1 -U postgres -d fuzzyregion_demo \
  -f /dev/stdin < examples/demo_queries.sql
```

When you are done:

```bash
docker rm -f fuzzyregion-demo
```

## Repo guide

- [`examples/sample_dataset.sql`](/home/jespe/github/fuzzyregion/examples/sample_dataset.sql): minimal demo tables and sample `fuzzyregion` values
- [`examples/demo_queries.sql`](/home/jespe/github/fuzzyregion/examples/demo_queries.sql): example queries covering inspection, projections, set operations, and membership transforms
- [`examples/tree_cover_transition_demo_queries.sql`](/home/jespe/github/fuzzyregion/examples/tree_cover_transition_demo_queries.sql): concise query set showing representation, querying, and composition for the shipped tree-cover transition demo
- [`examples/demo-data/po_valley_tree_cover_transition`](/home/jespe/github/fuzzyregion/examples/demo-data/po_valley_tree_cover_transition): shipped finalized alpha-cut inputs for the real-data demo
- [`examples/demo-output/po_valley_tree_cover_transition`](/home/jespe/github/fuzzyregion/examples/demo-output/po_valley_tree_cover_transition): generated demo summary, SVG plot, tuple-level JSON exports, and GeoJSON inspection views from the demo run
- [`examples/habitat_pollution_use_case.sql`](/home/jespe/github/fuzzyregion/examples/habitat_pollution_use_case.sql): focused query showing how a fuzzy habitat/pollution overlap becomes a decision-ready object
- [`docs/use-case-habitat-pollution.md`](/home/jespe/github/fuzzyregion/docs/use-case-habitat-pollution.md): worked practical example with a visual figure
- [`docs/use-case-tree-cover-transition.md`](/home/jespe/github/fuzzyregion/docs/use-case-tree-cover-transition.md): chosen real-data case study for one tree-cover raster, two fuzzy classes, and their transition zone
- [`docs/figures/habitat_pollution_use_case.svg`](/home/jespe/github/fuzzyregion/docs/figures/habitat_pollution_use_case.svg): generated visual of the use case
- [`docs/benchmark-notes.md`](/home/jespe/github/fuzzyregion/docs/benchmark-notes.md): current benchmarking guidance and limits
- [`scripts/install-postgres-extension.sh`](/home/jespe/github/fuzzyregion/scripts/install-postgres-extension.sh): install helper for an already-running PostgreSQL/PostGIS container
- [`scripts/load-tree-cover-transition-demo.sh`](/home/jespe/github/fuzzyregion/scripts/load-tree-cover-transition-demo.sh): load the shipped real-data demo assets into PostgreSQL
- [`scripts/run_demo.py`](/home/jespe/github/fuzzyregion/scripts/run_demo.py): one-command real-data demo runner that loads two fuzzyregion values, composes them, and writes a summary plus plot
- [`scripts/prepare-tree-cover-transition-demo.sh`](/home/jespe/github/fuzzyregion/scripts/prepare-tree-cover-transition-demo.sh): authoring-only preprocessing path for regenerating the shipped demo assets
- [`scripts/test-postgres.sh`](/home/jespe/github/fuzzyregion/scripts/test-postgres.sh): ephemeral PostgreSQL 18/PostGIS smoke test

## Why The Demo Matters

The important part of the Po Valley demo is not the domain story. The tree-cover raster is just understandable scaffolding for three type-level facts:

- representation: panels `a` and `b` are stored `fuzzyregion` values
- composition: panel `c` is another `fuzzyregion` returned by `fuzzyregion_intersection(...)`
- querying: the demo reads crisp views such as `alpha_cut` and `area_at` back from that result in SQL

The selected first real-data case study is here:

- [`docs/use-case-tree-cover-transition.md`](/home/jespe/github/fuzzyregion/docs/use-case-tree-cover-transition.md)

The older habitat/pollution write-up remains available as a secondary paper-inspired example:

- [`docs/use-case-habitat-pollution.md`](/home/jespe/github/fuzzyregion/docs/use-case-habitat-pollution.md)

The public demo boundary is intentionally narrow:

- shipped finalized demo assets
- one Python runner
- one demo summary
- one plot

The raw raster preprocessing path exists only so the repository can reproduce
those shipped assets when needed. It is not the main demo.

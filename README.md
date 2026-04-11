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

## Installing into your own Postgres

The Quickstart above runs `fuzzyregion` in a throwaway container. To use it against a Postgres you already run — to write your own SQL, store `fuzzyregion` values, and have them persist across restarts — install it directly with `cargo pgrx install`, the standard pgrx toolchain command.

This is the local developer install path. `fuzzyregion` is not yet distributed as an apt/yum package or through PGXN, so there is no "package manager" route today; building from source against your host Postgres is the supported way to try the extension outside the demo.

### Prerequisites

- PostgreSQL 14-18 installed on the host, with matching server development headers (for example `postgresql-server-dev-18` on Debian/Ubuntu)
- PostGIS 3.x available in the target database
- A Rust toolchain (`rustup` recommended)
- `clang` / `libclang-dev` (pgrx uses bindgen)
- `cargo-pgrx`, installed once with `cargo install cargo-pgrx --locked`

### Install

Register your system Postgres with pgrx, then build and install the extension files into it:

```bash
cargo pgrx init --pg18 $(which pg_config)

cargo pgrx install \
  --package fuzzyregion-pg \
  --pg-config $(which pg_config) \
  --release
```

Replace `--pg18` and the `pg_config` path with whatever matches your install — for example `--pg17 /usr/lib/postgresql/17/bin/pg_config` on a Debian/Ubuntu host running PostgreSQL 17. `cargo pgrx init` only needs to run the first time you install `fuzzyregion` into a given major version.

`cargo pgrx install` drops three files into the directories that `pg_config` reports:

- `fuzzyregion.control` → `$(pg_config --sharedir)/extension/`
- `fuzzyregion--<version>.sql` → `$(pg_config --sharedir)/extension/`
- `fuzzyregion.so` → `$(pg_config --pkglibdir)/`

From Postgres's point of view this is indistinguishable from any other extension install — those are the same paths a distro package would eventually target.

### Enable the extension

In the database where you want to use `fuzzyregion`:

```sql
CREATE EXTENSION postgis;
CREATE EXTENSION fuzzyregion;
```

`fuzzyregion` depends on PostGIS, so PostGIS must be available in the same database. After that, the SQL surface listed under [Current status](#current-status) is available in that database.

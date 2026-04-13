ARG POSTGIS_TAG=18-3.6

# ── build stage: compile fuzzyregion from source ─────────────────────
FROM postgis/postgis:${POSTGIS_TAG} AS builder

ARG PG_MAJOR=18
ARG RUST_VERSION=1.94.0
ARG PGRX_VERSION=0.17.0

USER root
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential ca-certificates curl \
      clang libclang-dev pkg-config \
      postgresql-server-dev-${PG_MAJOR} \
 && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain ${RUST_VERSION}
ENV PATH="/root/.cargo/bin:${PATH}"

RUN cargo install cargo-pgrx --version "=${PGRX_VERSION}" --locked
RUN cargo pgrx init --pg${PG_MAJOR} $(which pg_config)

COPY . /build
WORKDIR /build

RUN cargo pgrx package \
      --package fuzzyregion-pg \
      --pg-config $(which pg_config)

# ── final image: postgis + fuzzyregion artifacts ─────────────────────
FROM postgis/postgis:${POSTGIS_TAG}

ARG PG_MAJOR=18

COPY --from=builder \
  /build/target/release/fuzzyregion-pg${PG_MAJOR}/usr/ /usr/

# Demo data and init script (runs on first start, after PostGIS init).
COPY examples/demo-data/po_valley_tree_cover_transition/*.geojson /demo-data/
COPY initdb/10_fuzzyregion_demo.sql /docker-entrypoint-initdb.d/

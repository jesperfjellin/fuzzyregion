#![deny(unsafe_op_in_unsafe_fn)]
#![doc = r#"
PostgreSQL-facing integration crate for `fuzzyregion`.

This crate is intentionally thin. It exists to:

- expose SQL functions and types
- translate between PostgreSQL values and the core domain model
- implement the PostGIS-backed geometry engine

It should not own fuzzyregion semantics. Those live in `fuzzyregion-core`.
"#]

::pgrx::pg_module_magic!(name, version);

/// SQL entrypoints and user-facing PostgreSQL functions.
pub mod api;

/// PostGIS-backed geometry adapters and codecs.
pub mod interop;

/// PostgreSQL base-type wrapper for `fuzzyregion`.
pub mod sql_type;

/// On-disk and in-memory PostgreSQL storage representations.
pub mod storage;

pub use interop::{
    PostgisEngine, PostgisError, PostgisGeometry, PostgisPoint, decode_stored_fuzzyregion,
    encode_domain_fuzzyregion,
};
pub use sql_type::Fuzzyregion;

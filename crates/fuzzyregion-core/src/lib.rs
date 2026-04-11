#![forbid(unsafe_code)]
#![doc = r#"
Database-agnostic domain types and algorithms for `fuzzyregion`.

This crate owns the semantic rules for fuzzy regions:

- validated alpha values
- canonical level ordering
- nesting constraints
- crisp projections such as support and alpha-cuts
- closure-preserving set operations orchestrated through a geometry engine

The core crate deliberately knows nothing about PostgreSQL, PostGIS, or `pgrx`.
All crisp geometry behavior is delegated to [`GeometryEngine`].
"#]

pub mod alpha;
pub mod engine;
pub mod error;
pub mod fuzzyregion;
pub mod level;

pub use alpha::{Alpha, AlphaError, AlphaThreshold};
pub use engine::GeometryEngine;
pub use error::{CoreError, CoreResult, MembershipTransformError};
pub use fuzzyregion::{Fuzzyregion, UncheckedFuzzyregion};
pub use level::Level;

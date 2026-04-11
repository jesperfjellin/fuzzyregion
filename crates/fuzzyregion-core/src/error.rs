//! Error types returned by the fuzzyregion core domain model.

use std::error::Error;
use std::fmt;

use crate::alpha::{Alpha, AlphaError};

/// The standard result type used by the core crate.
pub type CoreResult<T, E> = Result<T, CoreError<E>>;

/// Errors returned by membership-only alpha remapping operations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MembershipTransformError {
    /// The supplied power was not finite.
    NotFinitePower {
        /// The original value supplied by the caller.
        value: f64,
    },
    /// The supplied power must be greater than `1.0`.
    PowerMustBeGreaterThanOne {
        /// The original value supplied by the caller.
        value: f64,
    },
}

impl fmt::Display for MembershipTransformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFinitePower { value } => {
                write!(f, "membership transform power {value} must be finite")
            }
            Self::PowerMustBeGreaterThanOne { value } => {
                write!(
                    f,
                    "membership transform power {value} must be greater than 1.0"
                )
            }
        }
    }
}

impl Error for MembershipTransformError {}

/// Errors returned by fuzzyregion domain operations.
#[derive(Debug, PartialEq)]
pub enum CoreError<E> {
    /// An alpha value was invalid.
    InvalidAlpha(AlphaError),
    /// Two operands with incompatible SRIDs were combined.
    MixedOperandSrid {
        /// The left operand SRID.
        left: i32,
        /// The right operand SRID.
        right: i32,
    },
    /// A level list contained the same alpha more than once.
    DuplicateAlpha {
        /// The duplicated alpha value.
        alpha: Alpha,
    },
    /// A normalized geometry turned out to be empty.
    EmptyGeometry {
        /// The level that produced the empty geometry.
        alpha: Alpha,
    },
    /// Level geometries did not all share the same SRID.
    MixedSrid {
        /// The SRID established by the first level.
        expected: i32,
        /// The SRID found on the violating level.
        found: i32,
        /// The alpha attached to the violating level.
        alpha: Alpha,
    },
    /// A higher-alpha level was not contained in the next lower-alpha level.
    NonNestedLevels {
        /// The higher membership level.
        higher: Alpha,
        /// The lower membership level that should have contained `higher`.
        lower: Alpha,
    },
    /// The geometry backend returned an error.
    Geometry(E),
}

impl<E> From<AlphaError> for CoreError<E> {
    fn from(value: AlphaError) -> Self {
        Self::InvalidAlpha(value)
    }
}

impl<E: fmt::Display> fmt::Display for CoreError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAlpha(error) => error.fmt(f),
            Self::MixedOperandSrid { left, right } => {
                write!(
                    f,
                    "cannot combine fuzzyregions with SRIDs {left} and {right}"
                )
            }
            Self::DuplicateAlpha { alpha } => {
                write!(f, "alpha {} appears more than once", alpha.value())
            }
            Self::EmptyGeometry { alpha } => {
                write!(f, "geometry at alpha {} is empty", alpha.value())
            }
            Self::MixedSrid {
                expected,
                found,
                alpha,
            } => write!(
                f,
                "geometry at alpha {} has SRID {found}, expected {expected}",
                alpha.value()
            ),
            Self::NonNestedLevels { higher, lower } => write!(
                f,
                "geometry at alpha {} is not contained in geometry at alpha {}",
                higher.value(),
                lower.value()
            ),
            Self::Geometry(error) => error.fmt(f),
        }
    }
}

impl<E: Error + 'static> Error for CoreError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidAlpha(error) => Some(error),
            Self::Geometry(error) => Some(error),
            Self::MixedOperandSrid { .. }
            | Self::DuplicateAlpha { .. }
            | Self::EmptyGeometry { .. }
            | Self::MixedSrid { .. }
            | Self::NonNestedLevels { .. } => None,
        }
    }
}

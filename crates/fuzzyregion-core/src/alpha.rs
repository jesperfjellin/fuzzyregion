//! Validated alpha values used by fuzzyregion storage and query APIs.

use std::cmp::Ordering;
use std::error::Error;
use std::fmt;

/// Errors returned when constructing validated alpha values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AlphaError {
    /// The supplied value was not finite.
    NotFinite {
        /// The original value supplied by the caller.
        value: f64,
    },
    /// A stored level alpha was outside `(0, 1]`.
    LevelOutOfRange {
        /// The original value supplied by the caller.
        value: f64,
    },
    /// A query threshold was outside `[0, 1]`.
    ThresholdOutOfRange {
        /// The original value supplied by the caller.
        value: f64,
    },
}

impl fmt::Display for AlphaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFinite { value } => write!(f, "alpha value {value} must be finite"),
            Self::LevelOutOfRange { value } => {
                write!(f, "level alpha {value} must be in the range (0, 1]")
            }
            Self::ThresholdOutOfRange { value } => {
                write!(f, "alpha threshold {value} must be in the range [0, 1]")
            }
        }
    }
}

impl Error for AlphaError {}

/// A validated membership level used by stored fuzzyregion levels.
///
/// Stored alphas are restricted to the interval `(0, 1]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Alpha(f64);

impl Alpha {
    /// Returns the raw floating-point value.
    pub fn value(self) -> f64 {
        self.0
    }

    /// Returns `true` when the alpha is exactly `1.0`.
    pub fn is_one(self) -> bool {
        self.0 == 1.0
    }
}

impl Eq for Alpha {}

impl Ord for Alpha {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl PartialOrd for Alpha {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl TryFrom<f64> for Alpha {
    type Error = AlphaError;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        if !value.is_finite() {
            return Err(AlphaError::NotFinite { value });
        }

        if value <= 0.0 || value > 1.0 {
            return Err(AlphaError::LevelOutOfRange { value });
        }

        Ok(Self(value))
    }
}

impl From<Alpha> for f64 {
    fn from(value: Alpha) -> Self {
        value.0
    }
}

/// A validated threshold for crisp projections such as alpha-cuts.
///
/// Thresholds are allowed to be exactly `0.0` so callers can express support-like
/// queries without inventing a synthetic stored level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlphaThreshold(f64);

impl AlphaThreshold {
    /// Returns the raw floating-point value.
    pub fn value(self) -> f64 {
        self.0
    }

    /// Returns `true` when the threshold is exactly `0.0`.
    pub fn is_zero(self) -> bool {
        self.0 == 0.0
    }
}

impl Eq for AlphaThreshold {}

impl Ord for AlphaThreshold {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl PartialOrd for AlphaThreshold {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl TryFrom<f64> for AlphaThreshold {
    type Error = AlphaError;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        if !value.is_finite() {
            return Err(AlphaError::NotFinite { value });
        }

        if !(0.0..=1.0).contains(&value) {
            return Err(AlphaError::ThresholdOutOfRange { value });
        }

        Ok(Self(value))
    }
}

impl From<Alpha> for AlphaThreshold {
    fn from(value: Alpha) -> Self {
        Self(value.value())
    }
}

impl From<AlphaThreshold> for f64 {
    fn from(value: AlphaThreshold) -> Self {
        value.0
    }
}

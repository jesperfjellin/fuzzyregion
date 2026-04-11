//! Stored fuzzyregion level values.

use crate::Alpha;

/// A single stored membership level in a fuzzyregion.
///
/// Each level couples a validated alpha with a crisp polygonal geometry
/// representation owned by the configured geometry backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Level<G> {
    alpha: Alpha,
    geometry: G,
}

impl<G> Level<G> {
    /// Creates a new level from a validated alpha and a backend geometry value.
    pub fn new(alpha: Alpha, geometry: G) -> Self {
        Self { alpha, geometry }
    }

    /// Returns the level alpha.
    pub fn alpha(&self) -> Alpha {
        self.alpha
    }

    /// Returns a shared reference to the level geometry.
    pub fn geometry(&self) -> &G {
        &self.geometry
    }

    /// Consumes the level and returns its geometry.
    pub fn into_geometry(self) -> G {
        self.geometry
    }
}

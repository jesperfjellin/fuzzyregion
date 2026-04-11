//! Geometry backend abstractions used by the fuzzyregion domain model.

/// A crisp geometry backend used by the domain model.
///
/// The core crate orchestrates fuzzyregion semantics, but it never performs
/// geometry work directly. Implementors of this trait provide the crisp geometry
/// operations needed to normalize, validate, project, and combine level
/// geometries.
pub trait GeometryEngine {
    /// The concrete polygonal geometry representation used by the backend.
    type Geometry;

    /// The concrete point representation used by membership lookups.
    type Point;

    /// The backend-specific error type.
    type Error;

    /// Normalizes an input geometry into the engine's canonical multipolygon form.
    fn normalize_multipolygon(
        &self,
        geometry: Self::Geometry,
    ) -> Result<Self::Geometry, Self::Error>;

    /// Returns `true` when the geometry is empty.
    fn is_empty(&self, geometry: &Self::Geometry) -> Result<bool, Self::Error>;

    /// Returns the geometry SRID.
    fn srid(&self, geometry: &Self::Geometry) -> Result<i32, Self::Error>;

    /// Returns `true` when both geometries are topologically equivalent.
    fn topologically_equals(
        &self,
        left: &Self::Geometry,
        right: &Self::Geometry,
    ) -> Result<bool, Self::Error>;

    /// Returns `true` when `container` covers `containee`, including boundary-only contact.
    ///
    /// The fuzzyregion domain model uses boundary-inclusive containment semantics
    /// for nested levels. PostgreSQL/PostGIS backends should therefore map this
    /// to coverage semantics rather than strict interior-only containment.
    fn contains(
        &self,
        container: &Self::Geometry,
        containee: &Self::Geometry,
    ) -> Result<bool, Self::Error>;

    /// Returns `true` when the geometry covers the supplied point, including boundary points.
    fn contains_point(
        &self,
        geometry: &Self::Geometry,
        point: &Self::Point,
    ) -> Result<bool, Self::Error>;

    /// Computes the crisp union of two geometries.
    fn union(
        &self,
        left: &Self::Geometry,
        right: &Self::Geometry,
    ) -> Result<Self::Geometry, Self::Error>;

    /// Computes the crisp intersection of two geometries.
    fn intersection(
        &self,
        left: &Self::Geometry,
        right: &Self::Geometry,
    ) -> Result<Self::Geometry, Self::Error>;

    /// Computes the crisp difference `left - right`.
    fn difference(
        &self,
        left: &Self::Geometry,
        right: &Self::Geometry,
    ) -> Result<Self::Geometry, Self::Error>;

    /// Computes a backend-specific area measure for the geometry.
    fn area(&self, geometry: &Self::Geometry) -> Result<f64, Self::Error>;

    /// Returns the geometry bounding box as a polygonal geometry.
    fn bounding_box(&self, geometry: &Self::Geometry) -> Result<Self::Geometry, Self::Error>;
}

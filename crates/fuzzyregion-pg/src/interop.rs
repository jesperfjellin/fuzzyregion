//! PostGIS-backed geometry interop for the `fuzzyregion` extension.
//!
//! This module owns the bridge between:
//!
//! - the core domain model in `fuzzyregion-core`
//! - the persisted binary payload in [`crate::storage`]
//! - the PostgreSQL/PostGIS runtime exposed through `pgrx`
//!
//! Geometry values are represented as EWKB byte payloads and all crisp geometry
//! operations are delegated to PostGIS through SPI.

use std::error::Error;
use std::fmt;

use fuzzyregion_core::{
    CoreError, CoreResult, Fuzzyregion, GeometryEngine, Level, UncheckedFuzzyregion,
};
use pgrx::datum::DatumWithOid;
use pgrx::spi::{Spi, SpiError};

use crate::storage::{StorageError, StoredFuzzyregion, StoredLevel};

/// A PostGIS geometry value stored as EWKB.
///
/// This is the canonical geometry representation used by the PostgreSQL
/// extension crate. The binary payload is stable enough to persist in storage
/// and portable enough to hand to PostGIS for further processing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostgisGeometry {
    ewkb: Vec<u8>,
}

impl PostgisGeometry {
    /// Creates a geometry wrapper from EWKB bytes.
    pub fn from_ewkb(ewkb: Vec<u8>) -> Result<Self, PostgisError> {
        if ewkb.is_empty() {
            return Err(PostgisError::EmptyGeometryBytes);
        }

        Ok(Self { ewkb })
    }

    /// Returns the underlying EWKB bytes.
    pub fn ewkb(&self) -> &[u8] {
        &self.ewkb
    }

    /// Consumes the wrapper and returns the EWKB bytes.
    pub fn into_ewkb(self) -> Vec<u8> {
        self.ewkb
    }
}

/// A PostGIS point value stored as EWKB.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostgisPoint {
    ewkb: Vec<u8>,
}

impl PostgisPoint {
    /// Creates a point wrapper from EWKB bytes.
    pub fn from_ewkb(ewkb: Vec<u8>) -> Result<Self, PostgisError> {
        if ewkb.is_empty() {
            return Err(PostgisError::EmptyPointBytes);
        }

        Ok(Self { ewkb })
    }

    /// Returns the underlying EWKB bytes.
    pub fn ewkb(&self) -> &[u8] {
        &self.ewkb
    }
}

/// Errors returned by the PostGIS integration layer.
#[derive(Debug)]
pub enum PostgisError {
    /// A geometry wrapper was constructed from empty bytes.
    EmptyGeometryBytes,
    /// A point wrapper was constructed from empty bytes.
    EmptyPointBytes,
    /// A point argument was not a non-empty POINT geometry.
    InvalidPointGeometry,
    /// A stored payload violated invariants that should already have been canonicalized.
    CorruptStoredValue(String),
    /// PostGIS unexpectedly returned `NULL`.
    NullResult(&'static str),
    /// A stored payload could not be converted into runtime values.
    Storage(StorageError),
    /// PostgreSQL SPI returned an error.
    Spi(SpiError),
}

impl From<SpiError> for PostgisError {
    fn from(value: SpiError) -> Self {
        Self::Spi(value)
    }
}

impl From<StorageError> for PostgisError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl fmt::Display for PostgisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyGeometryBytes => write!(f, "geometry EWKB bytes must not be empty"),
            Self::EmptyPointBytes => write!(f, "point EWKB bytes must not be empty"),
            Self::InvalidPointGeometry => {
                write!(f, "point input must be a non-empty POINT geometry")
            }
            Self::CorruptStoredValue(message) => write!(f, "{message}"),
            Self::NullResult(context) => write!(f, "PostGIS returned NULL while {context}"),
            Self::Storage(error) => error.fmt(f),
            Self::Spi(error) => error.fmt(f),
        }
    }
}

impl Error for PostgisError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::Spi(error) => Some(error),
            Self::EmptyGeometryBytes
            | Self::EmptyPointBytes
            | Self::InvalidPointGeometry
            | Self::CorruptStoredValue(_)
            | Self::NullResult(_) => None,
        }
    }
}

/// The production geometry engine backed by PostGIS.
#[derive(Clone, Copy, Debug, Default)]
pub struct PostgisEngine;

impl GeometryEngine for PostgisEngine {
    type Geometry = PostgisGeometry;
    type Point = PostgisPoint;
    type Error = PostgisError;

    fn normalize_multipolygon(
        &self,
        geometry: Self::Geometry,
    ) -> Result<Self::Geometry, Self::Error> {
        self.query_geometry(
            "normalizing a geometry to canonical MULTIPOLYGON form",
            "SELECT ST_AsEWKB(ST_Multi(ST_CollectionExtract(ST_GeomFromEWKB($1), 3)))",
            &[DatumWithOid::from(geometry.into_ewkb())],
        )
    }

    fn is_empty(&self, geometry: &Self::Geometry) -> Result<bool, Self::Error> {
        self.query_bool(
            "checking whether a geometry is empty",
            "SELECT ST_IsEmpty(ST_GeomFromEWKB($1))",
            &[DatumWithOid::from(geometry.ewkb().to_vec())],
        )
    }

    fn srid(&self, geometry: &Self::Geometry) -> Result<i32, Self::Error> {
        self.query_i32(
            "reading a geometry SRID",
            "SELECT ST_SRID(ST_GeomFromEWKB($1))",
            &[DatumWithOid::from(geometry.ewkb().to_vec())],
        )
    }

    fn topologically_equals(
        &self,
        left: &Self::Geometry,
        right: &Self::Geometry,
    ) -> Result<bool, Self::Error> {
        self.query_bool(
            "checking topological equality",
            "SELECT ST_Equals(ST_GeomFromEWKB($1), ST_GeomFromEWKB($2))",
            &[
                DatumWithOid::from(left.ewkb().to_vec()),
                DatumWithOid::from(right.ewkb().to_vec()),
            ],
        )
    }

    fn contains(
        &self,
        container: &Self::Geometry,
        containee: &Self::Geometry,
    ) -> Result<bool, Self::Error> {
        self.query_bool(
            "checking geometric containment",
            "SELECT ST_Covers(ST_GeomFromEWKB($1), ST_GeomFromEWKB($2))",
            &[
                DatumWithOid::from(container.ewkb().to_vec()),
                DatumWithOid::from(containee.ewkb().to_vec()),
            ],
        )
    }

    fn contains_point(
        &self,
        geometry: &Self::Geometry,
        point: &Self::Point,
    ) -> Result<bool, Self::Error> {
        self.query_bool(
            "checking point membership in a geometry",
            "SELECT ST_Covers(ST_GeomFromEWKB($1), ST_GeomFromEWKB($2))",
            &[
                DatumWithOid::from(geometry.ewkb().to_vec()),
                DatumWithOid::from(point.ewkb().to_vec()),
            ],
        )
    }

    fn union(
        &self,
        left: &Self::Geometry,
        right: &Self::Geometry,
    ) -> Result<Self::Geometry, Self::Error> {
        self.query_geometry(
            "computing a PostGIS union",
            "SELECT ST_AsEWKB(ST_Union(ST_GeomFromEWKB($1), ST_GeomFromEWKB($2)))",
            &[
                DatumWithOid::from(left.ewkb().to_vec()),
                DatumWithOid::from(right.ewkb().to_vec()),
            ],
        )
    }

    fn intersection(
        &self,
        left: &Self::Geometry,
        right: &Self::Geometry,
    ) -> Result<Self::Geometry, Self::Error> {
        self.query_geometry(
            "computing a PostGIS intersection",
            "SELECT ST_AsEWKB(ST_Intersection(ST_GeomFromEWKB($1), ST_GeomFromEWKB($2)))",
            &[
                DatumWithOid::from(left.ewkb().to_vec()),
                DatumWithOid::from(right.ewkb().to_vec()),
            ],
        )
    }

    fn difference(
        &self,
        left: &Self::Geometry,
        right: &Self::Geometry,
    ) -> Result<Self::Geometry, Self::Error> {
        self.query_geometry(
            "computing a PostGIS difference",
            "SELECT ST_AsEWKB(ST_Difference(ST_GeomFromEWKB($1), ST_GeomFromEWKB($2)))",
            &[
                DatumWithOid::from(left.ewkb().to_vec()),
                DatumWithOid::from(right.ewkb().to_vec()),
            ],
        )
    }

    fn area(&self, geometry: &Self::Geometry) -> Result<f64, Self::Error> {
        self.query_f64(
            "computing geometry area",
            "SELECT ST_Area(ST_GeomFromEWKB($1))",
            &[DatumWithOid::from(geometry.ewkb().to_vec())],
        )
    }

    fn bounding_box(&self, geometry: &Self::Geometry) -> Result<Self::Geometry, Self::Error> {
        self.query_geometry(
            "computing a geometry bounding box",
            "SELECT ST_AsEWKB(ST_Envelope(ST_GeomFromEWKB($1)))",
            &[DatumWithOid::from(geometry.ewkb().to_vec())],
        )
    }
}

impl PostgisEngine {
    /// Returns an empty canonical `MULTIPOLYGON`, optionally carrying the supplied SRID.
    pub fn empty_multipolygon(&self, srid: Option<i32>) -> Result<PostgisGeometry, PostgisError> {
        match srid {
            Some(srid) => self.query_geometry(
                "constructing an empty multipolygon with SRID",
                "SELECT ST_AsEWKB(ST_SetSRID(ST_GeomFromText('MULTIPOLYGON EMPTY'), $1))",
                &[DatumWithOid::from(srid)],
            ),
            None => self.query_geometry(
                "constructing an empty multipolygon without SRID",
                "SELECT ST_AsEWKB(ST_GeomFromText('MULTIPOLYGON EMPTY'))",
                &[],
            ),
        }
    }

    /// Validates and wraps a SQL geometry value as a non-empty PostGIS point.
    pub fn point_from_ewkb(&self, ewkb: Vec<u8>) -> Result<PostgisPoint, PostgisError> {
        let point = PostgisPoint::from_ewkb(ewkb)?;
        let is_valid = self.query_bool(
            "validating a point geometry argument",
            "SELECT ST_GeometryType(ST_GeomFromEWKB($1)) = 'ST_Point' AND NOT ST_IsEmpty(ST_GeomFromEWKB($1))",
            &[DatumWithOid::from(point.ewkb().to_vec())],
        )?;

        if !is_valid {
            return Err(PostgisError::InvalidPointGeometry);
        }

        Ok(point)
    }

    /// Reads the SRID of a validated point value.
    pub fn point_srid(&self, point: &PostgisPoint) -> Result<i32, PostgisError> {
        self.query_i32(
            "reading a point SRID",
            "SELECT ST_SRID(ST_GeomFromEWKB($1))",
            &[DatumWithOid::from(point.ewkb().to_vec())],
        )
    }

    /// Formats a geometry as EWKT for human-readable exports.
    pub fn geometry_to_ewkt(&self, geometry: &PostgisGeometry) -> Result<String, PostgisError> {
        self.query_string(
            "formatting a geometry as EWKT",
            "SELECT ST_AsEWKT(ST_GeomFromEWKB($1))",
            &[DatumWithOid::from(geometry.ewkb().to_vec())],
        )
    }

    fn query_geometry(
        &self,
        context: &'static str,
        sql: &str,
        args: &[DatumWithOid<'_>],
    ) -> Result<PostgisGeometry, PostgisError> {
        let ewkb = Spi::get_one_with_args::<Vec<u8>>(sql, args)?
            .ok_or(PostgisError::NullResult(context))?;
        PostgisGeometry::from_ewkb(ewkb)
    }

    fn query_bool(
        &self,
        context: &'static str,
        sql: &str,
        args: &[DatumWithOid<'_>],
    ) -> Result<bool, PostgisError> {
        Spi::get_one_with_args::<bool>(sql, args)?.ok_or(PostgisError::NullResult(context))
    }

    fn query_i32(
        &self,
        context: &'static str,
        sql: &str,
        args: &[DatumWithOid<'_>],
    ) -> Result<i32, PostgisError> {
        Spi::get_one_with_args::<i32>(sql, args)?.ok_or(PostgisError::NullResult(context))
    }

    fn query_f64(
        &self,
        context: &'static str,
        sql: &str,
        args: &[DatumWithOid<'_>],
    ) -> Result<f64, PostgisError> {
        Spi::get_one_with_args::<f64>(sql, args)?.ok_or(PostgisError::NullResult(context))
    }

    fn query_string(
        &self,
        context: &'static str,
        sql: &str,
        args: &[DatumWithOid<'_>],
    ) -> Result<String, PostgisError> {
        Spi::get_one_with_args::<String>(sql, args)?.ok_or(PostgisError::NullResult(context))
    }
}

/// Decodes a stored payload into the canonical domain representation.
pub fn decode_stored_fuzzyregion(
    stored: StoredFuzzyregion,
    engine: &PostgisEngine,
) -> CoreResult<Fuzzyregion<PostgisGeometry>, PostgisError> {
    let levels = stored
        .levels()
        .iter()
        .cloned()
        .map(|level| {
            Ok(Level::new(
                level.alpha(),
                PostgisGeometry::from_ewkb(level.into_geometry_ewkb())
                    .map_err(CoreError::Geometry)?,
            ))
        })
        .collect::<CoreResult<Vec<_>, PostgisError>>()?;

    Fuzzyregion::canonicalize(UncheckedFuzzyregion::new(levels), engine)
}

/// Encodes a canonical domain value into the persisted payload representation.
pub fn encode_domain_fuzzyregion(
    value: &Fuzzyregion<PostgisGeometry>,
) -> Result<StoredFuzzyregion, PostgisError> {
    let levels = value
        .levels()
        .iter()
        .map(|level| {
            StoredLevel::new(level.alpha(), level.geometry().ewkb().to_vec())
                .map_err(PostgisError::Storage)
        })
        .collect::<Result<Vec<_>, _>>()?;

    StoredFuzzyregion::new(value.srid(), levels).map_err(PostgisError::Storage)
}

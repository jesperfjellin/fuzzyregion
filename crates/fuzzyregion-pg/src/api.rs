//! SQL-facing functions for the `fuzzyregion` extension.
//!
//! This module keeps the SQL surface narrow and intentional:
//!
//! - Rust owns the base type and the EWKB-oriented low-level functions
//! - SQL wrappers expose PostGIS `geometry` ergonomics on top of those low-level functions

use std::error::Error;
use std::fmt;

use fuzzyregion_core::{
    Alpha, AlphaError, AlphaThreshold, Fuzzyregion as DomainFuzzyregion, GeometryEngine, Level,
    MembershipTransformError,
};
use pgrx::JsonB;
use pgrx::prelude::*;
use serde_json::{Value, json};

use crate::interop::{PostgisEngine, PostgisError, PostgisGeometry};
use crate::sql_type::{Fuzzyregion, encode_hex};

/// Creates a `fuzzyregion` from aligned alpha and EWKB geometry arrays.
#[pg_extern(immutable, parallel_safe)]
fn fuzzyregion_from_ewkb(
    alphas: Array<'_, f64>,
    geoms: Array<'_, &[u8]>,
) -> Result<Fuzzyregion, ApiError> {
    let alpha_count = alphas.len();
    let geometry_count = geoms.len();

    if alpha_count != geometry_count {
        return Err(ApiError::MismatchedLevelCount {
            alphas: alpha_count,
            geometries: geometry_count,
        });
    }

    let levels = alphas
        .iter()
        .zip(geoms.iter())
        .enumerate()
        .map(|(index, (alpha, ewkb))| {
            let alpha = alpha.ok_or(ApiError::NullAlphaElement { index })?;
            let alpha = Alpha::try_from(alpha).map_err(ApiError::InvalidAlpha)?;
            let ewkb = ewkb.ok_or(ApiError::NullGeometryElement { index })?;
            let geometry = PostgisGeometry::from_ewkb(ewkb.to_vec()).map_err(ApiError::Postgis)?;
            Ok(Level::new(alpha, geometry))
        })
        .collect::<Result<Vec<_>, ApiError>>()?;

    let engine = PostgisEngine;
    let domain = DomainFuzzyregion::from_levels(levels, &engine).map_err(ApiError::Core)?;
    Fuzzyregion::from_domain(&domain).map_err(ApiError::Postgis)
}

/// Returns the shared SRID stored on a `fuzzyregion`, if any.
#[pg_extern(immutable, parallel_safe)]
fn fuzzyregion_srid(fr: Fuzzyregion) -> Option<i32> {
    fr.srid()
}

/// Returns `true` when the supplied value satisfies all `fuzzyregion` invariants.
#[pg_extern(immutable, strict, parallel_safe)]
fn fuzzyregion_is_valid(fr: Fuzzyregion) -> bool {
    validation_errors(&fr).is_empty()
}

/// Returns validation errors for a `fuzzyregion` value.
#[pg_extern(immutable, strict, parallel_safe)]
fn fuzzyregion_validate(fr: Fuzzyregion) -> Vec<String> {
    validation_errors(&fr)
}

/// Returns the stored levels and metadata for inspection.
#[pg_extern(immutable, strict, parallel_safe)]
fn fuzzyregion_levels(fr: Fuzzyregion) -> JsonB {
    JsonB(levels_json(&fr))
}

/// Returns the smallest stored alpha, or `NULL` for the empty value.
#[pg_extern(immutable, strict, parallel_safe)]
fn fuzzyregion_min_alpha(fr: Fuzzyregion) -> Option<f64> {
    fr.stored()
        .levels()
        .last()
        .map(|level| level.alpha().value())
}

/// Returns the highest stored alpha, or `NULL` for the empty value.
#[pg_extern(immutable, strict, parallel_safe)]
fn fuzzyregion_max_alpha(fr: Fuzzyregion) -> Option<f64> {
    fr.stored()
        .levels()
        .first()
        .map(|level| level.alpha().value())
}

/// Returns the number of stored levels.
#[pg_extern(immutable, strict, parallel_safe)]
fn fuzzyregion_num_levels(fr: Fuzzyregion) -> i32 {
    i32::try_from(fr.stored().levels().len())
        .expect("fuzzyregion level count exceeds PostgreSQL integer range")
}

/// Returns the area of the alpha-cut geometry.
#[pg_extern(immutable, strict, parallel_safe)]
fn fuzzyregion_area_at(fr: Fuzzyregion, alpha: f64) -> Result<f64, ApiError> {
    let threshold = AlphaThreshold::try_from(alpha).map_err(ApiError::InvalidThreshold)?;
    let engine = PostgisEngine;
    let domain = fr.to_domain(&engine).map_err(ApiError::Postgis)?;

    match domain.alpha_cut(threshold) {
        Some(geometry) => engine.area(geometry).map_err(ApiError::Postgis),
        None => Ok(0.0),
    }
}

/// Returns a human-readable JSON export of the value.
#[pg_extern(immutable, strict, parallel_safe)]
fn fuzzyregion_to_jsonb(fr: Fuzzyregion) -> Result<JsonB, ApiError> {
    Ok(JsonB(human_readable_json(&fr)?))
}

/// Returns the low-level debug text representation of the value.
#[pg_extern(immutable, strict, parallel_safe)]
fn fuzzyregion_to_text(fr: Fuzzyregion) -> String {
    fr.to_text_representation()
}

/// Rescales memberships so the highest stored alpha becomes `1.0`.
#[pg_extern(immutable, strict, parallel_safe)]
fn fuzzyregion_normalize(fr: Fuzzyregion) -> Result<Fuzzyregion, ApiError> {
    let engine = PostgisEngine;
    let domain = fr.to_domain(&engine).map_err(ApiError::Postgis)?;
    let normalized = domain.normalize_membership();

    Fuzzyregion::from_domain(&normalized).map_err(ApiError::Postgis)
}

/// Sharpens memberships by applying `alpha' = alpha^power`.
#[pg_extern(immutable, strict, parallel_safe)]
fn fuzzyregion_concentrate(fr: Fuzzyregion, power: f64) -> Result<Fuzzyregion, ApiError> {
    unary_membership_transform(fr, power, |region, power| {
        region.concentrate_membership(power)
    })
}

/// Softens memberships by applying `alpha' = alpha^(1/power)`.
#[pg_extern(immutable, strict, parallel_safe)]
fn fuzzyregion_dilate_membership(fr: Fuzzyregion, power: f64) -> Result<Fuzzyregion, ApiError> {
    unary_membership_transform(fr, power, |region, power| region.dilate_membership(power))
}

/// Returns the support geometry as EWKB.
#[pg_extern(immutable, parallel_safe)]
fn fuzzyregion_support_ewkb(fr: Fuzzyregion) -> Result<Vec<u8>, ApiError> {
    projection_or_empty(fr.support_ewkb(), fr.srid())
}

/// Returns the core geometry as EWKB.
#[pg_extern(immutable, parallel_safe)]
fn fuzzyregion_core_ewkb(fr: Fuzzyregion) -> Result<Vec<u8>, ApiError> {
    projection_or_empty(fr.core_ewkb(), fr.srid())
}

/// Returns the alpha-cut geometry as EWKB.
#[pg_extern(immutable, parallel_safe)]
fn fuzzyregion_alpha_cut_ewkb(fr: Fuzzyregion, alpha: f64) -> Result<Vec<u8>, ApiError> {
    let threshold = AlphaThreshold::try_from(alpha).map_err(ApiError::InvalidThreshold)?;
    projection_or_empty(fr.alpha_cut_ewkb(threshold), fr.srid())
}

/// Returns the membership value at a supplied point geometry.
#[pg_extern(immutable, parallel_safe)]
fn fuzzyregion_membership_at_ewkb(fr: Fuzzyregion, point: &[u8]) -> Result<f64, ApiError> {
    let engine = PostgisEngine;
    let point = engine
        .point_from_ewkb(point.to_vec())
        .map_err(ApiError::Postgis)?;
    let point_srid = engine.point_srid(&point).map_err(ApiError::Postgis)?;

    if let Some(region_srid) = fr.srid() {
        if region_srid != point_srid {
            return Err(ApiError::PointSridMismatch {
                region_srid,
                point_srid,
            });
        }
    }

    let domain = fr.to_domain(&engine).map_err(ApiError::Postgis)?;
    let membership = domain
        .membership_at(&engine, &point)
        .map_err(ApiError::Core)?
        .map_or(0.0, |alpha| alpha.value());

    Ok(membership)
}

/// Returns the bounding box of the support geometry as EWKB.
#[pg_extern(immutable, parallel_safe)]
fn fuzzyregion_bbox_ewkb(fr: Fuzzyregion) -> Result<Vec<u8>, ApiError> {
    let engine = PostgisEngine;
    let domain = fr.to_domain(&engine).map_err(ApiError::Postgis)?;
    let bbox = domain.bbox(&engine).map_err(ApiError::Core)?;

    projection_or_empty(bbox.as_ref().map(PostgisGeometry::ewkb), fr.srid())
}

/// Computes the standard fuzzy union of two `fuzzyregion` values.
#[pg_extern(immutable, strict, parallel_safe)]
fn fuzzyregion_union(a: Fuzzyregion, b: Fuzzyregion) -> Result<Fuzzyregion, ApiError> {
    binary_set_operation(a, b, |left, right, engine| left.union(right, engine))
}

/// Computes the standard fuzzy intersection of two `fuzzyregion` values.
#[pg_extern(immutable, strict, parallel_safe)]
fn fuzzyregion_intersection(a: Fuzzyregion, b: Fuzzyregion) -> Result<Fuzzyregion, ApiError> {
    binary_set_operation(a, b, |left, right, engine| left.intersection(right, engine))
}

/// Computes the standard fuzzy difference `a - b`.
#[pg_extern(immutable, strict, parallel_safe)]
fn fuzzyregion_difference(a: Fuzzyregion, b: Fuzzyregion) -> Result<Fuzzyregion, ApiError> {
    binary_set_operation(a, b, |left, right, engine| left.difference(right, engine))
}

fn projection_or_empty(ewkb: Option<&[u8]>, srid: Option<i32>) -> Result<Vec<u8>, ApiError> {
    match ewkb {
        Some(ewkb) => Ok(ewkb.to_vec()),
        None => Ok(PostgisEngine
            .empty_multipolygon(srid)
            .map_err(ApiError::Postgis)?
            .into_ewkb()),
    }
}

fn validation_errors(fr: &Fuzzyregion) -> Vec<String> {
    let engine = PostgisEngine;

    match fr.to_domain(&engine) {
        Ok(_) => Vec::new(),
        Err(error) => vec![error.to_string()],
    }
}

fn levels_json(fr: &Fuzzyregion) -> Value {
    let stored = fr.stored();
    let levels = stored
        .levels()
        .iter()
        .map(|level| {
            json!({
                "alpha": level.alpha().value(),
                "geometry_ewkb": encode_hex(level.geometry_ewkb()),
            })
        })
        .collect::<Vec<_>>();

    json!({
        "version": stored.version().as_u8(),
        "srid": stored.srid(),
        "is_empty": stored.is_empty(),
        "num_levels": stored.levels().len(),
        "levels": levels,
    })
}

fn human_readable_json(fr: &Fuzzyregion) -> Result<Value, ApiError> {
    let engine = PostgisEngine;
    let domain = fr.to_domain(&engine).map_err(ApiError::Postgis)?;
    let levels = domain
        .levels()
        .iter()
        .map(|level| {
            Ok(json!({
                "alpha": level.alpha().value(),
                "geometry_ewkt": engine
                    .geometry_to_ewkt(level.geometry())
                    .map_err(ApiError::Postgis)?,
            }))
        })
        .collect::<Result<Vec<_>, ApiError>>()?;

    Ok(json!({
        "version": fr.stored().version().as_u8(),
        "srid": domain.srid(),
        "is_empty": domain.is_empty(),
        "num_levels": domain.num_levels(),
        "min_alpha": domain.min_alpha().map(|alpha| alpha.value()),
        "max_alpha": domain.max_alpha().map(|alpha| alpha.value()),
        "levels": levels,
    }))
}

fn binary_set_operation<F>(a: Fuzzyregion, b: Fuzzyregion, op: F) -> Result<Fuzzyregion, ApiError>
where
    F: FnOnce(
        &DomainFuzzyregion<PostgisGeometry>,
        &DomainFuzzyregion<PostgisGeometry>,
        &PostgisEngine,
    )
        -> fuzzyregion_core::CoreResult<DomainFuzzyregion<PostgisGeometry>, PostgisError>,
{
    let engine = PostgisEngine;
    let left = a.to_domain(&engine).map_err(ApiError::Postgis)?;
    let right = b.to_domain(&engine).map_err(ApiError::Postgis)?;
    let result = op(&left, &right, &engine).map_err(ApiError::Core)?;

    Fuzzyregion::from_domain(&result).map_err(ApiError::Postgis)
}

fn unary_membership_transform<F>(
    fr: Fuzzyregion,
    power: f64,
    transform: F,
) -> Result<Fuzzyregion, ApiError>
where
    F: FnOnce(
        &DomainFuzzyregion<PostgisGeometry>,
        f64,
    ) -> Result<DomainFuzzyregion<PostgisGeometry>, MembershipTransformError>,
{
    let engine = PostgisEngine;
    let domain = fr.to_domain(&engine).map_err(ApiError::Postgis)?;
    let transformed = transform(&domain, power).map_err(ApiError::Transform)?;

    Fuzzyregion::from_domain(&transformed).map_err(ApiError::Postgis)
}

/// Errors returned by the SQL API layer.
#[derive(Debug)]
enum ApiError {
    MismatchedLevelCount { alphas: usize, geometries: usize },
    NullAlphaElement { index: usize },
    NullGeometryElement { index: usize },
    PointSridMismatch { region_srid: i32, point_srid: i32 },
    InvalidAlpha(AlphaError),
    InvalidThreshold(AlphaError),
    Transform(MembershipTransformError),
    Core(fuzzyregion_core::CoreError<PostgisError>),
    Postgis(PostgisError),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MismatchedLevelCount { alphas, geometries } => write!(
                f,
                "alpha and geometry arrays must have the same length, found {alphas} alphas and {geometries} geometries"
            ),
            Self::NullAlphaElement { index } => {
                write!(f, "alpha array element {index} must not be NULL")
            }
            Self::NullGeometryElement { index } => {
                write!(f, "geometry array element {index} must not be NULL")
            }
            Self::PointSridMismatch {
                region_srid,
                point_srid,
            } => write!(
                f,
                "point SRID {point_srid} does not match fuzzyregion SRID {region_srid}"
            ),
            Self::InvalidAlpha(error) | Self::InvalidThreshold(error) => error.fmt(f),
            Self::Transform(error) => error.fmt(f),
            Self::Core(error) => error.fmt(f),
            Self::Postgis(error) => error.fmt(f),
        }
    }
}

impl Error for ApiError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidAlpha(error) | Self::InvalidThreshold(error) => Some(error),
            Self::Transform(error) => Some(error),
            Self::Core(error) => Some(error),
            Self::Postgis(error) => Some(error),
            Self::MismatchedLevelCount { .. }
            | Self::NullAlphaElement { .. }
            | Self::NullGeometryElement { .. }
            | Self::PointSridMismatch { .. } => None,
        }
    }
}

pgrx::extension_sql!(
    r#"
    CREATE FUNCTION fuzzyregion_from_geoms(alphas double precision[], geoms geometry[])
    RETURNS fuzzyregion
    LANGUAGE SQL
    IMMUTABLE
    PARALLEL SAFE
    AS $$
      SELECT fuzzyregion_from_ewkb(
        $1,
        ARRAY(
          SELECT ST_AsEWKB(input.geom)
          FROM unnest($2) WITH ORDINALITY AS input(geom, ord)
          ORDER BY input.ord
        )
      )
    $$;

    CREATE FUNCTION fuzzyregion_support(fr fuzzyregion)
    RETURNS geometry
    LANGUAGE SQL
    IMMUTABLE
    STRICT
    PARALLEL SAFE
    AS $$
      SELECT ST_GeomFromEWKB(fuzzyregion_support_ewkb($1))
    $$;

    CREATE FUNCTION fuzzyregion_core(fr fuzzyregion)
    RETURNS geometry
    LANGUAGE SQL
    IMMUTABLE
    STRICT
    PARALLEL SAFE
    AS $$
      SELECT ST_GeomFromEWKB(fuzzyregion_core_ewkb($1))
    $$;

    CREATE FUNCTION fuzzyregion_alpha_cut(fr fuzzyregion, alpha double precision)
    RETURNS geometry
    LANGUAGE SQL
    IMMUTABLE
    STRICT
    PARALLEL SAFE
    AS $$
      SELECT ST_GeomFromEWKB(fuzzyregion_alpha_cut_ewkb($1, $2))
    $$;

    CREATE FUNCTION fuzzyregion_membership_at(fr fuzzyregion, p geometry(Point))
    RETURNS double precision
    LANGUAGE SQL
    IMMUTABLE
    STRICT
    PARALLEL SAFE
    AS $$
      SELECT fuzzyregion_membership_at_ewkb($1, ST_AsEWKB($2))
    $$;

    CREATE FUNCTION fuzzyregion_bbox(fr fuzzyregion)
    RETURNS geometry
    LANGUAGE SQL
    IMMUTABLE
    STRICT
    PARALLEL SAFE
    AS $$
      SELECT ST_GeomFromEWKB(fuzzyregion_bbox_ewkb($1))
    $$;
    "#,
    name = "fuzzyregion_geometry_wrappers",
    requires = [
        fuzzyregion_from_ewkb,
        fuzzyregion_support_ewkb,
        fuzzyregion_core_ewkb,
        fuzzyregion_alpha_cut_ewkb,
        fuzzyregion_membership_at_ewkb,
        fuzzyregion_bbox_ewkb,
    ],
    creates = [
        Function(fuzzyregion_from_geoms),
        Function(fuzzyregion_support),
        Function(fuzzyregion_core),
        Function(fuzzyregion_alpha_cut),
        Function(fuzzyregion_membership_at),
        Function(fuzzyregion_bbox),
    ],
);

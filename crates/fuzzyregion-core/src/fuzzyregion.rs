//! Canonical and unchecked fuzzyregion value types.

use crate::alpha::{Alpha, AlphaThreshold};
use crate::engine::GeometryEngine;
use crate::error::{CoreError, CoreResult, MembershipTransformError};
use crate::level::Level;

/// A fuzzyregion value that has not yet been canonicalized.
///
/// Constructors that gather raw user input should build this type first and then
/// hand it to [`Fuzzyregion::canonicalize`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UncheckedFuzzyregion<G> {
    levels: Vec<Level<G>>,
}

impl<G> UncheckedFuzzyregion<G> {
    /// Creates an unchecked fuzzyregion from raw levels.
    pub fn new(levels: Vec<Level<G>>) -> Self {
        Self { levels }
    }

    /// Returns the raw, unchecked levels.
    pub fn levels(&self) -> &[Level<G>] {
        &self.levels
    }

    /// Consumes the value and returns the underlying level list.
    pub fn into_levels(self) -> Vec<Level<G>> {
        self.levels
    }
}

/// A canonical fuzzy region composed of nested alpha levels.
///
/// The empty value is allowed so standard set operations remain closed. When
/// non-empty, levels are guaranteed to be:
///
/// - sorted from highest alpha to lowest alpha
/// - unique by alpha
/// - normalized to the geometry backend's canonical multipolygon form
/// - free of empty geometries
/// - strictly nested by alpha
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fuzzyregion<G> {
    srid: Option<i32>,
    levels: Vec<Level<G>>,
}

impl<G> Fuzzyregion<G> {
    /// Canonicalizes a raw fuzzyregion using the supplied geometry engine.
    pub fn canonicalize<E>(
        unchecked: UncheckedFuzzyregion<G>,
        engine: &E,
    ) -> CoreResult<Self, E::Error>
    where
        E: GeometryEngine<Geometry = G>,
    {
        let mut levels = Vec::with_capacity(unchecked.levels.len());

        for level in unchecked.into_levels() {
            let alpha = level.alpha();
            let geometry = engine
                .normalize_multipolygon(level.into_geometry())
                .map_err(CoreError::Geometry)?;

            if engine.is_empty(&geometry).map_err(CoreError::Geometry)? {
                return Err(CoreError::EmptyGeometry { alpha });
            }

            levels.push(Level::new(alpha, geometry));
        }

        if levels.is_empty() {
            return Ok(Self::empty(None));
        }

        levels.sort_by(|left, right| right.alpha().cmp(&left.alpha()));

        for pair in levels.windows(2) {
            if pair[0].alpha() == pair[1].alpha() {
                return Err(CoreError::DuplicateAlpha {
                    alpha: pair[0].alpha(),
                });
            }
        }

        let expected_srid = engine
            .srid(levels[0].geometry())
            .map_err(CoreError::Geometry)?;

        for level in &levels[1..] {
            let srid = engine.srid(level.geometry()).map_err(CoreError::Geometry)?;
            if srid != expected_srid {
                return Err(CoreError::MixedSrid {
                    expected: expected_srid,
                    found: srid,
                    alpha: level.alpha(),
                });
            }
        }

        let mut collapsed: Vec<Level<G>> = Vec::with_capacity(levels.len());
        for level in levels {
            let should_drop = match collapsed.last() {
                Some(previous) => engine
                    .topologically_equals(previous.geometry(), level.geometry())
                    .map_err(CoreError::Geometry)?,
                None => false,
            };

            // Canonical storage keeps the highest alpha for a repeated extent.
            if !should_drop {
                collapsed.push(level);
            }
        }

        for pair in collapsed.windows(2) {
            let higher = &pair[0];
            let lower = &pair[1];
            let is_nested = engine
                .contains(lower.geometry(), higher.geometry())
                .map_err(CoreError::Geometry)?;

            if !is_nested {
                return Err(CoreError::NonNestedLevels {
                    higher: higher.alpha(),
                    lower: lower.alpha(),
                });
            }
        }

        Ok(Self {
            srid: Some(expected_srid),
            levels: collapsed,
        })
    }

    /// Canonicalizes a raw level list using the supplied geometry engine.
    pub fn from_levels<E>(levels: Vec<Level<G>>, engine: &E) -> CoreResult<Self, E::Error>
    where
        E: GeometryEngine<Geometry = G>,
    {
        Self::canonicalize(UncheckedFuzzyregion::new(levels), engine)
    }

    /// Returns `true` when the fuzzyregion contains no levels.
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    /// Returns the shared SRID for non-empty values.
    pub fn srid(&self) -> Option<i32> {
        self.srid
    }

    /// Returns the canonical levels, sorted from highest alpha to lowest alpha.
    pub fn levels(&self) -> &[Level<G>] {
        &self.levels
    }

    /// Returns the number of stored levels.
    pub fn num_levels(&self) -> usize {
        self.levels.len()
    }

    /// Returns the highest stored alpha.
    pub fn max_alpha(&self) -> Option<Alpha> {
        self.levels.first().map(Level::alpha)
    }

    /// Returns the smallest stored alpha.
    pub fn min_alpha(&self) -> Option<Alpha> {
        self.levels.last().map(Level::alpha)
    }

    /// Returns the support geometry when the fuzzyregion is non-empty.
    pub fn support(&self) -> Option<&G> {
        self.levels.last().map(Level::geometry)
    }

    /// Returns the core geometry when an explicit `alpha = 1.0` level exists.
    pub fn core(&self) -> Option<&G> {
        self.levels
            .iter()
            .find(|level| level.alpha().is_one())
            .map(Level::geometry)
    }

    /// Returns the canonical geometry for an alpha-cut threshold.
    ///
    /// Because stored levels are nested, the alpha-cut geometry is the lowest
    /// stored level whose alpha is still greater than or equal to the threshold.
    pub fn alpha_cut(&self, threshold: AlphaThreshold) -> Option<&G> {
        if threshold.is_zero() {
            return self.support();
        }

        self.levels
            .iter()
            .rev()
            .find(|level| level.alpha().value() >= threshold.value())
            .map(Level::geometry)
    }

    /// Returns the geometry for a strict alpha-cut threshold.
    ///
    /// This is the lowest stored level whose alpha is strictly greater than the
    /// threshold. It is primarily used internally when translating standard
    /// fuzzy difference semantics into threshold geometry operations.
    fn strict_alpha_cut(&self, threshold: AlphaThreshold) -> Option<&G> {
        if threshold.is_zero() {
            return self.support();
        }

        self.levels
            .iter()
            .rev()
            .find(|level| level.alpha().value() > threshold.value())
            .map(Level::geometry)
    }

    /// Returns the highest alpha whose geometry covers the supplied point.
    pub fn membership_at<E>(
        &self,
        engine: &E,
        point: &E::Point,
    ) -> CoreResult<Option<Alpha>, E::Error>
    where
        E: GeometryEngine<Geometry = G>,
    {
        for level in &self.levels {
            let contains = engine
                .contains_point(level.geometry(), point)
                .map_err(CoreError::Geometry)?;
            if contains {
                return Ok(Some(level.alpha()));
            }
        }

        Ok(None)
    }

    /// Returns the bounding box of the support geometry.
    pub fn bbox<E>(&self, engine: &E) -> CoreResult<Option<G>, E::Error>
    where
        E: GeometryEngine<Geometry = G>,
    {
        match self.support() {
            Some(geometry) => engine
                .bounding_box(geometry)
                .map(Some)
                .map_err(CoreError::Geometry),
            None => Ok(None),
        }
    }

    /// Rescales stored membership values so the maximum alpha becomes `1.0`.
    pub fn normalize_membership(&self) -> Self
    where
        G: Clone,
    {
        match self.max_alpha() {
            Some(max_alpha) if !max_alpha.is_one() => {
                self.remap_membership_values(|alpha| alpha.value() / max_alpha.value())
            }
            _ => self.clone(),
        }
    }

    /// Sharpens membership values by applying `alpha' = alpha^power`.
    pub fn concentrate_membership(&self, power: f64) -> Result<Self, MembershipTransformError>
    where
        G: Clone,
    {
        validate_membership_power(power)?;
        Ok(self.remap_membership_values(|alpha| alpha.value().powf(power)))
    }

    /// Softens membership values by applying `alpha' = alpha^(1/power)`.
    pub fn dilate_membership(&self, power: f64) -> Result<Self, MembershipTransformError>
    where
        G: Clone,
    {
        validate_membership_power(power)?;
        Ok(self.remap_membership_values(|alpha| alpha.value().powf(1.0 / power)))
    }

    /// Computes the MVP standard fuzzy union.
    pub fn union<E>(&self, other: &Self, engine: &E) -> CoreResult<Self, E::Error>
    where
        E: GeometryEngine<Geometry = G>,
        G: Clone,
    {
        self.ensure_compatible_srid(other)?;

        let mut levels = Vec::new();
        for alpha in self.alpha_domain(other) {
            let threshold = AlphaThreshold::from(alpha);
            let geometry = match (self.alpha_cut(threshold), other.alpha_cut(threshold)) {
                (Some(left), Some(right)) => {
                    Some(engine.union(left, right).map_err(CoreError::Geometry)?)
                }
                (Some(left), None) => Some(left.clone()),
                (None, Some(right)) => Some(right.clone()),
                (None, None) => None,
            };

            self.push_non_empty_level(alpha, geometry, engine, &mut levels)?;
        }

        self.finish_set_operation(levels, other.srid, engine)
    }

    /// Computes the MVP standard fuzzy intersection.
    pub fn intersection<E>(&self, other: &Self, engine: &E) -> CoreResult<Self, E::Error>
    where
        E: GeometryEngine<Geometry = G>,
        G: Clone,
    {
        self.ensure_compatible_srid(other)?;

        let mut levels = Vec::new();
        for alpha in self.alpha_domain(other) {
            let threshold = AlphaThreshold::from(alpha);
            let geometry = match (self.alpha_cut(threshold), other.alpha_cut(threshold)) {
                (Some(left), Some(right)) => Some(
                    engine
                        .intersection(left, right)
                        .map_err(CoreError::Geometry)?,
                ),
                _ => None,
            };

            self.push_non_empty_level(alpha, geometry, engine, &mut levels)?;
        }

        self.finish_set_operation(levels, other.srid, engine)
    }

    /// Computes the MVP standard fuzzy difference `self - other`.
    ///
    /// For the standard fuzzy difference `min(mu_self, 1 - mu_other)`, the
    /// crisp threshold geometry at alpha `t` is:
    ///
    /// `alpha_cut(self, t) - strict_alpha_cut(other, 1 - t)`.
    pub fn difference<E>(&self, other: &Self, engine: &E) -> CoreResult<Self, E::Error>
    where
        E: GeometryEngine<Geometry = G>,
        G: Clone,
    {
        self.ensure_compatible_srid(other)?;

        let mut levels = Vec::new();
        for alpha in self.alpha_domain(other) {
            let threshold = AlphaThreshold::from(alpha);
            let exclusion_threshold = AlphaThreshold::try_from(1.0 - alpha.value())
                .expect("1 - alpha for alpha in (0, 1] must stay within [0, 1]");
            let geometry = match (
                self.alpha_cut(threshold),
                other.strict_alpha_cut(exclusion_threshold),
            ) {
                (Some(left), Some(right)) => Some(
                    engine
                        .difference(left, right)
                        .map_err(CoreError::Geometry)?,
                ),
                (Some(left), None) => Some(left.clone()),
                (None, _) => None,
            };

            self.push_non_empty_level(alpha, geometry, engine, &mut levels)?;
        }

        self.finish_set_operation(levels, other.srid, engine)
    }

    fn empty(srid: Option<i32>) -> Self {
        Self {
            srid,
            levels: Vec::new(),
        }
    }

    fn alpha_domain(&self, other: &Self) -> Vec<Alpha> {
        let mut domain = Vec::with_capacity(self.levels.len() + other.levels.len());
        domain.extend(self.levels.iter().map(Level::alpha));
        domain.extend(other.levels.iter().map(Level::alpha));
        domain.sort_unstable_by(|left, right| right.cmp(left));
        domain.dedup();
        domain
    }

    fn ensure_compatible_srid<E>(&self, other: &Self) -> CoreResult<(), E> {
        match (self.srid, other.srid) {
            (Some(left), Some(right)) if left != right => {
                Err(CoreError::MixedOperandSrid { left, right })
            }
            _ => Ok(()),
        }
    }

    fn push_non_empty_level<E>(
        &self,
        alpha: Alpha,
        geometry: Option<G>,
        engine: &E,
        levels: &mut Vec<Level<G>>,
    ) -> CoreResult<(), E::Error>
    where
        E: GeometryEngine<Geometry = G>,
    {
        if let Some(geometry) = geometry {
            let geometry = engine
                .normalize_multipolygon(geometry)
                .map_err(CoreError::Geometry)?;

            if !engine.is_empty(&geometry).map_err(CoreError::Geometry)? {
                levels.push(Level::new(alpha, geometry));
            }
        }

        Ok(())
    }

    fn finish_set_operation<E>(
        &self,
        levels: Vec<Level<G>>,
        other_srid: Option<i32>,
        engine: &E,
    ) -> CoreResult<Self, E::Error>
    where
        E: GeometryEngine<Geometry = G>,
    {
        if levels.is_empty() {
            return Ok(Self::empty(self.srid.or(other_srid)));
        }

        Self::from_levels(levels, engine)
    }

    fn remap_membership_values<F>(&self, remap: F) -> Self
    where
        G: Clone,
        F: Fn(Alpha) -> f64,
    {
        let levels = self
            .levels
            .iter()
            .map(|level| {
                let remapped = Alpha::try_from(remap(level.alpha()))
                    .expect("membership remapping must preserve alpha validity");
                Level::new(remapped, level.geometry().clone())
            })
            .collect();

        Self {
            srid: self.srid,
            levels,
        }
    }
}

fn validate_membership_power(power: f64) -> Result<(), MembershipTransformError> {
    if !power.is_finite() {
        return Err(MembershipTransformError::NotFinitePower { value: power });
    }

    if power <= 1.0 {
        return Err(MembershipTransformError::PowerMustBeGreaterThanOne { value: power });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::convert::Infallible;

    use crate::MembershipTransformError;

    use super::{Alpha, AlphaThreshold, Fuzzyregion, GeometryEngine, Level};

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct MockGeometry {
        srid: i32,
        cells: BTreeSet<i32>,
    }

    impl MockGeometry {
        fn new(srid: i32, cells: &[i32]) -> Self {
            Self {
                srid,
                cells: cells.iter().copied().collect(),
            }
        }
    }

    #[derive(Debug, Default)]
    struct MockEngine;

    impl GeometryEngine for MockEngine {
        type Geometry = MockGeometry;
        type Point = i32;
        type Error = Infallible;

        fn normalize_multipolygon(
            &self,
            geometry: Self::Geometry,
        ) -> Result<Self::Geometry, Self::Error> {
            Ok(geometry)
        }

        fn is_empty(&self, geometry: &Self::Geometry) -> Result<bool, Self::Error> {
            Ok(geometry.cells.is_empty())
        }

        fn srid(&self, geometry: &Self::Geometry) -> Result<i32, Self::Error> {
            Ok(geometry.srid)
        }

        fn topologically_equals(
            &self,
            left: &Self::Geometry,
            right: &Self::Geometry,
        ) -> Result<bool, Self::Error> {
            Ok(left.srid == right.srid && left.cells == right.cells)
        }

        fn contains(
            &self,
            container: &Self::Geometry,
            containee: &Self::Geometry,
        ) -> Result<bool, Self::Error> {
            Ok(container.srid == containee.srid && containee.cells.is_subset(&container.cells))
        }

        fn contains_point(
            &self,
            geometry: &Self::Geometry,
            point: &Self::Point,
        ) -> Result<bool, Self::Error> {
            Ok(geometry.cells.contains(point))
        }

        fn union(
            &self,
            left: &Self::Geometry,
            right: &Self::Geometry,
        ) -> Result<Self::Geometry, Self::Error> {
            let mut cells = left.cells.clone();
            cells.extend(right.cells.iter().copied());
            Ok(MockGeometry {
                srid: left.srid,
                cells,
            })
        }

        fn intersection(
            &self,
            left: &Self::Geometry,
            right: &Self::Geometry,
        ) -> Result<Self::Geometry, Self::Error> {
            Ok(MockGeometry {
                srid: left.srid,
                cells: left.cells.intersection(&right.cells).copied().collect(),
            })
        }

        fn difference(
            &self,
            left: &Self::Geometry,
            right: &Self::Geometry,
        ) -> Result<Self::Geometry, Self::Error> {
            Ok(MockGeometry {
                srid: left.srid,
                cells: left.cells.difference(&right.cells).copied().collect(),
            })
        }

        fn area(&self, geometry: &Self::Geometry) -> Result<f64, Self::Error> {
            Ok(geometry.cells.len() as f64)
        }

        fn bounding_box(&self, geometry: &Self::Geometry) -> Result<Self::Geometry, Self::Error> {
            Ok(geometry.clone())
        }
    }

    fn alpha(value: f64) -> Alpha {
        Alpha::try_from(value).unwrap()
    }

    #[test]
    fn canonicalization_sorts_levels_and_collapses_duplicate_extents() {
        let engine = MockEngine;
        let region = Fuzzyregion::from_levels(
            vec![
                Level::new(alpha(0.5), MockGeometry::new(4326, &[1, 2])),
                Level::new(alpha(1.0), MockGeometry::new(4326, &[1])),
                Level::new(alpha(0.8), MockGeometry::new(4326, &[1])),
            ],
            &engine,
        )
        .unwrap();

        assert_eq!(region.srid(), Some(4326));
        assert_eq!(region.levels().len(), 2);
        assert_eq!(region.levels()[0].alpha().value(), 1.0);
        assert_eq!(region.levels()[1].alpha().value(), 0.5);
    }

    #[test]
    fn alpha_cut_zero_returns_support_and_membership_uses_highest_covering_alpha() {
        let engine = MockEngine;
        let region = Fuzzyregion::from_levels(
            vec![
                Level::new(alpha(1.0), MockGeometry::new(4326, &[1])),
                Level::new(alpha(0.8), MockGeometry::new(4326, &[1, 2])),
                Level::new(alpha(0.5), MockGeometry::new(4326, &[1, 2])),
            ],
            &engine,
        )
        .unwrap();

        let support = region
            .alpha_cut(AlphaThreshold::try_from(0.0).unwrap())
            .unwrap();
        let cut = region
            .alpha_cut(AlphaThreshold::try_from(0.6).unwrap())
            .unwrap();

        assert_eq!(support.cells, BTreeSet::from([1, 2]));
        assert_eq!(cut.cells, BTreeSet::from([1, 2]));
        assert_eq!(region.membership_at(&engine, &1).unwrap(), Some(alpha(1.0)));
        assert_eq!(region.membership_at(&engine, &2).unwrap(), Some(alpha(0.8)));
        assert_eq!(region.membership_at(&engine, &3).unwrap(), None);
    }

    #[test]
    fn core_can_be_absent_without_invalidating_the_value() {
        let engine = MockEngine;
        let region = Fuzzyregion::from_levels(
            vec![
                Level::new(alpha(0.8), MockGeometry::new(4326, &[1])),
                Level::new(alpha(0.5), MockGeometry::new(4326, &[1, 2])),
            ],
            &engine,
        )
        .unwrap();

        assert_eq!(region.core(), None);
        assert_eq!(region.support().unwrap().cells, BTreeSet::from([1, 2]));
        assert_eq!(
            region
                .alpha_cut(AlphaThreshold::try_from(0.7).unwrap())
                .unwrap()
                .cells,
            BTreeSet::from([1]),
        );
    }

    #[test]
    fn standard_union_uses_the_combined_alpha_domain() {
        let engine = MockEngine;
        let left = Fuzzyregion::from_levels(
            vec![
                Level::new(alpha(1.0), MockGeometry::new(4326, &[1])),
                Level::new(alpha(0.5), MockGeometry::new(4326, &[1, 2])),
            ],
            &engine,
        )
        .unwrap();
        let right = Fuzzyregion::from_levels(
            vec![
                Level::new(alpha(0.7), MockGeometry::new(4326, &[2])),
                Level::new(alpha(0.4), MockGeometry::new(4326, &[2, 3])),
            ],
            &engine,
        )
        .unwrap();

        let union = left.union(&right, &engine).unwrap();

        assert_eq!(union.levels().len(), 3);
        assert_eq!(union.levels()[0].alpha().value(), 1.0);
        assert_eq!(union.levels()[1].alpha().value(), 0.7);
        assert_eq!(union.levels()[2].alpha().value(), 0.4);
        assert_eq!(
            union.levels()[2].geometry().cells,
            BTreeSet::from([1, 2, 3]),
        );
    }

    #[test]
    fn disjoint_intersection_returns_the_empty_value() {
        let engine = MockEngine;
        let left = Fuzzyregion::from_levels(
            vec![Level::new(alpha(0.5), MockGeometry::new(4326, &[1, 2]))],
            &engine,
        )
        .unwrap();
        let right = Fuzzyregion::from_levels(
            vec![Level::new(alpha(0.5), MockGeometry::new(4326, &[3, 4]))],
            &engine,
        )
        .unwrap();

        let intersection = left.intersection(&right, &engine).unwrap();

        assert!(intersection.is_empty());
        assert_eq!(intersection.srid(), Some(4326));
        assert_eq!(intersection.support(), None);
    }

    #[test]
    fn standard_difference_uses_complement_thresholds_instead_of_plain_alpha_cut_difference() {
        let engine = MockEngine;
        let left = Fuzzyregion::from_levels(
            vec![Level::new(alpha(0.5), MockGeometry::new(4326, &[1, 2, 3]))],
            &engine,
        )
        .unwrap();
        let right = Fuzzyregion::from_levels(
            vec![
                Level::new(alpha(0.7), MockGeometry::new(4326, &[2])),
                Level::new(alpha(0.3), MockGeometry::new(4326, &[2, 3])),
            ],
            &engine,
        )
        .unwrap();

        let difference = left.difference(&right, &engine).unwrap();

        assert_eq!(difference.levels().len(), 2);
        assert_eq!(difference.levels()[0].alpha().value(), 0.5);
        assert_eq!(
            difference.levels()[0].geometry().cells,
            BTreeSet::from([1, 3])
        );
        assert_eq!(difference.levels()[1].alpha().value(), 0.3);
        assert_eq!(
            difference.levels()[1].geometry().cells,
            BTreeSet::from([1, 2, 3])
        );
        assert_eq!(
            difference.membership_at(&engine, &1).unwrap(),
            Some(alpha(0.5))
        );
        assert_eq!(
            difference.membership_at(&engine, &2).unwrap(),
            Some(alpha(0.3))
        );
        assert_eq!(
            difference.membership_at(&engine, &3).unwrap(),
            Some(alpha(0.5))
        );
    }

    #[test]
    fn normalize_membership_rescales_the_highest_alpha_to_one() {
        let engine = MockEngine;
        let region = Fuzzyregion::from_levels(
            vec![
                Level::new(alpha(0.8), MockGeometry::new(4326, &[1])),
                Level::new(alpha(0.4), MockGeometry::new(4326, &[1, 2])),
            ],
            &engine,
        )
        .unwrap();

        let normalized = region.normalize_membership();

        assert_eq!(normalized.max_alpha().unwrap().value(), 1.0);
        assert_eq!(normalized.min_alpha().unwrap().value(), 0.5);
        assert_eq!(normalized.num_levels(), 2);
        assert_eq!(
            normalized.levels()[0].geometry(),
            region.levels()[0].geometry()
        );
        assert_eq!(
            normalized.levels()[1].geometry(),
            region.levels()[1].geometry()
        );
    }

    #[test]
    fn membership_transforms_only_remap_alphas() {
        let engine = MockEngine;
        let region = Fuzzyregion::from_levels(
            vec![
                Level::new(alpha(1.0), MockGeometry::new(4326, &[1])),
                Level::new(alpha(0.25), MockGeometry::new(4326, &[1, 2])),
            ],
            &engine,
        )
        .unwrap();

        let concentrated = region.concentrate_membership(2.0).unwrap();
        let dilated = region.dilate_membership(2.0).unwrap();

        assert_eq!(concentrated.levels()[0].alpha().value(), 1.0);
        assert_eq!(concentrated.levels()[1].alpha().value(), 0.0625);
        assert_eq!(
            concentrated.levels()[1].geometry(),
            region.levels()[1].geometry()
        );

        assert_eq!(dilated.levels()[0].alpha().value(), 1.0);
        assert_eq!(dilated.levels()[1].alpha().value(), 0.5);
        assert_eq!(
            dilated.levels()[1].geometry(),
            region.levels()[1].geometry()
        );
    }

    #[test]
    fn membership_transforms_reject_non_finite_or_small_powers() {
        let engine = MockEngine;
        let region = Fuzzyregion::from_levels(
            vec![Level::new(alpha(0.5), MockGeometry::new(4326, &[1, 2]))],
            &engine,
        )
        .unwrap();

        assert_eq!(
            region.concentrate_membership(1.0),
            Err(MembershipTransformError::PowerMustBeGreaterThanOne { value: 1.0 })
        );
        assert_eq!(
            region.dilate_membership(f64::INFINITY),
            Err(MembershipTransformError::NotFinitePower {
                value: f64::INFINITY
            })
        );
    }

    #[test]
    fn intersection_drops_non_polygonal_results_before_canonicalization() {
        #[derive(Clone, Debug, PartialEq, Eq)]
        struct ShapeGeometry {
            srid: i32,
            cells: BTreeSet<i32>,
            polygonal: bool,
        }

        #[derive(Debug, Default)]
        struct ShapeEngine;

        impl GeometryEngine for ShapeEngine {
            type Geometry = ShapeGeometry;
            type Point = i32;
            type Error = Infallible;

            fn normalize_multipolygon(
                &self,
                geometry: Self::Geometry,
            ) -> Result<Self::Geometry, Self::Error> {
                if geometry.polygonal {
                    Ok(geometry)
                } else {
                    Ok(ShapeGeometry {
                        srid: geometry.srid,
                        cells: BTreeSet::new(),
                        polygonal: true,
                    })
                }
            }

            fn is_empty(&self, geometry: &Self::Geometry) -> Result<bool, Self::Error> {
                Ok(geometry.cells.is_empty())
            }

            fn srid(&self, geometry: &Self::Geometry) -> Result<i32, Self::Error> {
                Ok(geometry.srid)
            }

            fn topologically_equals(
                &self,
                left: &Self::Geometry,
                right: &Self::Geometry,
            ) -> Result<bool, Self::Error> {
                Ok(left.srid == right.srid && left.cells == right.cells)
            }

            fn contains(
                &self,
                container: &Self::Geometry,
                containee: &Self::Geometry,
            ) -> Result<bool, Self::Error> {
                Ok(container.srid == containee.srid && containee.cells.is_subset(&container.cells))
            }

            fn contains_point(
                &self,
                geometry: &Self::Geometry,
                point: &Self::Point,
            ) -> Result<bool, Self::Error> {
                Ok(geometry.cells.contains(point))
            }

            fn union(
                &self,
                left: &Self::Geometry,
                right: &Self::Geometry,
            ) -> Result<Self::Geometry, Self::Error> {
                let mut cells = left.cells.clone();
                cells.extend(right.cells.iter().copied());
                Ok(ShapeGeometry {
                    srid: left.srid,
                    cells,
                    polygonal: true,
                })
            }

            fn intersection(
                &self,
                left: &Self::Geometry,
                right: &Self::Geometry,
            ) -> Result<Self::Geometry, Self::Error> {
                Ok(ShapeGeometry {
                    srid: left.srid,
                    cells: left.cells.intersection(&right.cells).copied().collect(),
                    polygonal: false,
                })
            }

            fn difference(
                &self,
                left: &Self::Geometry,
                right: &Self::Geometry,
            ) -> Result<Self::Geometry, Self::Error> {
                Ok(ShapeGeometry {
                    srid: left.srid,
                    cells: left.cells.difference(&right.cells).copied().collect(),
                    polygonal: true,
                })
            }

            fn area(&self, geometry: &Self::Geometry) -> Result<f64, Self::Error> {
                Ok(geometry.cells.len() as f64)
            }

            fn bounding_box(
                &self,
                geometry: &Self::Geometry,
            ) -> Result<Self::Geometry, Self::Error> {
                Ok(geometry.clone())
            }
        }

        let engine = ShapeEngine;
        let left = Fuzzyregion::from_levels(
            vec![Level::new(
                alpha(1.0),
                ShapeGeometry {
                    srid: 4326,
                    cells: [1].into_iter().collect(),
                    polygonal: true,
                },
            )],
            &engine,
        )
        .unwrap();
        let right = Fuzzyregion::from_levels(
            vec![Level::new(
                alpha(1.0),
                ShapeGeometry {
                    srid: 4326,
                    cells: [1].into_iter().collect(),
                    polygonal: true,
                },
            )],
            &engine,
        )
        .unwrap();

        let intersection = left.intersection(&right, &engine).unwrap();

        assert!(intersection.is_empty());
        assert_eq!(intersection.srid(), Some(4326));
    }
}

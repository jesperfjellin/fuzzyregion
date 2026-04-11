//! PostgreSQL base type for `fuzzyregion`.
//!
//! The SQL type is a thin wrapper around [`StoredFuzzyregion`]. PostgreSQL
//! stores the binary payload directly as a varlena datum, while text I/O uses a
//! deliberately explicit low-level syntax:
//!
//! ```text
//! v1;srid=4326;levels=[1:0102abcd,0.5:deadbeef]
//! ```
//!
//! This syntax is lossless and versioned, but it is not intended to be the
//! primary user-facing API. Named SQL constructor and projection functions are
//! the preferred surface.

use std::error::Error;
use std::ffi::CStr;
use std::fmt;
use std::str::FromStr;

use fuzzyregion_core::{Alpha, AlphaThreshold, Fuzzyregion as DomainFuzzyregion};
use pgrx::prelude::*;
use pgrx::{FromDatum, InOutFuncs, IntoDatum, StringInfo, pg_sys, regtypein};

use crate::interop::{
    PostgisEngine, PostgisError, PostgisGeometry, decode_stored_fuzzyregion,
    encode_domain_fuzzyregion,
};
use crate::storage::{StorageError, StoredFuzzyregion, StoredLevel};

const SQL_TYPE_NAME: &str = "fuzzyregion";
const TEXT_PREFIX: &str = "v1;srid=";
const TEXT_LEVELS_MARKER: &str = ";levels=[";

/// PostgreSQL wrapper for the extension-defined `fuzzyregion` base type.
#[derive(Clone, Debug, PartialEq, Eq, PostgresType)]
#[inoutfuncs]
#[bikeshed_postgres_type_manually_impl_from_into_datum]
pub struct Fuzzyregion {
    stored: StoredFuzzyregion,
}

impl Fuzzyregion {
    /// Creates a SQL wrapper from a stored payload.
    pub fn from_stored(stored: StoredFuzzyregion) -> Self {
        Self { stored }
    }

    /// Returns the persisted storage payload.
    pub fn stored(&self) -> &StoredFuzzyregion {
        &self.stored
    }

    /// Creates a SQL wrapper from a canonical domain value.
    pub fn from_domain(value: &DomainFuzzyregion<PostgisGeometry>) -> Result<Self, PostgisError> {
        Ok(Self {
            stored: encode_domain_fuzzyregion(value)?,
        })
    }

    /// Decodes the stored payload back into the canonical domain model.
    pub fn to_domain(
        &self,
        engine: &PostgisEngine,
    ) -> Result<DomainFuzzyregion<PostgisGeometry>, PostgisError> {
        decode_stored_fuzzyregion(self.stored.clone(), engine).map_err(|error| match error {
            fuzzyregion_core::CoreError::Geometry(error) => error,
            other => PostgisError::CorruptStoredValue(format!(
                "unexpected canonicalization failure while decoding fuzzyregion: {other}"
            )),
        })
    }

    /// Returns the shared SRID for non-empty values.
    pub fn srid(&self) -> Option<i32> {
        self.stored.srid()
    }

    /// Returns the support geometry EWKB, if present.
    pub fn support_ewkb(&self) -> Option<&[u8]> {
        self.stored.levels().last().map(StoredLevel::geometry_ewkb)
    }

    /// Returns the core geometry EWKB, if an explicit `alpha = 1.0` level exists.
    pub fn core_ewkb(&self) -> Option<&[u8]> {
        self.stored
            .levels()
            .iter()
            .find(|level| level.alpha().is_one())
            .map(StoredLevel::geometry_ewkb)
    }

    /// Returns the alpha-cut geometry EWKB, if present.
    pub fn alpha_cut_ewkb(&self, threshold: AlphaThreshold) -> Option<&[u8]> {
        if threshold.is_zero() {
            return self.support_ewkb();
        }

        self.stored
            .levels()
            .iter()
            .rev()
            .find(|level| level.alpha().value() >= threshold.value())
            .map(StoredLevel::geometry_ewkb)
    }

    fn parse_text(input: &str) -> Result<Self, TextFormatError> {
        let rest = input
            .trim()
            .strip_prefix(TEXT_PREFIX)
            .ok_or(TextFormatError::InvalidEnvelope)?;
        let (srid_text, levels_text) = rest
            .split_once(TEXT_LEVELS_MARKER)
            .ok_or(TextFormatError::InvalidEnvelope)?;
        let levels_text = levels_text
            .strip_suffix(']')
            .ok_or(TextFormatError::InvalidEnvelope)?;

        let srid = if srid_text == "null" {
            None
        } else {
            Some(i32::from_str(srid_text).map_err(|_| TextFormatError::InvalidSrid)?)
        };

        let mut levels = Vec::new();
        if !levels_text.is_empty() {
            for entry in levels_text.split(',') {
                let (alpha_text, ewkb_hex) = entry
                    .split_once(':')
                    .ok_or(TextFormatError::InvalidLevelEntry)?;
                let alpha = Alpha::try_from(
                    f64::from_str(alpha_text).map_err(|_| TextFormatError::InvalidAlpha)?,
                )
                .map_err(|error| TextFormatError::Storage(error.into()))?;
                let ewkb = decode_hex(ewkb_hex)?;
                levels.push(StoredLevel::new(alpha, ewkb).map_err(TextFormatError::Storage)?);
            }
        }

        Ok(Self {
            stored: StoredFuzzyregion::new(srid, levels).map_err(TextFormatError::Storage)?,
        })
    }

    fn to_text(&self) -> String {
        let srid = self
            .stored
            .srid()
            .map_or_else(|| "null".to_string(), |value| value.to_string());
        let levels = self
            .stored
            .levels()
            .iter()
            .map(|level| {
                format!(
                    "{}:{}",
                    level.alpha().value(),
                    encode_hex(level.geometry_ewkb())
                )
            })
            .collect::<Vec<_>>()
            .join(",");

        format!("v1;srid={srid};levels=[{levels}]")
    }

    /// Returns the low-level debug text representation used by type output.
    pub fn to_text_representation(&self) -> String {
        self.to_text()
    }
}

impl InOutFuncs for Fuzzyregion {
    fn input(input: &CStr) -> Self
    where
        Self: Sized,
    {
        let parsed = Self::parse_text(input.to_str().unwrap_or_else(|_| {
            pgrx::error!("invalid {SQL_TYPE_NAME} literal: input must be UTF-8")
        }))
        .unwrap_or_else(|error| pgrx::error!("invalid {SQL_TYPE_NAME} literal: {error}"));

        let engine = PostgisEngine;
        let domain = parsed
            .to_domain(&engine)
            .unwrap_or_else(|error| pgrx::error!("invalid {SQL_TYPE_NAME} literal: {error}"));

        Self::from_domain(&domain)
            .unwrap_or_else(|error| pgrx::error!("invalid {SQL_TYPE_NAME} literal: {error}"))
    }

    fn output(&self, buffer: &mut StringInfo) {
        buffer.push_str(&self.to_text());
    }
}

impl IntoDatum for Fuzzyregion {
    fn into_datum(self) -> Option<pg_sys::Datum> {
        self.stored.encode_body().into_datum()
    }

    fn type_oid() -> pg_sys::Oid {
        regtypein(SQL_TYPE_NAME)
    }
}

impl FromDatum for Fuzzyregion {
    unsafe fn from_polymorphic_datum(
        datum: pg_sys::Datum,
        is_null: bool,
        typoid: pg_sys::Oid,
    ) -> Option<Self> {
        let bytes = unsafe { Vec::<u8>::from_polymorphic_datum(datum, is_null, typoid) }?;
        let stored = StoredFuzzyregion::decode_body(&bytes)
            .unwrap_or_else(|error| pgrx::error!("invalid on-disk {SQL_TYPE_NAME} value: {error}"));
        Some(Self { stored })
    }
}

unsafe impl pgrx::callconv::BoxRet for Fuzzyregion {
    unsafe fn box_into<'fcx>(
        self,
        fcinfo: &mut pgrx::callconv::FcInfo<'fcx>,
    ) -> pgrx::datum::Datum<'fcx> {
        match IntoDatum::into_datum(self) {
            None => fcinfo.return_null(),
            Some(datum) => unsafe { fcinfo.return_raw_datum(datum) },
        }
    }
}

unsafe impl pgrx::datum::UnboxDatum for Fuzzyregion {
    type As<'src>
        = Self
    where
        Self: 'src;

    unsafe fn unbox<'src>(datum: pgrx::datum::Datum<'src>) -> Self::As<'src>
    where
        Self: 'src,
    {
        unsafe { <Self as FromDatum>::from_datum(std::mem::transmute(datum), false).unwrap() }
    }
}

unsafe impl<'fcx> pgrx::callconv::ArgAbi<'fcx> for Fuzzyregion {
    unsafe fn unbox_arg_unchecked(arg: pgrx::callconv::Arg<'_, 'fcx>) -> Self {
        let index = arg.index();
        unsafe {
            arg.unbox_arg_using_from_datum()
                .unwrap_or_else(|| panic!("argument {index} must not be null"))
        }
    }
}

#[derive(Debug)]
enum TextFormatError {
    InvalidEnvelope,
    InvalidSrid,
    InvalidLevelEntry,
    InvalidAlpha,
    InvalidHexLength,
    InvalidHexDigit { index: usize, found: char },
    Storage(StorageError),
}

impl fmt::Display for TextFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEnvelope => write!(
                f,
                "expected syntax `v1;srid=<int|null>;levels=[alpha:hex,...]`"
            ),
            Self::InvalidSrid => write!(f, "invalid SRID in fuzzyregion literal"),
            Self::InvalidLevelEntry => write!(f, "invalid level entry in fuzzyregion literal"),
            Self::InvalidAlpha => write!(f, "invalid alpha in fuzzyregion literal"),
            Self::InvalidHexLength => write!(f, "hex EWKB must contain an even number of digits"),
            Self::InvalidHexDigit { index, found } => {
                write!(f, "invalid hex digit `{found}` at position {index}")
            }
            Self::Storage(error) => error.fmt(f),
        }
    }
}

impl Error for TextFormatError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::InvalidEnvelope
            | Self::InvalidSrid
            | Self::InvalidLevelEntry
            | Self::InvalidAlpha
            | Self::InvalidHexLength
            | Self::InvalidHexDigit { .. } => None,
        }
    }
}

pub(crate) fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn decode_hex(input: &str) -> Result<Vec<u8>, TextFormatError> {
    let bytes = input.as_bytes();
    if bytes.len() % 2 != 0 {
        return Err(TextFormatError::InvalidHexLength);
    }

    let mut out = Vec::with_capacity(bytes.len() / 2);
    for (index, pair) in bytes.chunks_exact(2).enumerate() {
        let high = decode_nibble(pair[0], index * 2)?;
        let low = decode_nibble(pair[1], index * 2 + 1)?;
        out.push((high << 4) | low);
    }

    Ok(out)
}

fn decode_nibble(byte: u8, index: usize) -> Result<u8, TextFormatError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(TextFormatError::InvalidHexDigit {
            index,
            found: byte as char,
        }),
    }
}

#[cfg(test)]
mod tests {
    use fuzzyregion_core::Alpha;

    use super::Fuzzyregion;
    use crate::storage::{StoredFuzzyregion, StoredLevel};

    fn level(alpha: f64, ewkb: &[u8]) -> StoredLevel {
        StoredLevel::new(Alpha::try_from(alpha).unwrap(), ewkb.to_vec()).unwrap()
    }

    #[test]
    fn text_roundtrip_preserves_the_payload() {
        let stored = StoredFuzzyregion::new(
            Some(4326),
            vec![
                level(1.0, &[0x01, 0xab]),
                level(0.5, &[0xde, 0xad, 0xbe, 0xef]),
            ],
        )
        .unwrap();

        let value = Fuzzyregion::from_stored(stored.clone());
        let reparsed = Fuzzyregion::parse_text(&value.to_text()).unwrap();

        assert_eq!(reparsed.stored(), &stored);
    }

    #[test]
    fn empty_value_text_roundtrip_is_supported() {
        let stored = StoredFuzzyregion::empty(None);
        let value = Fuzzyregion::from_stored(stored.clone());
        let reparsed = Fuzzyregion::parse_text(&value.to_text()).unwrap();

        assert_eq!(reparsed.stored(), &stored);
    }
}

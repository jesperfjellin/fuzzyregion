//! Versioned binary storage for the PostgreSQL `fuzzyregion` type.
//!
//! This module defines the long-term payload contract for the extension-defined
//! PostgreSQL base type. The bytes handled here are the type body that will live
//! inside a varlena value once PostgreSQL I/O is added.
//!
//! Version `1` uses a big-endian envelope with this layout:
//!
//! 1. magic bytes `FZRG`
//! 2. format version as `u8`
//! 3. flags as `u8`
//! 4. reserved `u16`, currently required to be zero
//! 5. level count as `u32`
//! 6. optional SRID as `i32` when the `HAS_SRID` flag is set
//! 7. repeated level records:
//!    - alpha as `f64`
//!    - EWKB length as `u32`
//!    - EWKB bytes
//!
//! The envelope is fixed-endian and owned by this project. The EWKB payload
//! keeps its own internal byte-order marker as defined by the geometry format.

use std::error::Error;
use std::fmt;

use fuzzyregion_core::{Alpha, AlphaError};

/// Magic bytes that identify a `fuzzyregion` binary payload.
pub const STORAGE_MAGIC: [u8; 4] = *b"FZRG";

/// The newest storage format version supported by this crate.
pub const CURRENT_STORAGE_VERSION: StorageVersion = StorageVersion::V1;

const FLAG_HAS_SRID: u8 = 0b0000_0001;
const SUPPORTED_FLAGS_MASK: u8 = FLAG_HAS_SRID;
const RESERVED_HEADER: u16 = 0;
const BASE_HEADER_LEN: usize = 12;
const OPTIONAL_SRID_LEN: usize = 4;
const LEVEL_RECORD_HEADER_LEN: usize = 12;

/// Supported on-disk payload versions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum StorageVersion {
    /// Initial binary payload format.
    V1 = 1,
}

impl StorageVersion {
    /// Returns the version as the raw byte stored in the payload header.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Errors returned by the binary storage layer.
#[derive(Clone, Debug, PartialEq)]
pub enum StorageError {
    /// The payload was too short to decode the requested field.
    Truncated {
        /// The byte offset where decoding failed.
        offset: usize,
        /// The number of bytes required to continue decoding.
        needed: usize,
        /// The number of bytes still available at `offset`.
        remaining: usize,
    },
    /// The payload did not begin with the expected magic bytes.
    InvalidMagic {
        /// The four bytes found at the start of the payload.
        found: [u8; 4],
    },
    /// The payload version is not supported by this crate.
    UnsupportedVersion {
        /// The raw version byte found in the payload.
        found: u8,
    },
    /// The payload used flag bits not understood by this version.
    UnsupportedFlags {
        /// The raw flags byte found in the payload.
        found: u8,
    },
    /// The reserved header field was expected to be zero but was not.
    NonZeroReservedHeader {
        /// The raw reserved field value.
        found: u16,
    },
    /// A non-empty fuzzyregion payload omitted its SRID.
    MissingSridForNonEmptyPayload,
    /// A level stored no EWKB bytes.
    EmptyEwkb {
        /// The alpha attached to the offending level.
        alpha: Alpha,
    },
    /// The level list is too large to fit in the binary format.
    TooManyLevels {
        /// The attempted level count.
        count: usize,
    },
    /// A geometry byte payload is too large to fit in the binary format.
    EwkbTooLarge {
        /// The alpha attached to the offending level.
        alpha: Alpha,
        /// The attempted byte length.
        len: usize,
    },
    /// Levels were not stored in strictly descending alpha order.
    LevelsOutOfOrder {
        /// The alpha that appeared first.
        previous: Alpha,
        /// The alpha that appeared after `previous`.
        next: Alpha,
    },
    /// Two consecutive levels used the same alpha value.
    DuplicateAlpha {
        /// The repeated alpha.
        alpha: Alpha,
    },
    /// Extra bytes remained after a full payload was decoded.
    TrailingBytes {
        /// The number of unread trailing bytes.
        remaining: usize,
    },
    /// A decoded alpha was outside the valid range for stored levels.
    InvalidAlpha(AlphaError),
}

impl From<AlphaError> for StorageError {
    fn from(value: AlphaError) -> Self {
        Self::InvalidAlpha(value)
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated {
                offset,
                needed,
                remaining,
            } => write!(
                f,
                "payload truncated at offset {offset}: need {needed} bytes, only {remaining} remain"
            ),
            Self::InvalidMagic { found } => {
                let display = String::from_utf8_lossy(found);
                write!(f, "invalid storage magic {display:?}")
            }
            Self::UnsupportedVersion { found } => {
                write!(f, "unsupported storage version {found}")
            }
            Self::UnsupportedFlags { found } => {
                write!(f, "unsupported storage flags 0b{found:08b}")
            }
            Self::NonZeroReservedHeader { found } => {
                write!(f, "reserved header field must be zero, found {found}")
            }
            Self::MissingSridForNonEmptyPayload => {
                write!(f, "non-empty payloads must store an SRID")
            }
            Self::EmptyEwkb { alpha } => {
                write!(
                    f,
                    "level at alpha {} must store non-empty EWKB bytes",
                    alpha.value()
                )
            }
            Self::TooManyLevels { count } => {
                write!(f, "payload contains {count} levels, which exceeds u32::MAX")
            }
            Self::EwkbTooLarge { alpha, len } => write!(
                f,
                "EWKB payload at alpha {} has length {len}, which exceeds u32::MAX",
                alpha.value()
            ),
            Self::LevelsOutOfOrder { previous, next } => write!(
                f,
                "levels must be stored in strictly descending alpha order, but {} was followed by {}",
                previous.value(),
                next.value()
            ),
            Self::DuplicateAlpha { alpha } => {
                write!(f, "duplicate alpha {} in payload", alpha.value())
            }
            Self::TrailingBytes { remaining } => {
                write!(f, "payload has {remaining} trailing bytes")
            }
            Self::InvalidAlpha(error) => error.fmt(f),
        }
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidAlpha(error) => Some(error),
            Self::Truncated { .. }
            | Self::InvalidMagic { .. }
            | Self::UnsupportedVersion { .. }
            | Self::UnsupportedFlags { .. }
            | Self::NonZeroReservedHeader { .. }
            | Self::MissingSridForNonEmptyPayload
            | Self::EmptyEwkb { .. }
            | Self::TooManyLevels { .. }
            | Self::EwkbTooLarge { .. }
            | Self::LevelsOutOfOrder { .. }
            | Self::DuplicateAlpha { .. }
            | Self::TrailingBytes { .. } => None,
        }
    }
}

/// One persisted fuzzyregion level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredLevel {
    alpha: Alpha,
    geometry_ewkb: Vec<u8>,
}

impl StoredLevel {
    /// Creates a stored level from a validated alpha and its EWKB bytes.
    pub fn new(alpha: Alpha, geometry_ewkb: Vec<u8>) -> Result<Self, StorageError> {
        if geometry_ewkb.is_empty() {
            return Err(StorageError::EmptyEwkb { alpha });
        }

        if geometry_ewkb.len() > u32::MAX as usize {
            return Err(StorageError::EwkbTooLarge {
                alpha,
                len: geometry_ewkb.len(),
            });
        }

        Ok(Self {
            alpha,
            geometry_ewkb,
        })
    }

    /// Returns the level alpha.
    pub fn alpha(&self) -> Alpha {
        self.alpha
    }

    /// Returns the stored EWKB bytes for this level geometry.
    pub fn geometry_ewkb(&self) -> &[u8] {
        &self.geometry_ewkb
    }

    /// Consumes the level and returns its EWKB bytes.
    pub fn into_geometry_ewkb(self) -> Vec<u8> {
        self.geometry_ewkb
    }
}

/// The versioned body payload stored by PostgreSQL for a `fuzzyregion` value.
///
/// This struct models the exact persisted contract, excluding PostgreSQL's own
/// varlena header. A non-empty payload must carry an SRID. The empty value may
/// carry either a concrete SRID or no SRID at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredFuzzyregion {
    version: StorageVersion,
    srid: Option<i32>,
    levels: Vec<StoredLevel>,
}

impl StoredFuzzyregion {
    /// Creates an empty persisted fuzzyregion.
    pub fn empty(srid: Option<i32>) -> Self {
        Self {
            version: CURRENT_STORAGE_VERSION,
            srid,
            levels: Vec::new(),
        }
    }

    /// Creates a stored fuzzyregion using the current storage version.
    pub fn new(srid: Option<i32>, levels: Vec<StoredLevel>) -> Result<Self, StorageError> {
        Self::with_version(CURRENT_STORAGE_VERSION, srid, levels)
    }

    /// Creates a stored fuzzyregion using an explicit payload version.
    pub fn with_version(
        version: StorageVersion,
        srid: Option<i32>,
        levels: Vec<StoredLevel>,
    ) -> Result<Self, StorageError> {
        validate_levels(srid, &levels)?;

        Ok(Self {
            version,
            srid,
            levels,
        })
    }

    /// Decodes a persisted fuzzyregion body from bytes.
    pub fn decode_body(bytes: &[u8]) -> Result<Self, StorageError> {
        let mut reader = BodyReader::new(bytes);

        let magic = reader.read_array::<4>()?;
        if magic != STORAGE_MAGIC {
            return Err(StorageError::InvalidMagic { found: magic });
        }

        let version_byte = reader.read_u8()?;
        let version = match version_byte {
            1 => StorageVersion::V1,
            _ => {
                return Err(StorageError::UnsupportedVersion {
                    found: version_byte,
                });
            }
        };

        let flags = reader.read_u8()?;
        if flags & !SUPPORTED_FLAGS_MASK != 0 {
            return Err(StorageError::UnsupportedFlags { found: flags });
        }

        let reserved = reader.read_u16()?;
        if reserved != RESERVED_HEADER {
            return Err(StorageError::NonZeroReservedHeader { found: reserved });
        }

        let level_count = reader.read_u32()? as usize;
        let srid = if flags & FLAG_HAS_SRID != 0 {
            Some(reader.read_i32()?)
        } else {
            None
        };

        let mut levels = Vec::with_capacity(level_count);
        for _ in 0..level_count {
            let alpha = Alpha::try_from(reader.read_f64()?)?;
            let ewkb_len = reader.read_u32()? as usize;
            let ewkb = reader.read_vec(ewkb_len)?;
            levels.push(StoredLevel::new(alpha, ewkb)?);
        }

        if !reader.is_finished() {
            return Err(StorageError::TrailingBytes {
                remaining: reader.remaining(),
            });
        }

        Self::with_version(version, srid, levels)
    }

    /// Encodes the payload body to bytes.
    pub fn encode_body(&self) -> Vec<u8> {
        let srid_len = if self.srid.is_some() {
            OPTIONAL_SRID_LEN
        } else {
            0
        };
        let levels_len: usize = self
            .levels
            .iter()
            .map(|level| LEVEL_RECORD_HEADER_LEN + level.geometry_ewkb.len())
            .sum();
        let mut bytes = Vec::with_capacity(BASE_HEADER_LEN + srid_len + levels_len);

        bytes.extend_from_slice(&STORAGE_MAGIC);
        bytes.push(self.version.as_u8());
        bytes.push(self.flags());
        bytes.extend_from_slice(&RESERVED_HEADER.to_be_bytes());
        bytes.extend_from_slice(&(self.levels.len() as u32).to_be_bytes());

        if let Some(srid) = self.srid {
            bytes.extend_from_slice(&srid.to_be_bytes());
        }

        for level in &self.levels {
            bytes.extend_from_slice(&level.alpha.value().to_be_bytes());
            bytes.extend_from_slice(&(level.geometry_ewkb.len() as u32).to_be_bytes());
            bytes.extend_from_slice(&level.geometry_ewkb);
        }

        bytes
    }

    /// Returns the payload version.
    pub fn version(&self) -> StorageVersion {
        self.version
    }

    /// Returns the stored SRID, if any.
    pub fn srid(&self) -> Option<i32> {
        self.srid
    }

    /// Returns `true` when the payload stores no levels.
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    /// Returns the stored levels in canonical order.
    pub fn levels(&self) -> &[StoredLevel] {
        &self.levels
    }

    fn flags(&self) -> u8 {
        let mut flags = 0u8;
        if self.srid.is_some() {
            flags |= FLAG_HAS_SRID;
        }
        flags
    }
}

fn validate_levels(srid: Option<i32>, levels: &[StoredLevel]) -> Result<(), StorageError> {
    if !levels.is_empty() && srid.is_none() {
        return Err(StorageError::MissingSridForNonEmptyPayload);
    }

    if levels.len() > u32::MAX as usize {
        return Err(StorageError::TooManyLevels {
            count: levels.len(),
        });
    }

    for pair in levels.windows(2) {
        let previous = pair[0].alpha();
        let next = pair[1].alpha();

        if previous == next {
            return Err(StorageError::DuplicateAlpha { alpha: previous });
        }

        if previous < next {
            return Err(StorageError::LevelsOutOfOrder { previous, next });
        }
    }

    Ok(())
}

struct BodyReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> BodyReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn is_finished(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], StorageError> {
        let end = self.offset + N;
        let slice = self
            .bytes
            .get(self.offset..end)
            .ok_or(StorageError::Truncated {
                offset: self.offset,
                needed: N,
                remaining: self.remaining(),
            })?;
        self.offset = end;
        let mut array = [0u8; N];
        array.copy_from_slice(slice);
        Ok(array)
    }

    fn read_vec(&mut self, len: usize) -> Result<Vec<u8>, StorageError> {
        let end = self.offset + len;
        let slice = self
            .bytes
            .get(self.offset..end)
            .ok_or(StorageError::Truncated {
                offset: self.offset,
                needed: len,
                remaining: self.remaining(),
            })?;
        self.offset = end;
        Ok(slice.to_vec())
    }

    fn read_u8(&mut self) -> Result<u8, StorageError> {
        Ok(self.read_array::<1>()?[0])
    }

    fn read_u16(&mut self) -> Result<u16, StorageError> {
        Ok(u16::from_be_bytes(self.read_array::<2>()?))
    }

    fn read_u32(&mut self) -> Result<u32, StorageError> {
        Ok(u32::from_be_bytes(self.read_array::<4>()?))
    }

    fn read_i32(&mut self) -> Result<i32, StorageError> {
        Ok(i32::from_be_bytes(self.read_array::<4>()?))
    }

    fn read_f64(&mut self) -> Result<f64, StorageError> {
        Ok(f64::from_be_bytes(self.read_array::<8>()?))
    }
}

#[cfg(test)]
mod tests {
    use fuzzyregion_core::Alpha;

    use super::{
        CURRENT_STORAGE_VERSION, STORAGE_MAGIC, StorageError, StoredFuzzyregion, StoredLevel,
    };

    fn alpha(value: f64) -> Alpha {
        Alpha::try_from(value).unwrap()
    }

    fn level(alpha_value: f64, ewkb: &[u8]) -> StoredLevel {
        StoredLevel::new(alpha(alpha_value), ewkb.to_vec()).unwrap()
    }

    #[test]
    fn non_empty_payload_requires_srid() {
        let error = StoredFuzzyregion::new(None, vec![level(0.5, &[1, 2, 3])]).unwrap_err();
        assert_eq!(error, StorageError::MissingSridForNonEmptyPayload);
    }

    #[test]
    fn payload_round_trips_through_binary_encoding() {
        let payload = StoredFuzzyregion::new(
            Some(4326),
            vec![level(1.0, &[1, 2]), level(0.4, &[3, 4, 5])],
        )
        .unwrap();

        let decoded = StoredFuzzyregion::decode_body(&payload.encode_body()).unwrap();

        assert_eq!(decoded, payload);
        assert_eq!(decoded.version(), CURRENT_STORAGE_VERSION);
        assert_eq!(decoded.srid(), Some(4326));
    }

    #[test]
    fn empty_payload_without_srid_round_trips() {
        let payload = StoredFuzzyregion::empty(None);
        let decoded = StoredFuzzyregion::decode_body(&payload.encode_body()).unwrap();

        assert_eq!(decoded, payload);
        assert!(decoded.is_empty());
        assert_eq!(decoded.srid(), None);
    }

    #[test]
    fn decode_rejects_invalid_magic() {
        let payload = StoredFuzzyregion::empty(None).encode_body();
        let mut corrupted = payload.clone();
        corrupted[..4].copy_from_slice(b"NOPE");

        let error = StoredFuzzyregion::decode_body(&corrupted).unwrap_err();
        assert_eq!(error, StorageError::InvalidMagic { found: *b"NOPE" });
    }

    #[test]
    fn decode_rejects_out_of_order_levels() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&STORAGE_MAGIC);
        bytes.push(1);
        bytes.push(0b0000_0001);
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&4326i32.to_be_bytes());
        bytes.extend_from_slice(&0.4f64.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&1.0f64.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.push(2);

        let error = StoredFuzzyregion::decode_body(&bytes).unwrap_err();
        assert_eq!(
            error,
            StorageError::LevelsOutOfOrder {
                previous: alpha(0.4),
                next: alpha(1.0),
            }
        );
    }
}

//! Compact on-disk encoding shared by the native caches.
//!
//! These files are written once and read whole: nothing queries part of one, so the only
//! thing their format has to be good at is being small and parsing fast. Deflating the JSON
//! is what does that. Measured over a full shard set, the evidence rows compress to about a
//! seventh of their plain size, which beats hand-rolling a binary layout for them and leaves
//! one encoding to reason about rather than two.
//!
//! Reading accepts a plain JSON file as well, so a cache an older build left behind is still
//! read rather than rebuilt, and a file this build writes stays readable if the compression
//! is ever dropped.
use std::io::{Read, Write};

use serde::{Serialize, de::DeserializeOwned};

/// Cheap and fast rather than small: these files are read far more often than written, and
/// the difference between this and the slowest setting is a few percent of their size.
const LEVEL: flate2::Compression = flate2::Compression::fast();

/// The two bytes every deflate stream in gzip framing starts with.
const GZIP_MAGIC: [u8; 2] = [0x1F, 0x8B];

/// Writes `value` as deflated JSON. The writer is wrapped so the encoder's output reaches the
/// file in whole blocks instead of one write per token.
pub(crate) fn write<W: Write, T: Serialize>(writer: W, value: &T) -> Result<(), String> {
    let mut encoder = flate2::write::GzEncoder::new(std::io::BufWriter::new(writer), LEVEL);
    serde_json::to_writer(&mut encoder, value).map_err(|error| error.to_string())?;
    encoder
        .finish()
        .map_err(|error| error.to_string())?
        .flush()
        .map_err(|error| error.to_string())
}

/// Reads a file written by [`write`], or a plain JSON one from a build that predates it.
pub(crate) fn read<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    if !bytes.starts_with(&GZIP_MAGIC) {
        return serde_json::from_slice(bytes).map_err(|error| error.to_string());
    }
    let mut plain = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut plain)
        .map_err(|error| error.to_string())?;
    serde_json::from_slice(&plain).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Serialize, serde::Deserialize)]
    struct Sample {
        name: String,
        rows: Vec<u32>,
    }

    /// Shaped like what these files actually hold: many small repeating numbers, because the
    /// rows they carry are delta coded before they get here.
    fn sample() -> Sample {
        Sample {
            name: "shard".into(),
            rows: (0..5_000).map(|row: u32| row % 64).collect(),
        }
    }

    #[test]
    fn values_round_trip_and_get_smaller() {
        let value = sample();
        let mut written = Vec::new();
        write(&mut written, &value).unwrap();
        assert_eq!(read::<Sample>(&written).unwrap(), value);
        let plain = serde_json::to_vec(&value).unwrap();
        assert!(
            written.len() * 4 < plain.len(),
            "{} bytes is no better than {} plain",
            written.len(),
            plain.len()
        );
    }

    /// A cache an older build wrote is read rather than treated as missing, so switching to
    /// this encoding does not cost one rebuild of every index.
    #[test]
    fn plain_json_from_an_older_build_is_still_read() {
        let value = sample();
        let plain = serde_json::to_vec(&value).unwrap();
        assert_eq!(read::<Sample>(&plain).unwrap(), value);
    }

    #[test]
    fn a_truncated_or_corrupt_file_is_an_error_not_a_panic() {
        let mut written = Vec::new();
        write(&mut written, &sample()).unwrap();
        written.truncate(written.len() / 2);
        assert!(read::<Sample>(&written).is_err());
        assert!(read::<Sample>(&GZIP_MAGIC).is_err());
        assert!(read::<Sample>(b"{ not json").is_err());
        assert!(read::<Sample>(&[]).is_err());
    }
}

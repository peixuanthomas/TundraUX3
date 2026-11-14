use super::Cell;
use base64::{Engine, display::Base64Display, engine::general_purpose::STANDARD};
use flate2::{Compression, bufread::GzDecoder, write::GzEncoder};
use serde::de::{Error, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serializer};
use std::fmt;
use std::io::{Read, Write};

const MAX_CELLS: usize = 256 * 1024;
const MAX_CELL_BYTES: usize = 16 * 1024 * 1024;

pub(super) fn serialize<S: Serializer>(cells: &[Cell], serializer: S) -> Result<S::Ok, S::Error> {
    use serde::ser::Error;

    if cells.len() > MAX_CELLS {
        return Err(S::Error::custom("too many window cells"));
    }
    let raw = serde_json::to_vec(cells).map_err(S::Error::custom)?;
    if raw.len() > MAX_CELL_BYTES {
        return Err(S::Error::custom("window cell data exceeds limit"));
    }
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&raw).map_err(S::Error::custom)?;
    let compressed = encoder.finish().map_err(S::Error::custom)?;
    serializer.collect_str(&Base64Display::new(&compressed, &STANDARD))
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Cell>, D::Error> {
    let payload = String::deserialize(deserializer)?;
    if payload.len() > 2 * MAX_CELL_BYTES {
        return Err(D::Error::custom(
            "compressed window cell data exceeds limit",
        ));
    }
    let compressed = STANDARD.decode(payload).map_err(D::Error::custom)?;
    let mut decoder = GzDecoder::new(compressed.as_slice());
    let mut raw = Vec::new();
    (&mut decoder)
        .take(MAX_CELL_BYTES as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(D::Error::custom)?;
    if raw.len() > MAX_CELL_BYTES {
        return Err(D::Error::custom("window cell data exceeds limit"));
    }
    if !decoder.into_inner().is_empty() {
        return Err(D::Error::custom(
            "unexpected bytes after compressed window cells",
        ));
    }
    let mut decoder = serde_json::Deserializer::from_slice(&raw);
    let cells = decoder
        .deserialize_seq(CellVisitor)
        .map_err(D::Error::custom)?;
    decoder.end().map_err(D::Error::custom)?;
    Ok(cells)
}

struct CellVisitor;

impl<'de> Visitor<'de> for CellVisitor {
    type Value = Vec<Cell>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("window cells")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut cells = Vec::new();
        while let Some(cell) = seq.next_element()? {
            if cells.len() == MAX_CELLS {
                return Err(A::Error::custom("too many window cells"));
            }
            cells.push(cell);
        }
        Ok(cells)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_expanded_payload_beyond_the_byte_limit() {
        let data = vec![b' '; MAX_CELL_BYTES + 1];
        let error = deserialize_raw(&data).unwrap_err();
        assert!(error.to_string().contains("cell data exceeds limit"));
    }

    #[test]
    fn rejects_cell_count_above_the_limit_even_with_small_json() {
        let mut data = Vec::from(&b"["[..]);
        for _ in 0..MAX_CELLS {
            data.extend_from_slice(b"{},");
        }
        data.extend_from_slice(b"{}]");
        let error = deserialize_raw(&data).unwrap_err();
        assert!(error.to_string().contains("too many window cells"));
    }

    fn deserialize_raw(data: &[u8]) -> Result<Vec<Cell>, serde_json::Error> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(data).unwrap();
        let compressed = STANDARD.encode(encoder.finish().unwrap());
        let json = serde_json::to_string(&compressed).unwrap();
        deserialize(&mut serde_json::Deserializer::from_str(&json))
    }
}

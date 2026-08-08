//! Hex-mode layout: `offset | hex bytes | ASCII gutter`, computed purely by
//! offset math over an already-read byte window — no line index, no
//! per-row state beyond the row's base offset (viewer: Text and hex modes
//! with F4 mode toggle — "Hex layout by offset math").

pub const HEX_BYTES_PER_ROW: usize = 16;

/// One hex-mode row: its offset and its 1..=16 bytes (short only for the
/// file's final row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HexRow {
    pub offset: u64,
    pub bytes: Vec<u8>,
}

impl HexRow {
    /// The row's bytes as two-digit uppercase hex pairs, space-separated,
    /// and padded with blanks so every row's hex field spans the same width
    /// regardless of how many bytes it holds.
    pub fn hex_field(&self) -> String {
        let mut out = String::with_capacity(HEX_BYTES_PER_ROW * 3);
        for i in 0..HEX_BYTES_PER_ROW {
            if i > 0 {
                out.push(' ');
            }
            match self.bytes.get(i) {
                Some(b) => out.push_str(&format!("{b:02X}")),
                None => out.push_str("  "),
            }
        }
        out
    }

    /// The ASCII gutter: each byte as its printable ASCII character, or `.`
    /// for anything outside the printable range.
    pub fn ascii_gutter(&self) -> String {
        self.bytes.iter().map(|&b| if (0x20..=0x7e).contains(&b) { b as char } else { '.' }).collect()
    }
}

/// Lay out `bytes` (already read from [`super::byte_source::ByteSource`]
/// starting at `base`) into 16-byte hex rows. Row `r`'s offset is
/// `base + r*16` and its bytes are `bytes[r*16 .. r*16+16]` (clamped to
/// however many bytes were actually read) — purely a function of `bytes`
/// and `base`, with no state carried between rows.
pub fn hex_rows(bytes: &[u8], base: u64) -> Vec<HexRow> {
    bytes
        .chunks(HEX_BYTES_PER_ROW)
        .enumerate()
        .map(|(i, chunk)| HexRow { offset: base + (i * HEX_BYTES_PER_ROW) as u64, bytes: chunk.to_vec() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_rows_computes_offset_by_pure_math() {
        let bytes: Vec<u8> = (0..40u8).collect();
        let rows = hex_rows(&bytes, 0x1000);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].offset, 0x1000);
        assert_eq!(rows[1].offset, 0x1010);
        assert_eq!(rows[2].offset, 0x1020);
        assert_eq!(rows[0].bytes, (0..16u8).collect::<Vec<u8>>());
        assert_eq!(rows[1].bytes, (16..32u8).collect::<Vec<u8>>());
        assert_eq!(rows[2].bytes, (32..40u8).collect::<Vec<u8>>());
    }

    #[test]
    fn hex_rows_last_row_is_short_without_padding_the_bytes_vec() {
        let bytes: Vec<u8> = (0..20u8).collect();
        let rows = hex_rows(&bytes, 0);
        assert_eq!(rows[1].bytes.len(), 4);
    }

    #[test]
    fn hex_field_formats_uppercase_pairs_and_pads_short_rows() {
        let row = HexRow { offset: 0, bytes: vec![0x00, 0xff, 0x0a] };
        let field = row.hex_field();
        assert!(field.starts_with("00 FF 0A"));
        // Padded out to 16 slots' worth of "XX " tokens.
        assert_eq!(field.len(), HEX_BYTES_PER_ROW * 3 - 1);
    }

    #[test]
    fn ascii_gutter_replaces_non_printable_bytes_with_dot() {
        let row = HexRow { offset: 0, bytes: vec![b'A', 0x00, b'z', 0x7f, b' '] };
        assert_eq!(row.ascii_gutter(), "A.z. ");
    }

    #[test]
    fn empty_window_yields_no_rows() {
        assert!(hex_rows(&[], 42).is_empty());
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn every_row_has_the_expected_offset_and_byte_slice(bytes in prop::collection::vec(any::<u8>(), 0..300), base in 0u64..1_000_000) {
                let rows = hex_rows(&bytes, base);
                for (i, row) in rows.iter().enumerate() {
                    prop_assert_eq!(row.offset, base + (i * HEX_BYTES_PER_ROW) as u64);
                    let start = i * HEX_BYTES_PER_ROW;
                    let end = (start + HEX_BYTES_PER_ROW).min(bytes.len());
                    prop_assert_eq!(&row.bytes[..], &bytes[start..end]);
                }
                let total: usize = rows.iter().map(|r| r.bytes.len()).sum();
                prop_assert_eq!(total, bytes.len());
            }
        }
    }
}

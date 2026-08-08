//! The Info display mode's content model.
//!
//! Every value that needs a drive or directory query is an `Option`: `None`
//! renders as the static `…` placeholder and is replaced in place when the
//! worker thread's answer arrives. Nothing here performs a query — the model
//! is a pure projection of already-resolved values, so the renderer stays a
//! pure function of core state.

use crate::listing::format_count;

/// The static placeholder shown for a value whose background query has not
/// resolved. Deliberately a single CP437-heritage glyph: no spinner, no
/// animation.
pub const PENDING: &str = "\u{2026}";

/// Resolved Info values for one panel. Every field starts `None` when Info
/// mode is entered and is filled in as its query completes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InfoValues {
    pub memory_bytes: Option<u64>,
    pub drive_total: Option<u64>,
    pub drive_free: Option<u64>,
    pub volume_label: Option<String>,
    pub serial: Option<String>,
    pub file_count: Option<usize>,
    pub dir_count: Option<usize>,
}

impl InfoValues {
    /// True once every async field has resolved — used by tests and by the
    /// TUI to avoid re-issuing a query that is already satisfied.
    pub fn is_complete(&self) -> bool {
        self.memory_bytes.is_some()
            && self.drive_total.is_some()
            && self.drive_free.is_some()
            && self.volume_label.is_some()
            && self.serial.is_some()
            && self.file_count.is_some()
            && self.dir_count.is_some()
    }
}

/// One labelled row inside an Info box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoField {
    pub label: String,
    pub value: String,
}

/// One single-line-framed box in the stacked Info layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoBox {
    pub title: String,
    pub fields: Vec<InfoField>,
}

fn bytes_field(label: &str, value: Option<u64>) -> InfoField {
    let value = match value {
        Some(n) => format!("{} bytes", format_count(n as usize)),
        None => PENDING.to_string(),
    };
    InfoField { label: label.to_string(), value }
}

fn count_field(label: &str, value: Option<usize>) -> InfoField {
    let value = match value {
        Some(n) => format_count(n),
        None => PENDING.to_string(),
    };
    InfoField { label: label.to_string(), value }
}

fn text_field(label: &str, value: Option<&str>) -> InfoField {
    // A volume with no label resolves to an empty string, which is a real
    // answer — only an unresolved query shows `…`.
    let value = match value {
        Some("") => "(none)".to_string(),
        Some(s) => s.to_string(),
        None => PENDING.to_string(),
    };
    InfoField { label: label.to_string(), value }
}

/// The stacked boxes for a panel in Info mode. `drive` is the panel's
/// current drive letter (`None` for a UNC path, which has no letter).
pub fn info_boxes(values: &InfoValues, drive: Option<char>) -> Vec<InfoBox> {
    let drive_title = match drive {
        Some(letter) => format!("Drive {}:", letter.to_ascii_uppercase()),
        None => "Drive".to_string(),
    };
    vec![
        InfoBox { title: "System".to_string(), fields: vec![bytes_field("Memory free", values.memory_bytes)] },
        InfoBox {
            title: drive_title,
            fields: vec![
                text_field("Volume label", values.volume_label.as_deref()),
                text_field("Serial number", values.serial.as_deref()),
                bytes_field("Total space", values.drive_total),
                bytes_field("Free space", values.drive_free),
            ],
        },
        InfoBox {
            title: "Directory".to_string(),
            fields: vec![count_field("Files", values.file_count), count_field("Directories", values.dir_count)],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_async_value_starts_as_the_pending_glyph() {
        let boxes = info_boxes(&InfoValues::default(), Some('C'));
        let values: Vec<&str> = boxes.iter().flat_map(|b| b.fields.iter()).map(|f| f.value.as_str()).collect();
        assert!(values.iter().all(|v| *v == PENDING), "unresolved fields render as `…`, got {values:?}");
        assert_eq!(values.len(), 7, "memory, label, serial, total, free, files, dirs");
    }

    #[test]
    fn content_set_covers_every_required_field() {
        let boxes = info_boxes(&InfoValues::default(), Some('C'));
        let labels: Vec<&str> = boxes.iter().flat_map(|b| b.fields.iter()).map(|f| f.label.as_str()).collect();
        for expected in ["Memory free", "Volume label", "Serial number", "Total space", "Free space", "Files", "Directories"] {
            assert!(labels.contains(&expected), "missing `{expected}` in {labels:?}");
        }
    }

    #[test]
    fn resolved_values_replace_the_placeholder_in_place() {
        let mut values = InfoValues::default();
        values.drive_total = Some(1_234_567);
        let boxes = info_boxes(&values, Some('C'));
        let drive_box = &boxes[1];
        assert_eq!(drive_box.fields[2].value, "1,234,567 bytes");
        // Field positions and the other placeholders are untouched.
        assert_eq!(drive_box.fields[0].label, "Volume label");
        assert_eq!(drive_box.fields[3].value, PENDING);
    }

    #[test]
    fn empty_volume_label_is_a_resolved_answer_not_a_placeholder() {
        let values = InfoValues { volume_label: Some(String::new()), ..InfoValues::default() };
        let boxes = info_boxes(&values, Some('C'));
        assert_eq!(boxes[1].fields[0].value, "(none)");
    }

    #[test]
    fn unc_path_without_a_drive_letter_still_titles_the_drive_box() {
        let boxes = info_boxes(&InfoValues::default(), None);
        assert_eq!(boxes[1].title, "Drive");
    }

    #[test]
    fn is_complete_only_once_every_field_resolved() {
        let mut values = InfoValues::default();
        assert!(!values.is_complete());
        values = InfoValues {
            memory_bytes: Some(1),
            drive_total: Some(2),
            drive_free: Some(3),
            volume_label: Some("OS".into()),
            serial: Some("0000-0000".into()),
            file_count: Some(4),
            dir_count: Some(5),
        };
        assert!(values.is_complete());
    }
}

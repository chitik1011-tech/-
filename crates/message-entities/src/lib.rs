//! Telegram formatted text and message entities.
//!
//! Entity offsets and lengths use Telegram's UTF-16 code-unit convention. They
//! are validated against the text and can be converted to UTF-8 byte ranges
//! only when both endpoints fall on Unicode scalar boundaries. This crate
//! models entities supplied by the backend; it does not infer entities from
//! message text.

use serde::{de, Deserialize, Deserializer, Serialize};
use std::ops::Range;
use thiserror::Error;

/// A supported Telegram message entity type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EntityKind {
    Bold,
    Italic,
    Underline,
    Strikethrough,
    Code,
    Pre { language: String },
    TextUrl { url: String },
    Url,
    Email,
    Mention,
    Hashtag,
    BotCommand,
    Spoiler,
}

/// A Telegram entity range, measured in UTF-16 code units.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TextEntity {
    offset: u32,
    length: u32,
    #[serde(flatten)]
    kind: EntityKind,
}

impl<'de> Deserialize<'de> for TextEntity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WireTextEntity {
            offset: u32,
            length: u32,
            #[serde(flatten)]
            kind: EntityKind,
        }

        let wire = WireTextEntity::deserialize(deserializer)?;
        Self::new(wire.offset, wire.length, wire.kind).map_err(de::Error::custom)
    }
}

impl TextEntity {
    /// Creates an entity with a positive UTF-16 length and non-overflowing end.
    pub fn new(offset: u32, length: u32, kind: EntityKind) -> Result<Self, EntityError> {
        if length == 0 {
            return Err(EntityError::EmptyRange);
        }
        offset.checked_add(length).ok_or(EntityError::OffsetOverflow)?;
        Ok(Self {
            offset,
            length,
            kind,
        })
    }

    /// Returns the UTF-16 code-unit offset from the start of the text.
    pub fn offset(&self) -> u32 {
        self.offset
    }

    /// Returns the entity length in UTF-16 code units.
    pub fn length(&self) -> u32 {
        self.length
    }

    /// Returns the entity's formatting or semantic type.
    pub fn kind(&self) -> &EntityKind {
        &self.kind
    }

    /// Converts this entity's validated UTF-16 range to a UTF-8 byte range.
    pub fn byte_range(&self, text: &str) -> Result<Range<usize>, EntityError> {
        let end = self
            .offset
            .checked_add(self.length)
            .ok_or(EntityError::OffsetOverflow)?;
        let start_byte = byte_index_at_utf16_offset(text, self.offset)?;
        let end_byte = byte_index_at_utf16_offset(text, end)?;
        Ok(start_byte..end_byte)
    }
}

/// Formatted text with backend-provided Telegram entities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FormattedText {
    text: String,
    entities: Vec<TextEntity>,
}

impl FormattedText {
    /// Validates entity ranges against text using UTF-16 code-unit offsets.
    ///
    /// Entities may overlap. Every entity must have a positive length, remain
    /// within the text, and begin and end on Unicode scalar boundaries.
    pub fn new(text: String, entities: Vec<TextEntity>) -> Result<Self, EntityError> {
        let utf16_length = text.encode_utf16().count();
        for entity in &entities {
            if entity.length == 0 {
                return Err(EntityError::EmptyRange);
            }
            let end = entity
                .offset
                .checked_add(entity.length)
                .ok_or(EntityError::OffsetOverflow)?;
            if end as u64 > utf16_length as u64 {
                return Err(EntityError::OutOfBounds {
                    offset: entity.offset,
                    length: entity.length,
                    text_utf16_length: utf16_length,
                });
            }
            byte_index_at_utf16_offset(&text, entity.offset)?;
            byte_index_at_utf16_offset(&text, end)?;
        }
        Ok(Self { text, entities })
    }

    /// Returns the unmodified message text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the backend-provided entities in their original order.
    pub fn entities(&self) -> &[TextEntity] {
        &self.entities
    }

    /// Returns a validated entity's UTF-8 byte range.
    pub fn entity_byte_range(&self, index: usize) -> Result<Range<usize>, EntityError> {
        let entity = self
            .entities
            .get(index)
            .ok_or(EntityError::EntityIndexOutOfBounds { index })?;
        entity.byte_range(&self.text)
    }
}

impl<'de> Deserialize<'de> for FormattedText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WireFormattedText {
            text: String,
            entities: Vec<TextEntity>,
        }

        let wire = WireFormattedText::deserialize(deserializer)?;
        Self::new(wire.text, wire.entities).map_err(de::Error::custom)
    }
}

/// Errors returned when entity ranges cannot be safely applied to a string.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EntityError {
    #[error("entity length must be greater than zero")]
    EmptyRange,
    #[error("entity UTF-16 offset plus length overflows u32")]
    OffsetOverflow,
    #[error("entity at UTF-16 offset {offset} with length {length} exceeds text length {text_utf16_length}")]
    OutOfBounds {
        offset: u32,
        length: u32,
        text_utf16_length: usize,
    },
    #[error("UTF-16 offset {offset} falls inside a Unicode scalar value")]
    SplitsUnicodeScalar { offset: u32 },
    #[error("entity index {index} is out of bounds")]
    EntityIndexOutOfBounds { index: usize },
}

/// Maps a UTF-16 code-unit offset to a UTF-8 byte index without splitting a
/// Unicode scalar value (including a supplementary-plane emoji).
fn byte_index_at_utf16_offset(text: &str, target: u32) -> Result<usize, EntityError> {
    let mut utf16_index = 0_usize;
    let target_index = target as usize;

    for (byte_index, ch) in text.char_indices() {
        if utf16_index == target_index {
            return Ok(byte_index);
        }

        let next = utf16_index + ch.len_utf16();
        if target_index < next {
            return Err(EntityError::SplitsUnicodeScalar { offset: target });
        }
        utf16_index = next;
    }

    if utf16_index == target_index {
        Ok(text.len())
    } else {
        Err(EntityError::OutOfBounds {
            offset: target,
            length: 0,
            text_utf16_length: utf16_index,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telegram_utf16_offsets_convert_to_utf8_ranges() {
        let text = "A😀éZ".to_owned();
        // UTF-8 bytes: A=1, 😀=4, é=2, Z=1. UTF-16 units: 1, 2, 1, 1.
        let entities = vec![
            TextEntity::new(1, 2, EntityKind::Bold).expect("valid test entity"),
            TextEntity::new(3, 1, EntityKind::Italic).expect("valid test entity"),
        ];
        let formatted = FormattedText::new(text, entities).expect("valid formatted text");

        assert_eq!(formatted.entity_byte_range(0), Ok(1..5));
        assert_eq!(formatted.entity_byte_range(1), Ok(5..7));
        assert_eq!(formatted.text().len(), 8);
    }

    #[test]
    fn rejects_offsets_that_split_surrogate_pair() {
        let emoji = "😀".to_owned();
        let split_start = TextEntity::new(1, 1, EntityKind::Bold).expect("non-overflowing range");
        assert_eq!(
            FormattedText::new(emoji.clone(), vec![split_start]),
            Err(EntityError::SplitsUnicodeScalar { offset: 1 })
        );

        let split_end = TextEntity::new(0, 1, EntityKind::Italic).expect("non-overflowing range");
        assert_eq!(
            FormattedText::new(emoji, vec![split_end]),
            Err(EntityError::SplitsUnicodeScalar { offset: 1 })
        );
    }

    #[test]
    fn rejects_ranges_past_end_and_zero_length() {
        let past_end = TextEntity::new(2, 1, EntityKind::Underline).expect("valid range shape");
        assert!(matches!(
            FormattedText::new("é".to_owned(), vec![past_end]),
            Err(EntityError::OutOfBounds { .. })
        ));
        assert_eq!(
            TextEntity::new(0, 0, EntityKind::Code),
            Err(EntityError::EmptyRange)
        );
    }

    #[test]
    fn wire_serialization_round_trips_all_supported_entity_kinds() {
        let kinds = vec![
            EntityKind::Bold,
            EntityKind::Italic,
            EntityKind::Underline,
            EntityKind::Strikethrough,
            EntityKind::Code,
            EntityKind::Pre {
                language: "rust".to_owned(),
            },
            EntityKind::TextUrl {
                url: "https://example.test/".to_owned(),
            },
            EntityKind::Url,
            EntityKind::Email,
            EntityKind::Mention,
            EntityKind::Hashtag,
            EntityKind::BotCommand,
            EntityKind::Spoiler,
        ];
        let entities = kinds
            .into_iter()
            .enumerate()
            .map(|(offset, kind)| {
                TextEntity::new(offset as u32, 1, kind).expect("valid test entity")
            })
            .collect();
        let formatted =
            FormattedText::new("abcdefghijklm".to_owned(), entities).expect("valid text");

        let json = serde_json::to_string(&formatted).expect("serializes");
        let decoded: FormattedText = serde_json::from_str(&json).expect("valid wire entity data");
        assert_eq!(decoded, formatted);
        assert_eq!(decoded.entities().len(), 13);
        assert!(json.contains("\"type\":\"pre\""));
        assert!(json.contains("\"type\":\"text_url\""));
    }

    #[test]
    fn deserialization_validates_external_ranges() {
        let error = serde_json::from_str::<FormattedText>(
            r#"{"text":"😀","entities":[{"type":"bold","offset":1,"length":1}]}"#,
        )
        .expect_err("mid-surrogate offset must be rejected");
        assert!(error.to_string().contains("Unicode scalar"));

        let empty_error = serde_json::from_str::<TextEntity>(
            r#"{"type":"bold","offset":0,"length":0}"#,
        )
        .expect_err("zero-length entity must be rejected");
        assert!(empty_error.to_string().contains("greater than zero"));
    }

    #[test]
    fn entity_wire_shape_is_flat_and_uses_telegram_field_names() {
        let formatted = FormattedText::new(
            "docs".to_owned(),
            vec![TextEntity::new(
                0,
                4,
                EntityKind::TextUrl {
                    url: "https://example.test".to_owned(),
                },
            )
            .expect("valid entity")],
        )
        .expect("valid formatted text");
        let value = serde_json::to_value(formatted).expect("serializes");
        assert_eq!(
            value,
            serde_json::json!({
                "text": "docs",
                "entities": [{
                    "type": "text_url",
                    "offset": 0,
                    "length": 4,
                    "url": "https://example.test"
                }]
            })
        );
    }
}

//! Edge of Fate item lore references and source-localized lore tabs.
use super::{localization, payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

pub fn for_item(reader: &mut Reader, hash: u32) -> Result<Option<Value>> {
    let (_, tag) = super::assets::item::find(reader, hash)?;
    let item = reader.tag(tag, Some(0x8080799D))?;
    read(reader, &item)
}

fn index(item: &Payload) -> Result<Option<usize>> {
    if item.u64(0x28)? == 0 {
        return Ok(None);
    }
    let block = item.pointer(0x28)?;
    ensure!(
        block >= 4 && item.u32(block - 4)? == 0x808073B6,
        "Unsupported modern item lore block"
    );
    let index = item.u16(block)?;
    Ok((index != u16::MAX).then_some(usize::from(index)))
}

pub fn read(reader: &mut Reader, item: &Payload) -> Result<Option<Value>> {
    let Some(index) = index(item)? else {
        return Ok(None);
    };
    let tags = reader.classes(0x808050CF);
    ensure!(
        tags.len() == 1,
        "Modern lore display table is missing or ambiguous"
    );
    let table = reader.tag(tags[0], Some(0x808050CF))?;
    let rows = table.array(8, 40, Some(0x808050D3))?;
    let row = *rows
        .get(index)
        .context("Modern lore index is outside its display table")?;
    let mut text = |offset| -> Result<Option<String>> {
        Ok(
            match localization::text(
                reader,
                table.u32(row + offset)? as usize,
                table.u32(row + offset + 4)?,
                0,
            )? {
                localization::Text::Found(value) => Some(value),
                localization::Text::Empty | localization::Text::Absent => None,
            },
        )
    };
    Ok(entry(text(12)?, text(20)?, text(28)?, table.u32(row + 8)?))
}

/// A display row can outlive the text it names. Without a description there is
/// no lore tab to retain, which is not a reason to fail the whole import.
fn entry(
    title: Option<String>,
    subtitle: Option<String>,
    description: Option<String>,
    hash: u32,
) -> Option<Value> {
    let description = description?;
    Some(json!({"title":title,"subtitle":subtitle,"text":description,"hash":hash}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_display_row_without_description_text_yields_no_lore_tab() {
        // Trendsetter's row resolves a title and subtitle, but its bank ships
        // no description, so the weapon converts with no lore instead.
        assert_eq!(
            entry(Some("Trendsetter".into()), Some("Lead.".into()), None, 7),
            None
        );
        let value = entry(None, None, Some("text".into()), 7).unwrap();
        assert_eq!(value["text"], "text");
        assert!(value["title"].is_null() && value["hash"] == 7);
    }

    #[test]
    fn lore_references_distinguish_absence_from_invalid_layout() {
        let mut item = Payload(vec![0; 96]);
        assert_eq!(index(&item).unwrap(), None);
        item.0[0x28..0x30].copy_from_slice(&24i64.to_le_bytes());
        assert!(index(&item).is_err());
        item.0[60..64].copy_from_slice(&0x808073B6u32.to_le_bytes());
        item.0[64..66].copy_from_slice(&17u16.to_le_bytes());
        assert_eq!(index(&item).unwrap(), Some(17));
        item.0[64..66].copy_from_slice(&u16::MAX.to_le_bytes());
        assert_eq!(index(&item).unwrap(), None);
    }
}

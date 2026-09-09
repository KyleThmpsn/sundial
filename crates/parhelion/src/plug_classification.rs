//! Private plug classification, independent of the plug's finished perks and runtime.
use sundial::package_authoring::investment_schema::{
    ITEM_PLUG_BLOCK_CATEGORY_OFFSET, ITEM_PLUG_BLOCK_CLASS, ITEM_PLUG_BLOCK_SEARCH_END,
    ITEM_PLUG_BLOCK_SEARCH_START, ITEM_RARITY_OFFSET, ITEM_STRING_TYPE_REFERENCE_OFFSET,
    ITEM_STRING_UI_TEMPLATE_HASH_OFFSET,
};

use crate::{
    AuthoringResult,
    error::invalid,
    tag_payload::{read_u32, write_u32},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PlugClassification {
    pub category: u32,
    rarity: u8,
    type_reference: [u8; 8],
    ui_template_hash: u32,
}

fn category_offset(definition: &[u8]) -> AuthoringResult<usize> {
    let end = definition.len().min(ITEM_PLUG_BLOCK_SEARCH_END);
    let mut matches = (ITEM_PLUG_BLOCK_SEARCH_START..end.saturating_sub(3))
        .step_by(4)
        .filter(|&offset| read_u32(definition, offset).ok() == Some(ITEM_PLUG_BLOCK_CLASS));
    let block = matches
        .next()
        .ok_or_else(|| invalid("Plug classification requires a typed native plug block"))?;
    if matches.next().is_some() {
        return Err(invalid(
            "Plug classification has more than one native plug block",
        ));
    }
    let offset = block + ITEM_PLUG_BLOCK_CATEGORY_OFFSET;
    read_u32(definition, offset)?;
    Ok(offset)
}

impl PlugClassification {
    pub(crate) fn category(definition: &[u8]) -> AuthoringResult<u32> {
        read_u32(definition, category_offset(definition)?)
    }

    pub fn from_template(definition: &[u8], strings: &[u8]) -> AuthoringResult<Self> {
        let category = Self::category(definition)?;
        let rarity = *definition
            .get(ITEM_RARITY_OFFSET)
            .filter(|&&rarity| (1..=6).contains(&rarity))
            .ok_or_else(|| invalid("Classification source has no supported native plug rarity"))?;
        if matches!(category, 0 | u32::MAX) {
            return Err(invalid(
                "Classification source has no active native plug category",
            ));
        }
        let type_reference = strings
            .get(ITEM_STRING_TYPE_REFERENCE_OFFSET..ITEM_STRING_TYPE_REFERENCE_OFFSET + 8)
            .ok_or_else(|| invalid("Classification source has no item-type string reference"))?
            .try_into()
            .expect("bounded eight-byte reference");
        let result = Self {
            category,
            rarity,
            type_reference,
            ui_template_hash: read_u32(strings, ITEM_STRING_UI_TEMPLATE_HASH_OFFSET)?,
        };
        if read_u32(&result.type_reference, 0)? == u32::MAX
            || matches!(read_u32(&result.type_reference, 4)?, 0 | u32::MAX)
        {
            return Err(invalid(
                "Classification source has an inactive item-type string reference",
            ));
        }
        Ok(result)
    }

    pub fn apply(self, definition: &mut [u8], strings: &mut [u8]) -> AuthoringResult<()> {
        // Validate every destination before writing either payload. No guessed fallback offsets.
        let category = category_offset(definition)?;
        definition
            .get(ITEM_RARITY_OFFSET)
            .ok_or_else(|| invalid("Private plug has no native rarity field"))?;
        strings
            .get(ITEM_STRING_TYPE_REFERENCE_OFFSET..ITEM_STRING_TYPE_REFERENCE_OFFSET + 8)
            .ok_or_else(|| invalid("Private plug has no item-type string reference"))?;
        read_u32(strings, ITEM_STRING_UI_TEMPLATE_HASH_OFFSET)?;
        write_u32(definition, category, self.category)?;
        // Stock intrinsic plugs carry their own tier independently of the containing weapon.
        // Reclassifying a Rare trait must not leave its native tier inconsistent with its donor.
        definition[ITEM_RARITY_OFFSET] = self.rarity;
        strings[ITEM_STRING_TYPE_REFERENCE_OFFSET..ITEM_STRING_TYPE_REFERENCE_OFFSET + 8]
            .copy_from_slice(&self.type_reference);
        // The inspection list selects its child template from this hash. A trait's empty
        // hash cannot render the intrinsic even when category and localized type match.
        write_u32(
            strings,
            ITEM_STRING_UI_TEMPLATE_HASH_OFFSET,
            self.ui_template_hash,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(category: u32) -> (Vec<u8>, Vec<u8>) {
        let mut definition = vec![0xAA; 0x220];
        write_u32(&mut definition, 0x184, ITEM_PLUG_BLOCK_CLASS).unwrap();
        write_u32(&mut definition, 0x188, category).unwrap();
        definition[ITEM_RARITY_OFFSET] = 5;
        let mut strings = vec![0xBB; 0x150];
        write_u32(&mut strings, ITEM_STRING_TYPE_REFERENCE_OFFSET, 3).unwrap();
        write_u32(&mut strings, ITEM_STRING_TYPE_REFERENCE_OFFSET + 4, 0x1234).unwrap();
        write_u32(
            &mut strings,
            ITEM_STRING_UI_TEMPLATE_HASH_OFFSET,
            0x7B20_E35D,
        )
        .unwrap();
        (definition, strings)
    }

    #[test]
    fn copies_native_plug_classification_without_changing_runtime() {
        let (source, source_strings) = template(0x67FB_A961);
        let classification = PlugClassification::from_template(&source, &source_strings).unwrap();
        let (mut target, mut strings) = template(0x0078_A617);
        target[ITEM_RARITY_OFFSET] = 4;
        write_u32(&mut strings, ITEM_STRING_TYPE_REFERENCE_OFFSET + 4, 0x5678).unwrap();
        write_u32(
            &mut strings,
            ITEM_STRING_UI_TEMPLATE_HASH_OFFSET,
            0x811C_9DC5,
        )
        .unwrap();
        let before = (target.clone(), strings.clone());
        classification.apply(&mut target, &mut strings).unwrap();
        assert_eq!(
            PlugClassification::from_template(&target, &strings).unwrap(),
            classification
        );
        write_u32(&mut target, 0x188, 0x0078_A617).unwrap();
        target[ITEM_RARITY_OFFSET] = 4;
        write_u32(&mut strings, ITEM_STRING_TYPE_REFERENCE_OFFSET + 4, 0x5678).unwrap();
        write_u32(
            &mut strings,
            ITEM_STRING_UI_TEMPLATE_HASH_OFFSET,
            0x811C_9DC5,
        )
        .unwrap();
        assert_eq!((target, strings), before);
    }

    #[test]
    fn trait_classification_can_clear_an_intrinsic_ui_template() {
        let (source, mut source_strings) = template(0x0078_A617);
        write_u32(
            &mut source_strings,
            ITEM_STRING_UI_TEMPLATE_HASH_OFFSET,
            0x811C_9DC5,
        )
        .unwrap();
        let classification = PlugClassification::from_template(&source, &source_strings).unwrap();
        let (mut target, mut strings) = template(0x67FB_A961);
        classification.apply(&mut target, &mut strings).unwrap();
        assert_eq!(
            read_u32(&strings, ITEM_STRING_UI_TEMPLATE_HASH_OFFSET).unwrap(),
            0x811C_9DC5
        );
    }

    #[test]
    fn rejects_missing_ambiguous_and_inactive_native_categories() {
        let (mut definition, strings) = template(0);
        assert!(PlugClassification::from_template(&definition, &strings).is_err());
        write_u32(&mut definition, 0x188, 123).unwrap();
        write_u32(&mut definition, 0x1C4, ITEM_PLUG_BLOCK_CLASS).unwrap();
        assert!(PlugClassification::from_template(&definition, &strings).is_err());
        definition.fill(0);
        assert!(PlugClassification::from_template(&definition, &strings).is_err());
    }

    #[test]
    fn malformed_destination_never_partially_changes_native_category() {
        let (mut definition, strings) = template(123);
        let classification = PlugClassification::from_template(&definition, &strings).unwrap();
        let before = definition.clone();
        assert!(classification.apply(&mut definition, &mut []).is_err());
        assert_eq!(definition, before);
        let mut truncated_strings = strings[..ITEM_STRING_UI_TEMPLATE_HASH_OFFSET + 3].to_vec();
        let strings_before = truncated_strings.clone();
        assert!(
            classification
                .apply(&mut definition, &mut truncated_strings)
                .is_err()
        );
        assert_eq!(definition, before);
        assert_eq!(truncated_strings, strings_before);
    }
}

//! Fit stock Sparrow traits to a private equipped plug without changing stock actions.
use crate::{
    AuthoringResult, ItemKind, WeaponCloneSpec, WeaponSocketColumnOverride,
    WeaponSocketPlugVariantOverride,
    error::invalid,
    tag_payload::{array_at, read_u16, read_u32, relative_target},
};
use std::collections::BTreeMap;
use sundial::package_authoring::investment_schema::{
    ITEM_INDEX_ROW_SIZE, ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET,
    ITEM_ORDINARY_SOCKET_POINTER_OFFSET, ITEM_ORDINARY_SOCKET_ROW_CLASS,
    ITEM_ORDINARY_SOCKET_ROW_SIZE,
};
use sundial::package_authoring::{PackageManager, native_weapon::base_sandbox_perks};
use tiger_pkg::TagHash;

pub(crate) fn expand(
    manager: &PackageManager,
    spec: &WeaponCloneSpec,
    definition: &[u8],
    item_table: &[u8],
    item_rows: usize,
    item_rows_by_hash: &BTreeMap<u32, Vec<usize>>,
) -> AuthoringResult<Option<WeaponCloneSpec>> {
    let Some(settings) = spec
        .overrides
        .sparrow
        .as_ref()
        .filter(|s| s.handling.has_changes())
    else {
        return Ok(None);
    };
    if spec.kind != ItemKind::Sparrow {
        return Err(invalid("Vehicle traits require a Sparrow item"));
    }
    let types = crate::item::weapon_socket_types(definition)?;
    let lane = types
        .iter()
        .position(|t| *t == 61)
        .ok_or_else(|| invalid("Vehicle traits require the base item's Sparrow Engine socket"))?;
    if spec
        .overrides
        .socket_columns
        .get(lane)
        .and_then(|c| c.as_ref())
        .and_then(|c| c.socket_type)
        .is_some_and(|t| t != 61)
    {
        return Err(invalid(
            "Vehicle traits need the Sparrow Engine socket to retain its role",
        ));
    }
    let mut expanded = spec.clone();
    let column = expanded
        .overrides
        .socket_columns
        .get(lane)
        .and_then(|c| c.as_ref());
    let source = match column {
        Some(column) => *column
            .choices
            .first()
            .ok_or_else(|| invalid("Vehicle traits require an equipped engine plug"))?,
        None => {
            let block = relative_target(definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET)?;
            let (count, _, rows, class) = array_at(definition, block)?;
            if class != ITEM_ORDINARY_SOCKET_ROW_CLASS || lane >= count {
                return Err(invalid("Vehicle traits found an unsupported socket list"));
            }
            let index = read_u16(
                definition,
                rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE
                    + ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET,
            )?;
            if index == u16::MAX {
                return Err(invalid("Vehicle traits require a stock engine plug"));
            }
            read_u32(
                item_table,
                item_rows + usize::from(index) * ITEM_INDEX_ROW_SIZE,
            )?
        }
    };
    let source_rows = item_rows_by_hash
        .get(&source)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let [source_index] = source_rows else {
        return Err(invalid(
            "Vehicle traits require an unambiguous stock engine plug",
        ));
    };
    let source_tag = read_u32(
        item_table,
        item_rows + *source_index * ITEM_INDEX_ROW_SIZE + 16,
    )?;
    let source_definition = manager
        .read_tag(TagHash(source_tag))
        .map_err(|error| invalid(format!("Vehicle engine plug: {error}")))?;
    let supplied = base_sandbox_perks(&source_definition).map_err(invalid)?;
    if expanded.overrides.socket_columns.is_empty() {
        expanded.overrides.socket_columns.resize(types.len(), None);
    }
    if expanded.overrides.socket_columns.len() < types.len() {
        return Err(invalid(
            "Vehicle traits found an incomplete socket override list",
        ));
    }
    expanded.overrides.socket_columns[lane].get_or_insert_with(|| WeaponSocketColumnOverride {
        choices: vec![source],
        socket_type: None,
        choice_weight_bits: Vec::new(),
        choice_conditions: Vec::new(),
        reusable_plug_set_index: None,
        randomized_plug_set_index: None,
        randomized_selection_program: Vec::new(),
    });
    let slot =
        u16::try_from(lane).map_err(|_| invalid("Vehicle trait socket index is too large"))?;
    let existing = expanded
        .overrides
        .socket_plug_variants
        .iter()
        .position(|v| v.socket_index == slot && v.choice_index == 0);
    let variant = if let Some(index) = existing {
        let variant = &mut expanded.overrides.socket_plug_variants[index];
        if variant.source_plug_hash != source {
            return Err(invalid(
                "Vehicle traits conflict with this engine's private plug source",
            ));
        }
        variant
    } else {
        expanded
            .overrides
            .socket_plug_variants
            .push(WeaponSocketPlugVariantOverride {
                replace_effects: false,
                investment_stats: Vec::new(),
                socket_index: slot,
                choice_index: 0,
                source_plug_hash: source,
                name: Some("Vehicle Tuning".into()),
                classification_donor_hash: None,
                icon: None,
                description: Some(
                    "Original engine with the selected vehicle handling and summon traits.".into(),
                ),
                offer_everywhere: false,
                additional_sandbox_perks: Vec::new(),
                sandbox_perks: Vec::new(),
            });
        expanded
            .overrides
            .socket_plug_variants
            .last_mut()
            .expect("inserted vehicle tuning plug")
    };
    for (enabled, row) in [
        (settings.handling.fast_summon, 325),
        (settings.handling.air_control, 327),
        (settings.handling.side_dodges, 328),
        (settings.handling.roll_tricks, 1042),
    ] {
        if enabled
            && (variant.replace_effects || !supplied.contains(&row))
            && !variant.additional_sandbox_perks.contains(&row)
            && !variant
                .sandbox_perks
                .iter()
                .any(|p| p.source_perk_index == row)
        {
            variant.additional_sandbox_perks.push(row);
        }
    }
    Ok(Some(expanded))
}

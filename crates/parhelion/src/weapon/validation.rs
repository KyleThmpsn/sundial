use super::*;

pub(super) fn validate_numeric_instructions(
    label: &str,
    instructions: &[WeaponNumericInstruction],
    allow_empty: bool,
) -> AuthoringResult<()> {
    if instructions.is_empty() {
        return if allow_empty {
            Ok(())
        } else {
            Err(invalid(format!("{label} cannot be empty")))
        };
    }
    if instructions.len() > 256 {
        return Err(invalid(format!(
            "{label} cannot contain more than 256 instructions"
        )));
    }
    let tokens = instructions
        .iter()
        .map(|instruction| (instruction.opcode, instruction.operand))
        .collect::<Vec<_>>();
    let depth = numeric_program_stack_depth(&tokens)
        .map_err(|error| invalid(format!("{label} is invalid: {error}")))?;
    if depth != 1 {
        return Err(invalid(format!(
            "{label} must leave exactly one value on the RPN stack; it leaves {depth}"
        )));
    }
    Ok(())
}

pub(super) fn validate_weapon_donor_references(spec: &WeaponCloneSpec) -> AuthoringResult<()> {
    if let Some(badge) = &spec.overrides.badge {
        badge.validate().map_err(invalid)?;
    }
    if let Some(lore) = &spec.overrides.lore {
        crate::presentation::validate_text(lore, 16384, "Lore").map_err(invalid)?;
        if lore.trim().is_empty() {
            return Err(invalid("Enter lore text or turn off custom lore."));
        }
    }
    spec.identity.validate_for_donor(spec.donor_item_hash)?;
    if matches!(spec.donor_item_hash, 0 | FNV1_EMPTY_HASH) {
        return Err(invalid(
            "The donor item hash must be nonzero and cannot use the reserved empty-name hash",
        ));
    }
    if spec
        .presentation_donor
        .as_ref()
        .is_some_and(|donor| matches!(donor.item_hash, 0 | FNV1_EMPTY_HASH))
    {
        return Err(invalid(
            "The geometry donor item hash must be nonzero and cannot use the reserved empty-name hash",
        ));
    }
    if spec
        .icon_donor
        .as_ref()
        .is_some_and(|donor| matches!(donor.item_hash, 0 | FNV1_EMPTY_HASH))
    {
        return Err(invalid(
            "The icon donor item hash must be nonzero and cannot use the reserved empty-name hash",
        ));
    }
    if spec
        .render_gear_donor
        .as_ref()
        .is_some_and(|donor| matches!(donor.item_hash, 0 | FNV1_EMPTY_HASH))
    {
        return Err(invalid(
            "The render-gear donor item hash must be nonzero and cannot use the reserved empty-name hash",
        ));
    }
    let mut runtime_bindings = BTreeSet::new();
    for donor in &spec.runtime_component_donors {
        if matches!(donor.binding_hash, 0 | u32::MAX)
            || !runtime_bindings.insert(donor.binding_hash)
        {
            return Err(invalid(
                "Runtime component binding hashes must be non-reserved and unique",
            ));
        }
        if matches!(donor.item_hash, 0 | FNV1_EMPTY_HASH) {
            return Err(invalid(
                "Runtime component donor item hashes must be nonzero and cannot use the reserved empty-name hash",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_weapon_clone_text(text: &WeaponCloneText) -> AuthoringResult<()> {
    validate_localized_text("Weapon name", &text.name)?;
    for (label, value) in [
        ("Item type", text.type_name.as_deref()),
        ("Collection name", text.collection_name.as_deref()),
        (
            "Collection description",
            text.collection_description.as_deref(),
        ),
        ("Inventory hint", text.inventory_hint.as_deref()),
        (
            "Collection requirement",
            text.collection_requirement.as_deref(),
        ),
    ] {
        if let Some(value) = value {
            validate_localized_text(label, value)?;
        }
    }
    if !text.flavor.is_empty() {
        validate_localized_text("Flavor text", &text.flavor)?;
    }
    validate_localized_text("Source text", &text.source)?;
    let mut locales = BTreeSet::new();
    for locale in &text.locale_overrides {
        if usize::from(locale.locale_index) >= LOCALIZATION_LOCALE_COUNT
            || !locales.insert(locale.locale_index)
        {
            return Err(invalid(format!(
                "Localized payload override {} is outside 0..{} or duplicated",
                locale.locale_index,
                LOCALIZATION_LOCALE_COUNT - 1
            )));
        }
        for (label, override_value, primary_value) in [
            (
                "item type",
                locale.type_name.as_ref(),
                text.type_name.as_ref(),
            ),
            (
                "collection name",
                locale.collection_name.as_ref(),
                text.collection_name.as_ref(),
            ),
            (
                "collection description",
                locale.collection_description.as_ref(),
                text.collection_description.as_ref(),
            ),
            (
                "inventory hint",
                locale.inventory_hint.as_ref(),
                text.inventory_hint.as_ref(),
            ),
            (
                "collection requirement",
                locale.collection_requirement.as_ref(),
                text.collection_requirement.as_ref(),
            ),
        ] {
            if override_value.is_some() && primary_value.is_none() {
                return Err(invalid(format!(
                    "Locale {} cannot replace {label} because the primary recipe does not author that optional text field",
                    locale.locale_index
                )));
            }
        }
        for (label, value) in [
            ("name", locale.name.as_deref()),
            ("item type", locale.type_name.as_deref()),
            ("flavor", locale.flavor.as_deref()),
            ("source", locale.source.as_deref()),
            ("collection name", locale.collection_name.as_deref()),
            (
                "collection description",
                locale.collection_description.as_deref(),
            ),
            ("inventory hint", locale.inventory_hint.as_deref()),
            (
                "collection requirement",
                locale.collection_requirement.as_deref(),
            ),
        ] {
            if let Some(value) = value {
                if label == "flavor" && value.is_empty() {
                    continue;
                }
                validate_localized_text(&format!("Locale {} {label}", locale.locale_index), value)?;
            }
        }
    }
    Ok(())
}

pub(super) fn validate_investment_stat_definitions(
    overrides: &WeaponCloneOverrides,
) -> AuthoringResult<()> {
    let mut definitions = BTreeSet::new();
    for &(definition, _) in &overrides.investment_stats {
        if !definitions.insert(definition) {
            return Err(invalid(format!(
                "Investment stat definition {definition} is overridden more than once"
            )));
        }
    }
    let mut removed = BTreeSet::new();
    for &definition in &overrides.removed_investment_stats {
        if u8::try_from(definition).is_err() {
            return Err(invalid(format!(
                "Removed investment stat definition {definition} does not fit the native 8-bit field"
            )));
        }
        if !removed.insert(definition) {
            return Err(invalid(format!(
                "Investment stat definition {definition} is removed more than once"
            )));
        }
        if definitions.contains(&definition) {
            return Err(invalid(format!(
                "Investment stat definition {definition} cannot be both overridden and removed"
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_base_perks_and_traits(
    overrides: &WeaponCloneOverrides,
) -> AuthoringResult<()> {
    if let Some(perks) = &overrides.base_sandbox_perks {
        validate_unique_native_indices("Base sandbox-perk", perks, 64)?;
    }
    if let Some(traits) = &overrides.trait_indices {
        validate_unique_native_indices("Item-trait", traits, 256)?;
    }
    Ok(())
}

pub(super) fn validate_unique_native_indices(
    label: &str,
    indices: &[u16],
    maximum: usize,
) -> AuthoringResult<()> {
    if indices.len() > maximum {
        return Err(invalid(format!(
            "A weapon cannot author more than {maximum} {label} rows"
        )));
    }
    let mut unique = BTreeSet::new();
    for &index in indices {
        if index == u16::MAX || !unique.insert(index) {
            return Err(invalid(format!(
                "{label} index {index} is disabled or duplicated"
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_native_scalar_overrides(
    overrides: &WeaponCloneOverrides,
) -> AuthoringResult<()> {
    if overrides
        .max_stack_size
        .is_some_and(|value| value == 0 || value > i32::MAX as u32)
    {
        return Err(invalid(
            "Weapon max stack size must fit the native positive signed 32-bit field",
        ));
    }
    if overrides.power_cap_group.is_some() && overrides.power_cap_groups.is_some() {
        return Err(invalid(
            "Choose either one power-cap group for every version row or advanced per-row power-cap groups, not both",
        ));
    }
    if let Some(groups) = &overrides.power_cap_groups {
        if groups.is_empty() {
            return Err(invalid(
                "Advanced power-cap groups must contain at least one table index",
            ));
        }
    }
    if overrides.weapon_pattern_index == Some(u16::MAX) {
        return Err(invalid(
            "Weapon pattern index 65535 is a native disabled sentinel",
        ));
    }
    Ok(())
}

pub(super) fn validate_presentation_overrides(
    overrides: &WeaponCloneOverrides,
) -> AuthoringResult<()> {
    if let Some(rows) = &overrides.art_arrangements {
        if rows.is_empty() || rows.len() > 4 {
            return Err(invalid(
                "Translation art must contain between one and four class rows",
            ));
        }
        let mut classes = BTreeSet::new();
        for row in rows {
            if !(-1..=2).contains(&row.character_class)
                || row.arrangement == u16::MAX
                || !classes.insert(row.character_class)
            {
                return Err(invalid(
                    "Translation-art rows require distinct classes -1 through 2 and active arrangement indices",
                ));
            }
        }
    }
    if let Some(arrays) = &overrides.render_dye_rows {
        for (array, rows) in arrays.iter().enumerate() {
            if rows.len() > 32 {
                return Err(invalid(format!(
                    "Translation dye array {array} cannot contain more than 32 rows"
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_socket_column_shapes(
    columns: &[Option<WeaponSocketColumnOverride>],
    variants: &[WeaponSocketPlugVariantOverride],
) -> AuthoringResult<()> {
    for (lane, column) in columns.iter().enumerate() {
        let Some(column) = column else {
            continue;
        };
        validate_socket_column_choices(lane, column, variants)?;
        validate_socket_column_weights(lane, column)?;
        validate_socket_column_programs(lane, column)?;
    }
    Ok(())
}

pub(super) fn validate_socket_column_choices(
    lane: usize,
    column: &WeaponSocketColumnOverride,
    variants: &[WeaponSocketPlugVariantOverride],
) -> AuthoringResult<()> {
    if column.socket_type == Some(u16::MAX) && column.choices.is_empty() {
        if !column.choice_weight_bits.is_empty()
            || !column.choice_conditions.is_empty()
            || column.reusable_plug_set_index.is_some()
            || column.randomized_plug_set_index.is_some()
            || !column.randomized_selection_program.is_empty()
        {
            return Err(invalid(format!(
                "Removed socket {lane} cannot retain plug selection settings"
            )));
        }
        return Ok(());
    }
    if !(1..=MAX_AUTHORED_EMBEDDED_SOCKET_CHOICES).contains(&column.choices.len()) {
        return Err(invalid(format!(
            "Authored socket column {lane} must contain between one and {MAX_AUTHORED_EMBEDDED_SOCKET_CHOICES} choices"
        )));
    }
    let mut choices = BTreeMap::<u32, Vec<usize>>::new();
    let variant_at = |choice| {
        variants.iter().find(|variant| {
            usize::from(variant.socket_index) == lane && usize::from(variant.choice_index) == choice
        })
    };
    for (choice, &hash) in column.choices.iter().enumerate() {
        if hash == 0 {
            return Err(invalid(format!(
                "Authored socket column {lane} contains plug hash zero"
            )));
        }
        let previous = choices.entry(hash).or_default();
        if previous
            .iter()
            .any(|&other| match (variant_at(other), variant_at(choice)) {
                (None, None) => true,
                (Some(left), Some(right)) => left.same_definition(right),
                _ => false,
            })
        {
            return Err(invalid(format!(
                "Authored socket column {lane} contains duplicate plug 0x{hash:08X}"
            )));
        }
        previous.push(choice);
    }
    if column.socket_type == Some(u16::MAX) {
        return Err(invalid(format!(
            "Authored socket column {lane} uses the disabled socket-type sentinel"
        )));
    }
    Ok(())
}

pub(super) fn validate_socket_column_weights(
    lane: usize,
    column: &WeaponSocketColumnOverride,
) -> AuthoringResult<()> {
    if column.choice_weight_bits.is_empty() {
        return Ok(());
    }
    if column.choice_weight_bits.len() != column.choices.len() {
        return Err(invalid(format!(
            "Authored socket column {lane} has {} weights for {} choices",
            column.choice_weight_bits.len(),
            column.choices.len()
        )));
    }
    let weights = column
        .choice_weight_bits
        .iter()
        .map(|bits| f32::from_bits(*bits))
        .collect::<Vec<_>>();
    if weights
        .iter()
        .any(|weight| !weight.is_finite() || *weight < 0.0)
        || !weights.iter().any(|weight| *weight > 0.0)
    {
        return Err(invalid(format!(
            "Authored socket column {lane} weights must be finite, nonnegative, and include at least one positive value"
        )));
    }
    Ok(())
}

pub(super) fn validate_socket_column_programs(
    lane: usize,
    column: &WeaponSocketColumnOverride,
) -> AuthoringResult<()> {
    if !column.choice_conditions.is_empty()
        && column.choice_conditions.len() != column.choices.len()
    {
        return Err(invalid(format!(
            "Authored socket column {lane} has {} conditions for {} choices",
            column.choice_conditions.len(),
            column.choices.len()
        )));
    }
    for (choice, condition) in column.choice_conditions.iter().enumerate() {
        validate_numeric_instructions(
            &format!("Socket column {lane} choice {choice} condition"),
            condition,
            true,
        )?;
    }
    if column.reusable_plug_set_index == Some(u16::MAX)
        || column.randomized_plug_set_index == Some(u16::MAX)
    {
        return Err(invalid(format!(
            "Authored socket column {lane} uses 65535 as an active plug-set index"
        )));
    }
    match column.randomized_plug_set_index {
        Some(_) => validate_numeric_instructions(
            &format!("Socket column {lane} randomized selection program"),
            &column.randomized_selection_program,
            false,
        ),
        None if !column.randomized_selection_program.is_empty() => Err(invalid(format!(
            "Authored socket column {lane} has a randomized selection program without a randomized plug set"
        ))),
        None => Ok(()),
    }
}

pub(crate) fn validate_socket_plug_variant_shapes(
    variants: &[WeaponSocketPlugVariantOverride],
) -> AuthoringResult<()> {
    let mut positions = BTreeSet::new();
    for (variant_index, variant) in variants.iter().enumerate() {
        let mut stat_indices = BTreeSet::new();
        for &(index, _) in &variant.investment_stats {
            if u8::try_from(index).is_err() || !stat_indices.insert(index) {
                return Err(invalid(format!(
                    "Custom perk {variant_index} stat contributions must use unique native 8-bit stat indices"
                )));
            }
        }
        let position = (variant.socket_index, variant.choice_index);
        if !positions.insert(position) {
            return Err(invalid(format!(
                "Socket-plug variants contain more than one edit for socket {} choice {}",
                variant.socket_index, variant.choice_index
            )));
        }
        if matches!(variant.source_plug_hash, 0 | FNV1_EMPTY_HASH) {
            return Err(invalid(format!(
                "Socket-plug variant {variant_index} has a reserved source plug hash"
            )));
        }
        if let Some(name) = &variant.name {
            validate_localized_text("Private socket-plug name", name)?;
        }
        if let Some(description) = &variant.description {
            validate_localized_text("Private socket-plug description", description)?;
        }
        if variant.additional_sandbox_perks.len() > 64
            || variant.additional_sandbox_perks.contains(&u16::MAX)
            || variant
                .additional_sandbox_perks
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != variant.additional_sandbox_perks.len()
        {
            return Err(invalid(
                "Additional private-plug effects must be unique active perk indices (at most 64)",
            ));
        }
        if variant
            .classification_donor_hash
            .is_some_and(|hash| matches!(hash, 0 | u32::MAX | FNV1_EMPTY_HASH))
        {
            return Err(invalid(format!(
                "Socket-plug variant {variant_index} has a reserved classification source hash"
            )));
        }
        if variant.sandbox_perks.is_empty() && !variant.replace_effects {
            return Err(invalid(format!(
                "Socket-plug variant {variant_index} does not select a finished sandbox perk"
            )));
        }
        let mut perk_indices = BTreeSet::new();
        for perk in &variant.sandbox_perks {
            if let Some(program) = &perk.program {
                program.validate().map_err(invalid)?;
                if perk.activation.is_some()
                    || !perk.runtime_values.is_empty()
                    || !perk.action_float_values.is_empty()
                    || !perk.projectiles.is_empty()
                {
                    return Err(invalid(
                        "A custom effect program cannot also contain stock action overrides.",
                    ));
                }
                for action in &program.actions {
                    validate_runtime_value_override_shapes(&action.asset().values)?;
                }
            }
            if perk.activation.is_some()
                && !sundial::package_authoring::sandbox_perk::activation::supports_activation(
                    perk.source_perk_index,
                )
            {
                return Err(invalid(format!(
                    "Socket-plug variant {variant_index}: activation conditions currently support only mapped Outlaw effects, not finished perk {}",
                    perk.source_perk_index
                )));
            }
            if !perk_indices.insert(perk.source_perk_index) {
                return Err(invalid(format!(
                    "Socket-plug variant {variant_index} selects finished sandbox-perk {} more than once",
                    perk.source_perk_index
                )));
            }
            let mut projectile_sources = BTreeSet::new();
            for projectile in &perk.projectiles {
                if !projectile_sources.insert(projectile.source_graph)
                    || [projectile.source_graph, projectile.donor_graph]
                        .iter()
                        .any(|tag| matches!(*tag, 0 | u32::MAX))
                {
                    return Err(invalid(
                        "Projectile selections must have unique source graphs and valid donor identities",
                    ));
                }
            }
            if !perk.runtime_values.is_empty() {
                validate_runtime_value_override_shapes(&perk.runtime_values).map_err(|error| {
                    error.context(format!(
                        "Socket-plug variant {variant_index} finished sandbox-perk {}",
                        perk.source_perk_index
                    ))
                })?;
            }
            let mut action_value_locators = BTreeSet::new();
            for (value_index, value) in perk.action_float_values.iter().enumerate() {
                if (value.node_type_handle & 0xFFF0_0000) != 0x8080_0000
                    || (value.value_type_handle & 0xFFF0_0000) != 0x8080_0000
                    || value.value_pointer_offset > 0x10_0000
                    || value.value_pointer_offset % 4 != 0
                    || !f32::from_bits(value.expected_bits).is_finite()
                    || !f32::from_bits(value.value_bits).is_finite()
                    || value.expected_bits == value.value_bits
                {
                    return Err(invalid(format!(
                        "Socket-plug variant {variant_index} finished sandbox-perk {} action float {value_index} has an invalid locator or value",
                        perk.source_perk_index
                    )));
                }
                if !action_value_locators.insert((
                    value.node_type_handle,
                    value.node_occurrence,
                    value.value_pointer_offset,
                    value.value_type_handle,
                )) {
                    return Err(invalid(format!(
                        "Socket-plug variant {variant_index} finished sandbox-perk {} repeats an action-float locator",
                        perk.source_perk_index
                    )));
                }
            }
        }
    }
    Ok(())
}

pub(super) fn validate_raw_payload_patch_shapes(
    patches: &[WeaponRawPayloadPatch],
) -> AuthoringResult<()> {
    if patches
        .iter()
        .any(|patch| patch.target == WeaponRawPayloadTarget::ItemDefinition)
        && patches
            .iter()
            .any(|patch| ITEM_DEFINITION_RAW_SUBTARGETS.contains(&patch.target))
    {
        return Err(invalid(
            "Raw gameplay-definition patches cannot be mixed with named item-block patches; choose one coordinate space so final bytes are unambiguous",
        ));
    }
    for (patch_index, patch) in patches.iter().enumerate() {
        if patch.bytes.is_empty() {
            return Err(invalid(format!(
                "Raw payload patch {patch_index} cannot be empty"
            )));
        }
        if patch.bytes.len() > 4096 {
            return Err(invalid(format!(
                "Raw payload patch {patch_index} exceeds the 4096-byte per-patch limit"
            )));
        }
        let range = raw_patch_range(patch_index, patch)?;
        for (other_index, other) in patches[..patch_index]
            .iter()
            .enumerate()
            .filter(|(_, other)| other.target == patch.target)
        {
            let other_range = raw_patch_range(other_index, other)?;
            if range.start < other_range.end && other_range.start < range.end {
                return Err(invalid(format!(
                    "Raw payload patches {other_index} and {patch_index} overlap"
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_runtime_resource_patch_shapes(
    patches: &[WeaponRuntimeResourcePatch],
) -> AuthoringResult<()> {
    for (patch_index, patch) in patches.iter().enumerate() {
        if !patch.graph_values.is_empty() {
            if patch.bytes.len() != 4 {
                return Err(invalid(format!(
                    "Runtime resource patch {patch_index} with graph values needs exactly four graph-tag bytes"
                )));
            }
            validate_runtime_value_override_shapes(&patch.graph_values)?;
        }
        if matches!(patch.binding_hash, 0 | u32::MAX) {
            return Err(invalid(format!(
                "Runtime resource patch {patch_index} has a reserved component-binding hash"
            )));
        }
        if patch.bytes.is_empty() {
            return Err(invalid(format!(
                "Runtime resource patch {patch_index} cannot be empty"
            )));
        }
        if patch.bytes.len() > 4096 {
            return Err(invalid(format!(
                "Runtime resource patch {patch_index} exceeds the 4096-byte per-patch limit"
            )));
        }
        let start = usize::try_from(patch.offset).map_err(|_| {
            invalid(format!(
                "Runtime resource patch {patch_index} offset is too large"
            ))
        })?;
        let end = start.checked_add(patch.bytes.len()).ok_or_else(|| {
            invalid(format!(
                "Runtime resource patch {patch_index} range overflows"
            ))
        })?;
        for (other_index, other) in
            patches[..patch_index]
                .iter()
                .enumerate()
                .filter(|(_, other)| {
                    other.binding_hash == patch.binding_hash
                        && other.resource_index == patch.resource_index
                })
        {
            let other_start = usize::try_from(other.offset).map_err(|_| {
                invalid(format!(
                    "Runtime resource patch {other_index} offset is too large"
                ))
            })?;
            let other_end = other_start.checked_add(other.bytes.len()).ok_or_else(|| {
                invalid(format!(
                    "Runtime resource patch {other_index} range overflows"
                ))
            })?;
            if start < other_end && other_start < end {
                return Err(invalid(format!(
                    "Runtime resource patches {other_index} and {patch_index} overlap"
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_runtime_value_override_shapes(
    overrides: &[WeaponRuntimeValueOverride],
) -> AuthoringResult<()> {
    let mut locators = BTreeSet::new();
    for (index, runtime) in overrides.iter().enumerate() {
        let locator = &runtime.locator;
        if matches!(locator.binding_hash, 0 | u32::MAX) {
            return Err(invalid(format!(
                "Runtime value {index} has a reserved component-binding hash"
            )));
        }
        // Native component schemas are package-backed type handles and do not all live in the
        // built-in 0x808xxxxx range. The exact schema and leaf type are proved again against the
        // effective runtime graph before any bytes are written, so normalization only rejects the
        // two reserved sentinel values here.
        if matches!(locator.root_schema, 0 | u32::MAX)
            || matches!(locator.type_handle, 0 | u32::MAX)
        {
            return Err(invalid(format!(
                "Runtime value {index} has an invalid reflected schema or field-type handle"
            )));
        }
        if locator.byte_size == 0 || locator.byte_size > 0x10_0000 {
            return Err(invalid(format!(
                "Runtime value {index} has invalid native width {}",
                locator.byte_size
            )));
        }
        if locator.path.len() > 32 {
            return Err(invalid(format!(
                "Runtime value {index} exceeds the native schema nesting limit"
            )));
        }
        if !locators.insert(locator.clone()) {
            return Err(invalid(format!(
                "Runtime value {index} duplicates an earlier reflected field"
            )));
        }
    }
    Ok(())
}

pub(super) fn raw_patch_range(
    patch_index: usize,
    patch: &WeaponRawPayloadPatch,
) -> AuthoringResult<std::ops::Range<usize>> {
    let start = usize::try_from(patch.offset).map_err(|_| {
        invalid(format!(
            "Raw payload patch {patch_index} offset is too large"
        ))
    })?;
    let end = start
        .checked_add(patch.bytes.len())
        .ok_or_else(|| invalid(format!("Raw payload patch {patch_index} range overflows")))?;
    Ok(start..end)
}

/// Validates compiler-facing investment overrides against the selected install's decoded donor
/// catalog. Native payload topology is checked separately during dependency resolution and build.
///
/// Structural recipe/spec validation remains with the public compiler wrappers. Loading Sundial's
/// catalog is skipped when no spec selects a presentation donor or changes a catalog-validated
/// stat, perk, trait, socket, translation selector, plug metadata field or raw payload range.
#[cfg(test)]
pub(crate) fn validate_weapon_clone_specs_against_catalog<'a>(
    install_directory: &Path,
    specs: impl IntoIterator<Item = &'a WeaponCloneSpec>,
) -> AuthoringResult<()> {
    validate_catalog_with_progress(install_directory, specs, &mut |_, _, _, _| {})
}

pub(crate) fn validate_catalog_with_progress<'a>(
    install_directory: &Path,
    specs: impl IntoIterator<Item = &'a WeaponCloneSpec>,
    progress: &mut dyn FnMut(bool, &str, usize, usize),
) -> AuthoringResult<()> {
    let specs = specs
        .into_iter()
        .filter(|spec| {
            spec.presentation_donor.is_some()
                || !spec.overrides.investment_stats.is_empty()
                || !spec.overrides.removed_investment_stats.is_empty()
                || spec.overrides.base_sandbox_perks.is_some()
                || spec.overrides.trait_indices.is_some()
                || !spec.overrides.socket_columns.is_empty()
                || spec.overrides.weapon_pattern_index.is_some()
                || spec.overrides.stat_group_index.is_some()
                || spec.overrides.socket_entry_list_index.is_some()
                || spec.overrides.plug_category_hash.is_some()
                || spec.overrides.roll_set_index.is_some()
                || spec.overrides.linked_plug_index.is_some()
                || !spec.overrides.raw_payload_patches.is_empty()
        })
        .collect::<Vec<_>>();
    if specs.is_empty() {
        return Ok(());
    }

    let catalog = InvestmentCatalog::load(install_directory, false, |event| {
        progress(true, event.message, event.completed, event.total);
    })
    .map_err(|error| invalid(format!("Could not load Sundial's donor catalog: {error}")))?;
    let installed_donors = catalog.weapon_donors();
    let installed_sandbox_perks = catalog
        .weapon_sandbox_perk_choices_from(crate::package_profile::is_stock_item_definition)
        .into_iter()
        .map(|choice| choice.perk_index)
        .collect::<BTreeSet<_>>();
    let installed_trait_indices = catalog
        .weapon_trait_choices()
        .into_iter()
        .map(|choice| choice.trait_index)
        .collect::<BTreeSet<_>>();
    let reusable_plug_set_count = catalog.reusable_plug_set_count();
    let socket_entry_list_count = catalog.socket_entry_list_count();

    let total = specs.len();
    for (index, spec) in specs.into_iter().enumerate() {
        progress(false, &spec.text.name, index, total);
        (|| -> AuthoringResult<()> {
        if let Some(reference) = &spec.presentation_donor {
            let gameplay = installed_donors
                .iter()
                .find(|donor| donor.hash == spec.donor_item_hash)
                .ok_or_else(|| invalid("Gameplay donor is not an installed weapon"))?;
            let appearance = installed_donors
                .iter()
                .find(|donor| donor.hash == reference.item_hash)
                .ok_or_else(|| invalid("Geometry donor is not an installed weapon"))?;
            let target = spec
                .overrides
                .inventory_slot
                .map(|slot| match slot {
                    WeaponInventorySlot::Kinetic => {
                        sundial::investment::WeaponInventorySlot::Kinetic
                    }
                    WeaponInventorySlot::Energy => sundial::investment::WeaponInventorySlot::Energy,
                    WeaponInventorySlot::Power => sundial::investment::WeaponInventorySlot::Power,
                })
                .or(gameplay.inventory_slot)
                .ok_or_else(|| invalid("Base weapon has an unknown inventory slot"))?;
            let mut runtime = gameplay.clone();
            runtime.weapon_translation_group =
                crate::capabilities::effective_weapon_translation_group(
                    gameplay,
                    spec.overrides.weapon_pattern_index,
                    &installed_donors,
                );
            match crate::capabilities::appearance_compatibility(appearance, &runtime, target) {
                crate::capabilities::AppearanceCompatibility::Compatible => {}
                assessment => {
                    return Err(invalid(format!(
                        "Weapon {:?}: appearance compatibility: {assessment:?}",
                        spec.text.name
                    )));
                }
            }
        }
        if let Some(index) = spec.overrides.socket_entry_list_index
            && usize::from(index) >= socket_entry_list_count
        {
            return Err(invalid(format!(
                "Weapon {:?} ({}) selects socket-entry-list index {index}, but the installed table contains {socket_entry_list_count} rows",
                spec.text.name, spec.namespace
            )));
        }
        for (lane, column) in spec.overrides.socket_columns.iter().enumerate() {
            let Some(column) = column else {
                continue;
            };
            for (kind, index) in [
                ("reusable", column.reusable_plug_set_index),
                ("randomized", column.randomized_plug_set_index),
            ] {
                if let Some(index) = index
                    && usize::from(index) >= reusable_plug_set_count
                {
                    return Err(invalid(format!(
                        "Weapon {:?} ({}) socket column {lane} selects {kind} plug-set index {index}, but the installed table contains {reusable_plug_set_count} rows",
                        spec.text.name, spec.namespace
                    )));
                }
            }
        }
        if let Some(perks) = &spec.overrides.base_sandbox_perks
            && let Some(perk) = perks
                .iter()
                .find(|perk| !installed_sandbox_perks.contains(perk))
        {
            return Err(invalid(format!(
                "Weapon {:?} ({}) selects base sandbox-perk index {perk}, but that row is not active and referenced in the installed catalog",
                spec.text.name, spec.namespace
            )));
        }
        if let Some(traits) = &spec.overrides.trait_indices
            && let Some(trait_index) = traits
                .iter()
                .find(|trait_index| !installed_trait_indices.contains(trait_index))
        {
            return Err(invalid(format!(
                "Weapon {:?} ({}) selects item-trait index {trait_index}, but that definition is not present in the installed trait table",
                spec.text.name, spec.namespace
            )));
        }
        if let Some(weapon_pattern_index) = spec.overrides.weapon_pattern_index
            && !installed_donors
                .iter()
                .any(|donor| donor.weapon_pattern_index == Some(weapon_pattern_index))
        {
            return Err(invalid(format!(
                "Weapon {:?} ({}) selects weapon-pattern index {weapon_pattern_index}, but no installed stock weapon represents it",
                spec.text.name, spec.namespace
            )));
        }
        if let Some(stat_group_index) = spec.overrides.stat_group_index
            && !installed_donors
                .iter()
                .any(|donor| donor.stat_group_index == Some(stat_group_index))
        {
            return Err(invalid(format!(
                "Weapon {:?} ({}) selects stat-display group {stat_group_index}, but no installed stock weapon represents it",
                spec.text.name, spec.namespace
            )));
        }

        let donor = catalog
            .weapon_donor_with_stat_group_index(
                spec.donor_item_hash,
                spec.overrides.stat_group_index,
            )
            .ok_or_else(|| {
            invalid(format!(
                "Weapon {:?} ({}) donor 0x{:08X} is not an installed weapon or its selected stat-display group is unavailable",
                spec.text.name, spec.namespace, spec.donor_item_hash,
            ))
        })?;
        let mut diagnostics = validate_stat_overrides(&donor, &spec.overrides.investment_stats);
        for variant in &spec.overrides.socket_plug_variants {
            let source_stats = catalog.item_stat_contributions(variant.source_plug_hash);
            for &(index, _) in &variant.investment_stats {
                if variant.replace_effects
                    && catalog
                        .perk_stat_choices()
                        .iter()
                        .any(|stat| stat.definition_index == index)
                {
                    continue;
                }
                if !source_stats
                    .iter()
                    .chain(&donor.investment_stats)
                    .chain(&donor.addable_investment_stats)
                    .any(|stat| stat.definition_index == index)
                {
                    return Err(invalid(format!(
                        "Custom perk stat {index} is not declared by the source plug or any installed weapon"
                    )));
                }
            }
        }
        for definition_index in &spec.overrides.removed_investment_stats {
            if !donor
                .investment_stats
                .iter()
                .any(|stat| stat.definition_index == *definition_index)
            {
                return Err(invalid(format!(
                    "Weapon {:?} ({}) cannot remove investment stat definition {definition_index} because its gameplay donor does not contain that row",
                    spec.text.name, spec.namespace
                )));
            }
        }

        if !spec.overrides.socket_columns.is_empty() {
            let overrides = spec
                .overrides
                .socket_columns
                .iter()
                .map(|column| column.as_ref().map(|column| column.choices.clone()))
                .collect::<Vec<_>>();
            let socket_types = spec
                .overrides
                .socket_columns
                .iter()
                .map(|column| column.as_ref().and_then(|column| column.socket_type))
                .collect::<Vec<_>>();
            for (socket_index, socket_type) in socket_types.iter().copied().enumerate() {
                if let Some(socket_type) = socket_type
                    && socket_type != u16::MAX
                    && !catalog.weapon_socket_type_is_known(spec.donor_item_hash, socket_type)
                {
                    return Err(invalid(format!(
                        "Weapon {:?} ({}) socket {socket_index} uses unknown native socket type {socket_type}",
                        spec.text.name, spec.namespace
                    )));
                }
            }
            let supported = catalog
                .weapon_supported_plug_sets_with_socket_types(spec.donor_item_hash, &socket_types)
                .map_err(|error| {
                    invalid(format!(
                        "Could not decode compatible plugs for weapon {:?} ({}): {error}",
                        spec.text.name, spec.namespace
                    ))
                })?
                .into_iter()
                .map(|set| SupportedPlugSet {
                    socket_index: set.socket_index,
                    plug_hashes: set.plug_hashes,
                })
                .collect::<Vec<_>>();
            diagnostics.extend(validate_socket_column_overrides_with_socket_types(
                &donor,
                &overrides,
                &socket_types,
                &supported,
            ));
        }

        diagnostics.retain(crate::capabilities::AuthoringDiagnostic::is_build_blocking);

        if !diagnostics.is_empty() {
            let messages = diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(invalid(format!(
                "Weapon {:?} ({}) has donor-incompatible investment overrides: {messages}",
                spec.text.name, spec.namespace
            )));
        }
        Ok(())
        })().map_err(|error| error.context(spec.error_context()))?;
        progress(false, &spec.text.name, index + 1, total);
    }
    Ok(())
}

pub(super) fn validate_project_package_owners(
    host: &[TagHash],
    investment: &[TagHash],
    strings: &[TagHash],
    collection: &[TagHash],
) -> AuthoringResult<()> {
    if host.iter().any(|tag| tag.pkg_id() != HOST_PACKAGE_ID) {
        return Err(invalid(
            "Project host tables unexpectedly moved out of package 0914",
        ));
    }
    if investment
        .windows(2)
        .any(|pair| pair[0].pkg_id() != pair[1].pkg_id())
    {
        return Err(invalid(
            "Project item and collectible tables unexpectedly have different package owners",
        ));
    }
    if strings
        .windows(2)
        .any(|pair| pair[0].pkg_id() != pair[1].pkg_id())
    {
        return Err(invalid(
            "Project string, metadata, category, and node-string tables unexpectedly have different package owners",
        ));
    }
    if collection.first().map(TagHash::pkg_id) != Some(COLLECTION_PACKAGE_ID)
        || collection
            .windows(2)
            .any(|pair| pair[0].pkg_id() != pair[1].pkg_id())
    {
        return Err(invalid(
            "Project collection, objective, expression, unlock, and auxiliary-index tables unexpectedly have different package owners",
        ));
    }
    Ok(())
}

pub(super) fn validate_authored_payloads(
    definition: &[u8],
    strings: &[u8],
    socket_column_indices: Option<&[Option<ResolvedSocketColumn>]>,
    expected_weapon_pattern_index: Option<u16>,
    spec: &WeaponCloneSpec,
) -> AuthoringResult<()> {
    validate_item_root_holder_bounds(definition)?;
    validate_weapon_translation_markers(definition)?;
    let authored_version = weapon_version_array(definition)?;
    if matching_u32_offsets(definition, spec.donor_item_hash).is_empty()
        && matching_u32_offsets(definition, spec.identity.item_hash)
            == [ITEM_DEFINITION_HASH_OFFSET]
        && matching_u32_offsets(strings, spec.donor_item_hash).is_empty()
        && read_u32(strings, ITEM_NAME_REFERENCE_OFFSET)? == LOCALIZATION_DONOR_TABLE_INDEX as u32
        && read_u32(strings, ITEM_NAME_REFERENCE_OFFSET + 4)? == spec.identity.name_hash
        && spec.text.type_name.as_ref().is_none_or(|_| {
            read_u32(strings, ITEM_TYPE_REFERENCE_OFFSET).ok()
                == Some(LOCALIZATION_DONOR_TABLE_INDEX as u32)
                && read_u32(strings, ITEM_TYPE_REFERENCE_OFFSET + 4).ok()
                    == Some(spec.identity.type_hash)
        })
        && read_u32(strings, ITEM_DESCRIPTION_REFERENCE_OFFSET)?
            == LOCALIZATION_DONOR_TABLE_INDEX as u32
        && read_u32(strings, ITEM_DESCRIPTION_REFERENCE_OFFSET + 4)? == spec.identity.flavor_hash
        && if spec.text.inventory_hint.is_some() {
            read_u32(strings, ITEM_DISPLAY_SOURCE_REFERENCE_OFFSET)?
                == LOCALIZATION_DONOR_TABLE_INDEX as u32
                && read_u32(strings, ITEM_DISPLAY_SOURCE_REFERENCE_OFFSET + 4)?
                    == spec.identity.inventory_hint_hash
        } else {
            read_u32(strings, ITEM_DISPLAY_SOURCE_REFERENCE_OFFSET)?
                == BLANK_LOCALIZED_REFERENCE_TABLE_INDEX
                && read_u32(strings, ITEM_DISPLAY_SOURCE_REFERENCE_OFFSET + 4)?
                    == BLANK_LOCALIZED_REFERENCE_HASH
        }
        && matching_u32_offsets(strings, RANDOM_PERKS_NOT_REACQUIRABLE_SOURCE_HASH).is_empty()
    {
        let resource = relative_target(definition, ITEM_INVESTMENT_STAT_POINTER_OFFSET)?;
        let (count, _, rows, class) = array_at(definition, resource)?;
        if class == ITEM_INVESTMENT_STAT_ROW_CLASS
            && spec
                .overrides
                .investment_stats
                .iter()
                .all(|(definition_index, value)| {
                    (0..count).any(|index| {
                        let row = rows + index * ITEM_INVESTMENT_STAT_ROW_SIZE;
                        read_u8(definition, row).ok().map(u16::from) == Some(*definition_index)
                            && read_u8(definition, row + 1).ok() == Some(0)
                            && read_i32(definition, row + 4).ok() == Some(*value)
                    })
                })
            && spec
                .overrides
                .inventory_slot
                .is_none_or(|slot| weapon_inventory_slot(definition).ok() == Some(slot))
            && spec
                .overrides
                .ammo_type
                .is_none_or(|ammo| item_string_ammo_type(strings).ok() == Some(Some(ammo)))
            && spec.overrides.modern_damage_type.is_none_or(|damage| {
                weapon_damage_descriptor(definition).ok() == Some(damage.descriptor())
            })
            && spec
                .overrides
                .base_sandbox_perks
                .as_ref()
                .is_none_or(|perks| {
                    let actual = weapon_sandbox_perks(definition).ok();
                    actual.is_some_and(|actual| {
                        if spec.overrides.modern_damage_type.is_some() {
                            actual
                                .iter()
                                .copied()
                                .filter(|perk| fixed_damage_perk(*perk).is_none())
                                .eq(perks
                                    .iter()
                                    .copied()
                                    .filter(|perk| fixed_damage_perk(*perk).is_none()))
                        } else {
                            actual == *perks
                        }
                    })
                })
            && spec
                .overrides
                .trait_indices
                .as_ref()
                .is_none_or(|traits| weapon_item_traits(definition).ok().as_ref() == Some(traits))
            && spec
                .overrides
                .max_stack_size
                .is_none_or(|value| weapon_max_stack_size(definition).ok() == Some(value))
            && spec.overrides.socket_entry_list_index.is_none_or(|index| {
                weapon_socket_entry_list_index(definition).ok() == Some(Some(index))
            })
            && spec
                .overrides
                .plug_category_hash
                .is_none_or(|hash| weapon_plug_category_hash(definition).ok() == Some(hash))
            && spec
                .overrides
                .roll_set_index
                .is_none_or(|index| weapon_roll_set_index(definition).ok() == Some(index))
            && spec
                .overrides
                .linked_plug_index
                .is_none_or(|index| weapon_linked_plug_index(definition).ok() == Some(index))
            && spec.overrides.power_cap_group.is_none_or(|group| {
                authored_version.as_ref().is_some_and(|version| {
                    !version.groups.is_empty() && version.groups.iter().all(|value| *value == group)
                })
            })
            && spec
                .overrides
                .rarity
                .is_none_or(|rarity| weapon_rarity(definition).ok() == Some(rarity))
            && weapon_pattern_index(definition).ok() == Some(expected_weapon_pattern_index)
            && spec
                .overrides
                .art_arrangements
                .as_ref()
                .is_none_or(|rows| weapon_art_arrangements(definition).ok().as_ref() == Some(rows))
            && spec
                .overrides
                .render_dye_rows
                .as_ref()
                .is_none_or(|arrays| {
                    weapon_render_dye_rows(definition).ok().as_ref() == Some(arrays)
                })
            && spec
                .overrides
                .stat_group_index
                .is_none_or(|index| item_string_stat_group_index(strings).ok() == Some(index))
            && socket_column_indices.is_none_or(|columns| {
                let Ok(resource) = relative_target(definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET)
                else {
                    return false;
                };
                let Ok((socket_count, _, socket_rows, _)) = array_at(definition, resource) else {
                    return false;
                };
                let socket_types = (0..socket_count)
                    .map(|lane| {
                        read_u16(
                            definition,
                            socket_rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE,
                        )
                    })
                    .collect::<AuthoringResult<Vec<_>>>();
                socket_types.is_ok_and(|socket_types| {
                    validate_weapon_socket_columns(definition, columns, &socket_types).is_ok()
                })
            })
        {
            if let Some(columns) = socket_column_indices {
                let resource = relative_target(definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET)?;
                let (socket_count, _, socket_rows, _) = array_at(definition, resource)?;
                let socket_types = (0..socket_count)
                    .map(|lane| {
                        read_u16(
                            definition,
                            socket_rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE,
                        )
                    })
                    .collect::<AuthoringResult<Vec<_>>>()?;
                validate_weapon_socket_columns(definition, columns, &socket_types)?;
            }
            return Ok(());
        }
    }
    Err(validation(
        "Authored weapon payload identity, strings, flavor, source, stats, damage, cap, rarity, pattern, display group, or plugs are inconsistent",
    ))
}

pub(super) fn validate_authored_localization_values(
    localization: &AuthoredLocalization,
    weapons: &[WeaponCloneSpec],
    custom_plugs: &[ResolvedCustomPlug],
) -> AuthoringResult<()> {
    let header_values = project_authored_localized_values(weapons, custom_plugs, 0)?;
    let custom_values = header_values.as_slice();
    let merged_count = LOCALIZATION_DONOR_STRING_HASHES.len() + custom_values.len();
    let (table_count, _, table_rows, table_class) = array_at(&localization.index, 8)?;
    let table_end = table_rows
        .checked_add(
            table_count
                .checked_mul(LOCALIZED_INDEX_ROW_SIZE)
                .ok_or_else(|| validation("Localized index row size overflowed"))?,
        )
        .ok_or_else(|| validation("Localized index row range overflowed"))?;
    let donor_row = table_rows
        .checked_add(
            LOCALIZATION_DONOR_TABLE_INDEX
                .checked_mul(LOCALIZED_INDEX_ROW_SIZE)
                .ok_or_else(|| validation("Localized donor row size overflowed"))?,
        )
        .ok_or_else(|| validation("Localized donor row range overflowed"))?;
    if table_count != LOCALIZATION_STOCK_TABLE_COUNT
        || table_class != LOCALIZED_INDEX_ROW_CLASS
        || table_end != localization.index.len()
        || read_u32(&localization.index, donor_row)? != LOCALIZATION_DONOR_TABLE_KEY
        || read_u32(&localization.index, donor_row + 4)? != localization.donor_header_tag.0
    {
        return Err(validation(
            "Authored localization changed the stock index instead of extending the rooted donor bank",
        ));
    }

    let (hash_count, _, hash_rows, hash_class) = array_at(&localization.merged_header, 8)?;
    let mut expected_hashes = LOCALIZATION_DONOR_STRING_HASHES.to_vec();
    expected_hashes.extend(custom_values.iter().map(|(hash, _)| *hash));
    if hash_count != merged_count
        || hash_class != LOCALIZATION_HEADER_HASH_CLASS
        || hash_rows
            .checked_add(
                merged_count
                    .checked_mul(size_of::<u32>())
                    .ok_or_else(|| validation("Localized hash row size overflowed"))?,
            )
            .ok_or_else(|| validation("Localized hash row range overflowed"))?
            != localization.merged_header.len()
        || localization.locale_data.len() != LOCALIZATION_LOCALE_COUNT
    {
        return Err(validation(
            "Authored localization header did not preserve the donor hashes and append the expected authored hashes",
        ));
    }
    let actual_hashes = (0..hash_count)
        .map(|index| read_u32(&localization.merged_header, hash_rows + index * 4))
        .collect::<AuthoringResult<Vec<_>>>()?;
    if actual_hashes != expected_hashes {
        return Err(validation(
            "Authored localization hashes do not match the expected donor and authored hashes",
        ));
    }

    let mut locale_tags = BTreeSet::new();
    for (locale_index, locale) in localization.locale_data.iter().enumerate() {
        let locale_values = project_authored_localized_values(weapons, custom_plugs, locale_index)?;
        let header_tag_offset = LOCALIZATION_DATA_TAG_START + locale_index * 4;
        let (part_count, _, _, part_class) = array_at(&locale.payload, 8)?;
        let (aux_count, _, _, aux_class) =
            array_at(&locale.payload, LOCALIZATION_AUX_DESCRIPTOR_OFFSET)?;
        let (byte_count, _, bytes, byte_class) =
            array_at(&locale.payload, LOCALIZATION_BYTE_DESCRIPTOR_OFFSET)?;
        let (combo_count, _, _, combo_class) =
            array_at(&locale.payload, LOCALIZATION_COMBO_DESCRIPTOR_OFFSET)?;
        let localized_bytes = bytes
            .checked_add(byte_count)
            .and_then(|end| locale.payload.get(bytes..end));
        if read_u32(&localization.merged_header, header_tag_offset)? != locale.donor_tag.0
            || part_count != merged_count
            || part_class != LOCALIZATION_PART_CLASS
            || aux_count == 0
            || aux_class != LOCALIZATION_AUX_CLASS
            || byte_count == 0
            || byte_class != LOCALIZATION_BYTE_CLASS
            || localized_bytes.and_then(|bytes| bytes.last()) != Some(&0)
            || combo_count != merged_count
            || combo_class != LOCALIZATION_COMBO_CLASS
            || locale_values
                .iter()
                .enumerate()
                .any(|(index, (_, expected))| {
                    decode_localized_value_at(&locale.payload, 2 + index)
                        .ok()
                        .as_deref()
                        != Some(*expected)
                })
        {
            return Err(validation(format!(
                "Authored localization locale {locale_index} is structurally inconsistent"
            )));
        }
        locale_tags.insert(locale.donor_tag.0);
    }
    if locale_tags.len() != LOCALIZATION_LOCALE_COUNT
        || localization
            .locale_data
            .iter()
            .enumerate()
            .any(|(index, locale)| {
                u32::try_from(LOCALIZATION_LOCALE_COUNT - index)
                    .ok()
                    .and_then(|distance| localization.donor_header_tag.0.checked_sub(distance))
                    != Some(locale.donor_tag.0)
            })
    {
        return Err(validation(
            "Authored localization locale tags are not one-to-one children immediately before their header",
        ));
    }
    Ok(())
}

pub(super) fn validate_localized_text(description: &str, value: &str) -> AuthoringResult<()> {
    if value.trim().is_empty() {
        return Err(invalid(format!("{description} cannot be empty")));
    }
    if value.contains('\0') {
        return Err(invalid(format!(
            "{description} cannot contain a NUL character"
        )));
    }
    if value.chars().count() > usize::from(u16::MAX) {
        return Err(invalid(format!(
            "{description} exceeds the localized 16-bit character limit"
        )));
    }
    Ok(())
}

//! Native artifacts use equipment slot 0x0112 and can carry ordinary equipped perk sockets.
use super::*;

/// Item strings hold conditional localized notices here: 0x28-byte rows of a condition program,
/// a localized reference and a style key.
const CONDITIONAL_NOTICE_DESCRIPTOR: usize = 0xD0;
const CONDITIONAL_NOTICE_ROW_CLASS: u32 = 0x8080_2B22;
/// The definition byte naming a Seasonal Artifact's season, `FF` on every other item.
const SEASON_ORDINAL_OFFSET: usize = 0xA8;
/// The definition's index of that season's artifact progression, `FFFF` where absent.
const SEASON_PROGRESSION_OFFSET: usize = 0xDA;
/// The first key of the item strings' client classification tuple. Stock artifacts hold `ghost`
/// (`EFCD14BE`), as Ghost Shells do, and weapons and most other items hold the empty name.
const CLASSIFICATION_OFFSET: usize = 0xB8;
const GHOST_CLASSIFICATION: u32 = 0xEFCD_14BE;
/// FNV-1 of `armor` and `head`, the first two keys of a class-neutral helmet's tuple.
const ARMOR_CLASSIFICATION: u32 = 0x2778_5BD2;
const HEAD_CLASSIFICATION: u32 = 0xCD88_7E07;
/// The inventory block's byte after its instanced flag. It is 1 on artifacts, perk plugs and
/// quest steps and 0 on 99% of weapons and armor, which reads as the item staying on its
/// character.
const CHARACTER_BOUND_OFFSET: usize = 0xBC;
/// Item strings `+08` point at the item's actions (`80805D25`): a localized label for deleting
/// the item, a second reference, the hold time in seconds and a style index. 6,000 stock items
/// name Dismantle (`7B9E5F5F` in table 5) with style 19 in 4,230 of them, and artifacts name
/// nothing, so the Character screen offers them no Dismantle.
const ACTIONS_POINTER: usize = 0x08;
const ACTIONS_CLASS: u32 = 0x8080_5D25;
const DISMANTLE_LABEL: (u32, u32) = (5, 0x7B9E_5F5F);
const DISMANTLE_STYLE: u16 = 19;
/// Item strings `+10` point at a display record (`80805D1C`) whose first word is a style index:
/// `0A` on weapons, armor, mods and emblems, `6B` on artifacts and the seasonal plugs.
const DISPLAY_POINTER: usize = 0x10;
const DISPLAY_CLASS: u32 = 0x8080_5D1C;
const SEASONAL_DISPLAY_STYLE: u16 = 0x6B;
const GEAR_DISPLAY_STYLE: u16 = 0x0A;
/// Item strings `+18` point at a type record (`80805D1A`): an ammunition type and an item type
/// index, `18` to `1A` on armor by class, `20` on Ghost Shells and absent on functional
/// artifacts. The 28 class-neutral helmets hold `18`.
const TYPE_POINTER: usize = 0x18;
const TYPE_CLASS: u32 = 0x8080_5D1A;

/// The classification key and item type index stock helmets of a class hold: FNV-1 of
/// `armor_titan`, `armor_hunter` and `armor_warlock` with types `19`, `18` and `1A`, and
/// `armor` with `18` on the class-neutral helmets. The tooltip names the class a failed equip
/// condition requires from this key, so a class-neutral key leaves that line without a name.
fn display_class(class: Option<crate::ArmorClass>) -> (u32, u16) {
    match class {
        Some(crate::ArmorClass::Titan) => (0xD60E_6BA5, 0x19),
        Some(crate::ArmorClass::Hunter) => (0xD74E_BFB3, 0x18),
        Some(crate::ArmorClass::Warlock) => (0x82E1_5A90, 0x1A),
        _ => (ARMOR_CLASSIFICATION, 0x18),
    }
}

/// Definition `+48` points at an item's quality block: 0x70 bytes whose descriptors at these
/// offsets hold the infusion category (UInt32 rows), three level arrays and the version
/// (UInt16 rows). Functional artifacts have none, so the client finds no Power for them.
const ITEM_QUALITY_POINTER: usize =
    sundial::package_authoring::investment_schema::ITEM_QUALITY_BLOCK_POINTER_OFFSET;
const QUALITY_CLASS: u32 = 0x8080_7BF5;
const QUALITY_SIZE: usize = 0x70;
const QUALITY_ARRAYS: [(usize, u32, usize); 5] = [
    (0x10, 0x8080_0070, 4),
    (0x28, 0x8080_7BFB, 2),
    (0x38, 0x8080_7BFB, 2),
    (0x48, 0x8080_7BFB, 2),
    (0x60, 0x8080_5921, 2),
];
/// Item strings `+70` point at the stat display block, whose word at `+14` names the stat
/// group the client lists. Armor holds group 4: Mobility, Resilience, Recovery, Discipline,
/// Intellect and Strength. Functional artifacts hold no block, and the client's stat reader
/// then finds no group, so the inspection screen shows neither stats nor sockets for them.
const STAT_DISPLAY_POINTER: usize = 0x70;
const STAT_DISPLAY_CLASS: u32 = 0x8080_5CF1;
const STAT_DISPLAY_SIZE: usize = 0x18;
const ARMOR_STAT_GROUP: u32 = 4;
/// The stats a stock Legendary helmet declares with no amount of its own. Its plugs and its
/// quality supply the amounts.
const HELMET_STATS: [(u16, i32); 3] = [(0, 0), (1, 0), (2, 0)];
/// Item strings `+58` point at an eight-byte label reference (`80805D0B`) on every stock item
/// with sockets and on none without. The inspection screen's socket component lists an item's
/// sockets only when both that block and the definition's sockets resolve.
const SOCKET_LABEL_POINTER: usize = 0x58;
const SOCKET_LABEL_CLASS: u32 = 0x8080_5D0B;
const SOCKET_LABEL_SIZE: usize = 8;
const HELMET_BUCKET: u8 = 3;
const LEGENDARY_RARITY: u8 = 4;
const HUNTER_CLASSIFICATION: u32 = 0xD74E_BFB3;

/// What an imported artifact takes from a stock Legendary helmet of its class: the quality
/// block with its arrays, the stat display block and the socket label block.
#[derive(Clone)]
pub(in crate::item) struct Lender {
    quality: Vec<u8>,
    arrays: Vec<Vec<u8>>,
    display: Vec<u8>,
    label: Vec<u8>,
}

/// The first stock Legendary helmet of the artifact's class that declares the helmet stats
/// and shows the armor stat group. An unrestricted artifact takes a Hunter helmet, whose item
/// type index the class-neutral helmets share.
fn lender(sources: &ProjectSources, class: Option<crate::ArmorClass>) -> AuthoringResult<Lender> {
    let key = match display_class(class).0 {
        ARMOR_CLASSIFICATION => HUNTER_CLASSIFICATION,
        key => key,
    };
    if let Some(lender) = sources
        .artifact_lenders
        .lock()
        .map_err(|_| invalid("Artifact stat lenders are unavailable"))?
        .get(&key)
    {
        return Ok(lender.clone());
    }
    for index in 0..sources.stock_item_count {
        let row = sources.item_rows + index * ITEM_ROW_SIZE + 16;
        let definition = read_tag(
            &sources.manager,
            TagHash(read_u32(&sources.stock_item_table, row)?),
            "Stock item definition",
        )?;
        if definition.len() <= ITEM_RARITY_OFFSET
            || definition[ITEM_INVENTORY_SLOT_OFFSET] != HELMET_BUCKET
            || definition[ITEM_RARITY_OFFSET] != LEGENDARY_RARITY
            || read_i64(&definition, ITEM_QUALITY_POINTER)? == 0
            || read_i64(&definition, ITEM_INVESTMENT_STAT_POINTER_OFFSET)? == 0
        {
            continue;
        }
        let stats = relative_target(&definition, ITEM_INVESTMENT_STAT_POINTER_OFFSET)?;
        if read_u64(&definition, stats)? != HELMET_STATS.len() as u64 {
            continue;
        }
        let (count, _, rows, class) = array_at(&definition, stats)?;
        let mut declared = (0..count)
            .map(|row| {
                let at = rows + row * ITEM_INVESTMENT_STAT_ROW_SIZE;
                Ok((
                    u16::from(read_u8(&definition, at)?),
                    read_i32(&definition, at + 4)?,
                ))
            })
            .collect::<AuthoringResult<Vec<_>>>()?;
        declared.sort_unstable();
        if class != ITEM_INVESTMENT_STAT_ROW_CLASS || declared != HELMET_STATS {
            continue;
        }
        let strings = read_tag(
            &sources.manager,
            TagHash(read_u32(
                &sources.stock_item_strings,
                sources.string_rows + index * ITEM_ROW_SIZE + 16,
            )?),
            "Stock item strings",
        )?;
        if read_u32(&strings, CLASSIFICATION_OFFSET)? != key
            || read_i64(&strings, STAT_DISPLAY_POINTER)? == 0
            || read_i64(&strings, SOCKET_LABEL_POINTER)? == 0
        {
            continue;
        }
        let display = relative_target(&strings, STAT_DISPLAY_POINTER)?;
        let label = relative_target(&strings, SOCKET_LABEL_POINTER)?;
        if read_u32(&strings, display - 4)? != STAT_DISPLAY_CLASS
            || read_u32(&strings, display + 0x14)? != ARMOR_STAT_GROUP
            || read_u32(&strings, label - 4)? != SOCKET_LABEL_CLASS
        {
            continue;
        }
        let quality = relative_target(&definition, ITEM_QUALITY_POINTER)?;
        if read_u32(&definition, quality - 4)? != QUALITY_CLASS {
            return Err(invalid(
                "Stock helmet quality block has an unsupported class",
            ));
        }
        let mut arrays = Vec::new();
        for (field, expected, stride) in QUALITY_ARRAYS {
            let (count, _, rows, class) = array_at(&definition, quality + field)?;
            if class != expected || count == 0 {
                return Err(invalid(
                    "Stock helmet quality block has an unsupported array",
                ));
            }
            arrays.push(
                definition
                    .get(rows..rows + count * stride)
                    .ok_or_else(|| invalid("Stock helmet quality rows are truncated"))?
                    .to_vec(),
            );
        }
        let lender = Lender {
            quality: definition
                .get(quality..quality + QUALITY_SIZE)
                .ok_or_else(|| invalid("Stock helmet quality block is truncated"))?
                .to_vec(),
            arrays,
            display: strings
                .get(display..display + STAT_DISPLAY_SIZE)
                .ok_or_else(|| invalid("Stock helmet stat display block is truncated"))?
                .to_vec(),
            label: strings
                .get(label..label + SOCKET_LABEL_SIZE)
                .ok_or_else(|| invalid("Stock helmet socket label block is truncated"))?
                .to_vec(),
        };
        sources
            .artifact_lenders
            .lock()
            .map_err(|_| invalid("Artifact stat lenders are unavailable"))?
            .insert(key, lender.clone());
        return Ok(lender);
    }
    Err(invalid(
        "No stock Legendary helmet can lend the artifact its stat display",
    ))
}

/// Give the artifact a helmet's declared stats, quality block, stat display block and socket
/// label block, so the client reads its plugs' stats and its Power and lists its sockets as it
/// does for armor.
fn stat_display(
    definition: &mut Vec<u8>,
    strings: &mut Vec<u8>,
    lender: &Lender,
) -> AuthoringResult<()> {
    let stats = relative_target(definition, ITEM_INVESTMENT_STAT_POINTER_OFFSET)?;
    if read_u32(definition, stats - 4)? != ITEM_INVESTMENT_STAT_RESOURCE_CLASS {
        return Err(invalid(
            "Artifact investment stats have an unsupported class",
        ));
    }
    // The template's rows are its season's: stat 60 with an unlock condition. They go, with
    // their programs, before the helmet's rows arrive.
    let (count, _, rows, class) = array_at(definition, stats)?;
    if class != ITEM_INVESTMENT_STAT_ROW_CLASS {
        return Err(invalid(
            "Artifact investment stat rows have an unsupported class",
        ));
    }
    let seasonal = (0..count)
        .map(|row| read_u8(definition, rows + row * ITEM_INVESTMENT_STAT_ROW_SIZE).map(u16::from))
        .collect::<AuthoringResult<Vec<_>>>()?;
    set_weapon_stats(definition, &[], &seasonal)?;
    set_weapon_stats(definition, &HELMET_STATS, &[])?;
    if read_i64(definition, ITEM_QUALITY_POINTER)? != 0
        || read_i64(strings, STAT_DISPLAY_POINTER)? != 0
        || read_i64(strings, SOCKET_LABEL_POINTER)? != 0
    {
        return Err(invalid(
            "Artifact template already holds a quality, stat display or socket label block",
        ));
    }
    while !(definition.len() + 4).is_multiple_of(8) {
        definition.push(0);
    }
    definition.extend(QUALITY_CLASS.to_le_bytes());
    let quality = definition.len();
    definition.extend(&lender.quality);
    write_relative_pointer(definition, ITEM_QUALITY_POINTER, quality)?;
    for ((field, class, stride), rows) in QUALITY_ARRAYS.into_iter().zip(&lender.arrays) {
        while !(definition.len() + 4).is_multiple_of(16) {
            definition.push(0);
        }
        definition.extend(0x80809FBDu32.to_le_bytes());
        let header = definition.len();
        definition.extend(((rows.len() / stride) as u64).to_le_bytes());
        definition.extend(u64::from(class).to_le_bytes());
        definition.extend(rows);
        write_u64(definition, quality + field, (rows.len() / stride) as u64)?;
        write_relative_pointer(definition, quality + field + 8, header)?;
    }
    while !definition.len().is_multiple_of(8) {
        definition.push(0);
    }
    while !(strings.len() + 4).is_multiple_of(8) {
        strings.push(0);
    }
    strings.extend(STAT_DISPLAY_CLASS.to_le_bytes());
    let display = strings.len();
    strings.extend(&lender.display);
    write_relative_pointer(strings, STAT_DISPLAY_POINTER, display)?;
    while !(strings.len() + 4).is_multiple_of(8) {
        strings.push(0);
    }
    strings.extend(SOCKET_LABEL_CLASS.to_le_bytes());
    let label = strings.len();
    strings.extend(&lender.label);
    write_relative_pointer(strings, SOCKET_LABEL_POINTER, label)?;
    let length = strings.len() as u64;
    write_u64(strings, 0, length)
}

pub(in crate::item) fn prepare(
    sources: &ProjectSources,
    definition: &mut Vec<u8>,
    strings: &mut Vec<u8>,
    spec: &WeaponCloneSpec,
) -> AuthoringResult<()> {
    if spec.kind != ItemKind::Artifact {
        return Ok(());
    }
    native_slot(definition, ItemKind::Artifact)?;
    if read_i64(definition, 0x88)? != 0 {
        return Err(invalid(
            "Choose a functional artifact base without a display model",
        ));
    }
    if imported(spec) {
        // Imported equipment is immediately usable. The source talents carry its behavior,
        // while the seasonal donor's conditions, objectives and Power stat are independent.
        for (field, class) in [(0x20, 0x808077FC), (0x90, 0x808077AB)] {
            if read_i64(definition, field)? != 0 {
                let at = relative_target(definition, field)?;
                if read_u32(definition, at - 4)? != class {
                    return Err(invalid("Artifact seasonal record has an unsupported class"));
                }
                write_i64(definition, field, 0)?;
            }
        }
        stat_display(
            definition,
            strings,
            &lender(sources, spec.overrides.armor_class)?,
        )?;
        // The seven functional Seasonal Artifacts alone hold a season ordinal and a progression
        // index here, shared by the two artifacts of a season, and every other item holds the
        // absent values. They tie the item to its season's unlocks, Power bonus and experience
        // track, which the seasonal tooltip shows in place of the item's own perks.
        if definition.len() < SEASON_PROGRESSION_OFFSET + 2 {
            return Err(invalid("Artifact definition is truncated"));
        }
        definition[SEASON_ORDINAL_OFFSET] = u8::MAX;
        definition[CHARACTER_BOUND_OFFSET] = 0;
        categories(definition)?;
        write_u16(definition, SEASON_PROGRESSION_OFFSET, u16::MAX)?;
    }
    if read_i64(definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET)? == 0
        && (!spec.overrides.socket_columns.is_empty()
            || !spec.overrides.socket_plug_variants.is_empty())
    {
        while !(definition.len() + 4).is_multiple_of(8) {
            definition.push(0);
        }
        definition.extend(0x808077C1u32.to_le_bytes());
        let holder = definition.len();
        definition.resize(holder + 32, 0);
        write_relative_pointer(definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET, holder)?;
        // Resolution requires a typed empty array. Ordinary socket expansion appends its rows.
        while !(definition.len() + 4).is_multiple_of(16) {
            definition.push(0);
        }
        definition.extend(0x80809FBDu32.to_le_bytes());
        let header = definition.len();
        definition.extend(0u64.to_le_bytes());
        definition.extend(u64::from(ITEM_ORDINARY_SOCKET_ROW_CLASS).to_le_bytes());
        definition.extend(NESTED_ARRAY_TRAILER);
        write_relative_pointer(definition, holder + 8, header)?;
    }
    let length = definition.len() as u64;
    write_u64(definition, 0, length)
}

/// The definition's item categories: an array of UInt16 category rows at `+E0`. Helmets hold
/// `[0, 1B]`, class items `[4, 1B]`, weapons `1A` with a subtype row and Ghost Shells `23`,
/// and the eleven stock artifacts hold none. The client derives an item's type from these,
/// which the inspection screen and the gear tooltip test before they show armor or weapon
/// details, so imported artifacts take the helmet pair their classification already names.
const CATEGORIES_OFFSET: usize = 0xE0;
const CATEGORY_ROW_CLASS: u32 = 0x8080_2C50;
const HELMET_CATEGORIES: [u16; 2] = [0, 0x1B];

fn categories(definition: &mut Vec<u8>) -> AuthoringResult<()> {
    if read_u64(definition, CATEGORIES_OFFSET)? != 0 {
        return Err(invalid("Artifact template already holds item categories"));
    }
    while !(definition.len() + 4).is_multiple_of(16) {
        definition.push(0);
    }
    definition.extend(0x80809FBDu32.to_le_bytes());
    let header = definition.len();
    definition.extend((HELMET_CATEGORIES.len() as u64).to_le_bytes());
    definition.extend(u64::from(CATEGORY_ROW_CLASS).to_le_bytes());
    for category in HELMET_CATEGORIES {
        definition.extend(category.to_le_bytes());
    }
    while !definition.len().is_multiple_of(8) {
        definition.push(0);
    }
    write_u64(
        definition,
        CATEGORIES_OFFSET,
        HELMET_CATEGORIES.len() as u64,
    )?;
    write_relative_pointer(definition, CATEGORIES_OFFSET + 8, header)
}

/// The class an artifact recipe selects, or none where it leaves the base unrestricted.
pub(in crate::item) fn classes(spec: &WeaponCloneSpec) -> crate::collection::Classes {
    spec.overrides
        .armor_class
        .and_then(crate::ArmorClass::native_class)
        .map_or(
            crate::collection::Classes::ALL,
            crate::collection::Classes::one,
        )
}

/// Restrict the artifact with the class-flag condition stock subclasses carry in the same
/// equipment block. Armor's class-and-slot flags have no artifact slot.
pub(in crate::item) fn apply_class(
    definition: &mut Vec<u8>,
    selected: crate::ArmorClass,
) -> AuthoringResult<()> {
    *definition = sundial::package_authoring::investment_schema::set_artifact_equipment_class(
        definition,
        selected.native_class(),
    )
    .map_err(invalid)?;
    let length = definition.len() as u64;
    write_u64(definition, 0, length)
}

pub(in crate::item) fn class(definition: &[u8]) -> AuthoringResult<Option<u8>> {
    sundial::package_authoring::investment_schema::artifact_equipment_class(definition)
        .map_err(validation)
}

/// Imported artifacts inspect their own sockets through the ordinary native item screen.
pub(in crate::item) fn presentation(
    strings: &mut [u8],
    spec: &WeaponCloneSpec,
) -> AuthoringResult<()> {
    if spec.kind != ItemKind::Artifact || !imported(spec) {
        return Ok(());
    }
    let action = relative_target(strings, 0x38)?;
    if read_u32(strings, action - 4)? != 0x80805D10
        || read_u64(strings, action + 0x10)? != 0
        || read_u64(strings, action + 0x18)? != 0
    {
        return Err(invalid(
            "Artifact inspection action has an unsupported layout",
        ));
    }
    // A seasonal artifact shows a localized line while its own season's unlock flag is set.
    // 15,064 of 15,424 stock item strings carry no such row, so the import carries none.
    if read_u64(strings, CONDITIONAL_NOTICE_DESCRIPTOR)? != 0 {
        let (_, header, _, class) = array_at(strings, CONDITIONAL_NOTICE_DESCRIPTOR)?;
        if class != CONDITIONAL_NOTICE_ROW_CLASS {
            return Err(invalid(
                "Artifact conditional notice has an unsupported row class",
            ));
        }
        set_array_count(strings, CONDITIONAL_NOTICE_DESCRIPTOR, header, 0)?;
    }
    // The ordinary inspection screen takes its scene and its socket sections from this tuple.
    // `ghost` stages a Ghost and lists Ghost sockets, and the empty name lists no sockets at
    // all (both seen in game on an imported artifact). The talents sit on armor perk sockets,
    // which the armor screen lists, so the import takes the class-neutral armor pair that 28
    // stock helmets hold.
    let (classification, item_type) = display_class(spec.overrides.armor_class);
    if read_u32(strings, CLASSIFICATION_OFFSET)? == GHOST_CLASSIFICATION {
        write_u32(strings, CLASSIFICATION_OFFSET, classification)?;
        write_u32(strings, CLASSIFICATION_OFFSET + 4, HEAD_CLASSIFICATION)?;
    }
    // The import is ordinary gear: it can be dismantled, and it takes the display style and
    // item type of the class-neutral helmets whose classification it holds. The tooltip and
    // the inspection screen read their sections from these records.
    let actions = relative_target(strings, ACTIONS_POINTER)?;
    let display = relative_target(strings, DISPLAY_POINTER)?;
    let kind = relative_target(strings, TYPE_POINTER)?;
    if read_u32(strings, actions - 4)? != ACTIONS_CLASS
        || read_u32(strings, display - 4)? != DISPLAY_CLASS
        || read_u32(strings, kind - 4)? != TYPE_CLASS
    {
        return Err(invalid(
            "Artifact display records have an unsupported class",
        ));
    }
    if read_u32(strings, actions + 4)? == BLANK_LOCALIZED_REFERENCE_HASH {
        write_localized_reference(strings, actions, DISMANTLE_LABEL.0, DISMANTLE_LABEL.1)?;
        write_u16(strings, actions + 0x14, DISMANTLE_STYLE)?;
    }
    if read_u16(strings, display)? == SEASONAL_DISPLAY_STYLE {
        write_u16(strings, display, GEAR_DISPLAY_STYLE)?;
    }
    if read_u16(strings, kind + 2)? == u16::MAX {
        write_u16(strings, kind + 2, item_type)?;
    }
    if read_u32(strings, DISPLAY_STYLE_OFFSET)? == BLANK_LOCALIZED_REFERENCE_HASH {
        write_u32(
            strings,
            DISPLAY_STYLE_OFFSET,
            sundial::package_authoring::fnv1_name_hash(DISPLAY_STYLE),
        )?;
    }
    // Shadowkeep's item-screen router maps 66E18DD8 to ordinary gear inspection. The
    // seasonal action instead selects the vendor screen and carries a vendor row at +4.
    write_u32(strings, action, 0x66E1_8DD8)?;
    write_u16(strings, action + 4, u16::MAX)?;
    write_localized_reference(
        strings,
        action + 8,
        BLANK_LOCALIZED_REFERENCE_TABLE_INDEX,
        BLANK_LOCALIZED_REFERENCE_HASH,
    )
}

/// The character screen's widget table and its byte-identical twin. Both import the slot panel
/// `80BC6F77` twice, for the weapon slots and for the `artifacts` slot, and set property `218`
/// on the second copy's two item buttons: component `154`, the equipped item, and `185`, each
/// item of the slot's list. That property names the tooltip a focused button opens: here the
/// artifact tooltip `80BC6F8D`, exported under name `50211623`, which shows the season's unlock
/// rows for whatever item is hovered and has no perk rows or Dismantle prompt. The weapon copy
/// sets no such property and opens the ordinary gear tooltip, named `3BFD3F80`.
const CHARACTER_SCREENS: [u32; 2] = [0x80BC_6F93, 0x80BC_6F94];
/// The root tables that hold the screen with their hierarchies: the first table itself, and
/// two that import the twin at index zero, so the buttons keep their component numbers.
const SCREEN_ROOTS: [(u32, u32); 3] = [
    (0x80BC_6F93, 0x80B4_6E13),
    (0x80BC_6882, 0x80BC_630C),
    (0x80BC_6F65, 0x80B4_6DE5),
];
const EQUIPPED_BUTTON: u16 = 0x154;
const LISTED_BUTTON: u16 = 0x185;
const TOOLTIP_PROPERTY: u32 = 0x218;
const ARTIFACT_TOOLTIP: u32 = 0x5021_1623;
const GEAR_TOOLTIP: u32 = 0x3BFD_3F80;
/// Item strings `+CC` hold the item's display style, an FNV-1 name hash that a button exposes
/// at path `[400, 17, 2]`, field `21`. Stock table `80BC73D0` chooses a tooltip by comparing it
/// with `ui_display_style_infuse`. Stock artifacts hold the empty name, so imported artifacts
/// carry a style of their own for the character screen to tell them apart.
const DISPLAY_STYLE_OFFSET: usize = 0xCC;
const DISPLAY_STYLE: &str = "ui_display_style_authored_artifact";

/// Private copies of the character screen tables. The equipped artifact's button opens the
/// ordinary gear tooltip for an imported artifact, which is gear with perks of its own, and
/// the artifact tooltip for any other, through a decision on the item's display style. The
/// slot's listed items open the gear tooltip, where the Dismantle prompt is.
pub(in crate::item) fn screens(
    manager: &sundial::package_authoring::PackageManager,
) -> AuthoringResult<Vec<crate::ReplacementSpec>> {
    use sundial::package_authoring::ui;
    let read = |tag: u32, class: u32| -> AuthoringResult<Vec<u8>> {
        let tag = tiger_pkg::TagHash(tag);
        if manager
            .get_entry(tag)
            .is_none_or(|entry| entry.reference != class)
        {
            return Err(invalid(format!(
                "Character screen {tag} has an unsupported class"
            )));
        }
        manager
            .read_tag(tag)
            .map_err(|error| invalid(format!("Character screen {tag}: {error}")))
    };
    let mut tables = std::collections::BTreeMap::new();
    for tag in CHARACTER_SCREENS {
        let mut payload = read(tag, ui::widget_class())?;
        for button in [EQUIPPED_BUTTON, LISTED_BUTTON] {
            payload = ui::remove_property(&payload, button, TOOLTIP_PROPERTY).map_err(invalid)?;
        }
        tables.insert(tag, payload);
    }
    let switch = ui::NameSwitch {
        name: crate::presentation::text_hash("artifact", "tooltip"),
        condition: ui::Endpoint {
            component: EQUIPPED_BUTTON,
            path: vec![0x400, 0x17, 2],
            property: 0x21,
        },
        equals: sundial::package_authoring::fnv1_name_hash(DISPLAY_STYLE),
        when_true: GEAR_TOOLTIP,
        when_false: ARTIFACT_TOOLTIP,
        outputs: vec![ui::Endpoint {
            component: EQUIPPED_BUTTON,
            path: Vec::new(),
            property: TOOLTIP_PROPERTY,
        }],
    };
    let mut output = Vec::new();
    for (root, hierarchy) in SCREEN_ROOTS {
        let widget = match tables.remove(&root) {
            Some(widget) => widget,
            None => {
                let widget = read(root, ui::widget_class())?;
                if ui::imports(&widget).map_err(invalid)?.first()
                    != Some(&(CHARACTER_SCREENS[1], 0))
                {
                    return Err(invalid(
                        "Character screen root does not import the screen table first",
                    ));
                }
                widget
            }
        };
        let tree = read(hierarchy, ui::hierarchy_class())?;
        if sundial::package_authoring::native_payload::bytes_at::<4>(&tree, 28).map_err(invalid)?
            != root.to_le_bytes()
        {
            return Err(invalid(
                "The character screen hierarchy names a different widget table",
            ));
        }
        let (widget, tree) = ui::name_switch(&widget, &tree, &switch).map_err(invalid)?;
        output.push(crate::ReplacementSpec {
            tag: tiger_pkg::TagHash(root),
            payload: widget,
        });
        output.push(crate::ReplacementSpec {
            tag: tiger_pkg::TagHash(hierarchy),
            payload: tree,
        });
    }
    for (tag, payload) in tables {
        output.push(crate::ReplacementSpec {
            tag: tiger_pkg::TagHash(tag),
            payload,
        });
    }
    Ok(output)
}
/// The texture sizes of a stock icon lane that carries a large picture beside its tile: 104
/// stock icons hold a 96-pixel tile, a 60-pixel copy and a 460-pixel picture, which the
/// inspection screen draws behind an item that stages no model.
const BACKDROP_LANE: [(u16, u16); 3] = [(96, 96), (60, 60), (460, 460)];

/// The first stock icon row whose container holds that lane, the base icon's own rarity plate
/// and no other layer. An imported artifact with a backdrop paints its icon over a copy of it,
/// since its base's lane holds the tile alone.
pub(in crate::item) fn backdrop_icon(
    sources: &ProjectSources,
    item_index: usize,
    base: TagHash,
) -> AuthoringResult<ResolvedIconDonor> {
    use sundial::package_authoring::icon_schema::{
        ICON_BACKGROUND_LAYER_OFFSET, ICON_LAYER_REFERENCE_OFFSETS, ICON_PRIMARY_LAYER_OFFSET,
    };
    let unavailable = || invalid("Artifact backdrop icons are unavailable");
    if let Some(&(icon_index, icon_container)) = sources
        .backdrop_icons
        .lock()
        .map_err(|_| unavailable())?
        .get(&base.0)
    {
        return Ok(ResolvedIconDonor {
            item_index,
            icon_index,
            icon_container,
        });
    }
    let manager = &sources.manager;
    let plate = read_u32(
        &crate::icon::edit::read_icon_container(manager, base)?,
        ICON_BACKGROUND_LAYER_OFFSET,
    )?;
    for icon_index in 0..STOCK_ITEM_ICON_COUNT {
        let Ok(icon_index) = u16::try_from(icon_index) else {
            break;
        };
        let Ok(container) = stock_item_icon_container(&sources.stock_item_icons, icon_index) else {
            continue;
        };
        let Ok(payload) = crate::icon::edit::read_icon_container(manager, container) else {
            continue;
        };
        let mut other = false;
        for offset in ICON_LAYER_REFERENCE_OFFSETS {
            let layer = read_u32(&payload, offset)?;
            other |= match offset {
                ICON_PRIMARY_LAYER_OFFSET => false,
                ICON_BACKGROUND_LAYER_OFFSET => layer != plate,
                _ => layer != u32::MAX,
            };
        }
        if other {
            continue;
        }
        let layer = TagHash(read_u32(&payload, ICON_PRIMARY_LAYER_OFFSET)?);
        let Ok(lane) = manager.read_tag(layer) else {
            continue;
        };
        let Ok(textures) = crate::icon::edit::texture_reference_offsets(&lane, layer) else {
            continue;
        };
        let sizes = textures
            .iter()
            .map(|&(_, header)| {
                let header = manager.read_tag(header).ok()?;
                Some((read_u16(&header, 0x0E).ok()?, read_u16(&header, 0x10).ok()?))
            })
            .collect::<Option<Vec<_>>>();
        if sizes.as_deref() != Some(&BACKDROP_LANE) {
            continue;
        }
        sources
            .backdrop_icons
            .lock()
            .map_err(|_| unavailable())?
            .insert(base.0, (icon_index, container));
        return Ok(ResolvedIconDonor {
            item_index,
            icon_index,
            icon_container: container,
        });
    }
    Err(invalid(
        "No stock icon holds a large picture beside a tile with the artifact's rarity plate",
    ))
}

/// Whether the build holds an imported artifact, whose slot then takes the gear tooltip.
pub(in crate::item) fn imported_artifact(spec: &WeaponCloneSpec) -> bool {
    spec.kind == ItemKind::Artifact && imported(spec)
}

/// A functional artifact can lack a collectible. Other native artifact definitions supply
/// acquisition and the Artifacts branch without lending their item identity or gameplay.
pub(in crate::item) fn collectible(sources: &ProjectSources) -> AuthoringResult<usize> {
    for index in 0..sources.stock_collectible_count {
        if collectible_item_bucket(sources, index) != Some(49) {
            continue;
        }
        let row = sources.collectible_rows + index * COLLECTIBLE_ROW_SIZE;
        if !collection_unlock_index(&sources.stock_collectibles, row)
            .is_ok_and(|flag| flag < sources.stock_unlock_count)
        {
            continue;
        }
        let parents = template_presentation_parents(
            &sources.stock_nodes,
            &sources.stock_collectibles,
            index,
        )?;
        if crate::progression::gear_collection_page(&sources.stock_nodes, &parents).is_ok() {
            return Ok(index);
        }
    }
    Err(invalid(
        "No native artifact collectible can supply the artifact's acquisition structure",
    ))
}

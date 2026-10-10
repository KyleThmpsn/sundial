//! Independent staged subclass, ability and private-effect readback.

use super::*;

/// Match the original bank's native component bindings in the copy. A private allocation has
/// no stock bank ID, so it must be reached through the entity's actual references.
fn bank_pair(manager: &PackageManager, original: u32, copied: u32) -> (u32, u32) {
    use sundial::package_authoring::entity::{
        weapon_component_binding_hashes, weapon_component_bindings,
    };
    let stock_entity = manager.read_tag(tiger_pkg::TagHash(original)).unwrap();
    let copied_entity = manager.read_tag(tiger_pkg::TagHash(copied)).unwrap();
    let stock_bank = ability_modifier::entity_bank(&stock_entity)
        .unwrap()
        .unwrap_or_else(|| panic!("0x{original:08X} binds a bank"));
    let mut banks = BTreeSet::new();
    for binding in weapon_component_binding_hashes(&stock_entity).unwrap() {
        let stock = weapon_component_bindings(&stock_entity, binding).unwrap();
        let copy = weapon_component_bindings(&copied_entity, binding).unwrap();
        assert_eq!(stock.len(), copy.len());
        for (stock, copy) in stock.iter().zip(&copy) {
            if stock.owner_tag == stock_bank {
                banks.insert(copy.owner_tag);
            }
        }
    }
    assert_eq!(
        banks.len(),
        1,
        "the copied entity binds one bank: {banks:X?}"
    );
    (stock_bank, *banks.first().unwrap())
}

/// The third movement ability's airborne jumps row read back: it names a copy of its entity,
/// which binds a private copy of its bank holding the authored count in that row, and the stock
/// bank keeps its own. Returns what `readback.json` records of it.
pub(in super::super) fn read_back_jump_row(
    manager: &PackageManager,
    authored: &SubclassSummary,
    (entity, row): &(u32, ability_movement::RowLane),
    name: &str,
) -> serde_json::Value {
    let copied = *authored
        .entry_entities
        .get(&layout::MOVEMENT[2])
        .unwrap_or_else(|| panic!("{name}'s third movement ability names an entity"));
    assert_ne!(
        copied, *entity,
        "{name}'s third movement ability names a copy of its entity"
    );
    let (stock_bank, private) = bank_pair(manager, *entity, copied);
    assert_ne!(
        private, stock_bank,
        "{name}'s third movement ability binds a private copy of its bank"
    );
    let count = |bank: u32| {
        let payload = manager.read_tag(tiger_pkg::TagHash(bank)).unwrap();
        ability_movement::row_lane(&payload, (row.key, row.row, row.lane))
            .unwrap_or_else(|error| panic!("bank 0x{bank:08X}: {error}"))
            .stock
    };
    assert_eq!(
        count(private),
        ROW_AIRBORNE_JUMPS,
        "{name}'s private bank's row sets the authored airborne jumps"
    );
    assert_eq!(
        count(stock_bank),
        row.stock,
        "the stock bank's row keeps its airborne jumps"
    );
    serde_json::json!({
        "stock_entity": format!("0x{entity:08X}"),
        "copy_entity": format!("0x{copied:08X}"),
        "stock_bank": format!("0x{stock_bank:08X}"),
        "private_bank": format!("0x{private:08X}"),
        "key": format!("0x{:08X}", row.key),
        "row": row.row,
        "stock": row.stock,
        "authored": ROW_AIRBORNE_JUMPS,
    })
}

/// The first movement ability's airborne jumps read back: it names a copy of its entity, which
/// holds the authored count, and the stock entity keeps its own. Returns what `readback.json`
/// records of it.
pub(in super::super) fn read_back_movement(
    manager: &PackageManager,
    authored: &SubclassSummary,
    (entity, jumps): &(u32, ability_movement::MovementValue),
    name: &str,
) -> serde_json::Value {
    let copied = *authored
        .entry_entities
        .get(&layout::MOVEMENT[0])
        .unwrap_or_else(|| panic!("{name}'s first movement ability names an entity"));
    assert_ne!(
        copied, *entity,
        "{name}'s first movement ability names a copy of its entity"
    );
    let count = |graph: u32| {
        movement_value(manager, graph, jumps.label)
            .unwrap_or_else(|| panic!("0x{graph:08X} holds its airborne jumps"))
            .stock()
    };
    assert_eq!(
        count(copied),
        AIRBORNE_JUMPS,
        "{name}'s copy allows the authored airborne jumps"
    );
    assert_eq!(
        count(*entity),
        jumps.stock(),
        "the stock movement ability keeps its airborne jumps"
    );
    serde_json::json!({
        "stock_entity": format!("0x{entity:08X}"),
        "copy_entity": format!("0x{copied:08X}"),
        "stock": jumps.stock(),
        "authored": AIRBORNE_JUMPS,
    })
}

/// The first grenade's damage type read back: every damage profile its copy's graphs name is a
/// private copy dealing the authored type, and the stock grenade's profiles keep theirs. Returns
/// what `readback.json` records of it.
pub(in super::super) fn read_back_damage(
    manager: &PackageManager,
    (stock, copied): (u32, u32),
    damage: crate::recipe::RecipeDamageType,
    name: &str,
) -> serde_json::Value {
    let mode = crate::subclass::EntryEdits {
        damage_type: Some(damage),
        ..Default::default()
    }
    .damage_mode()
    .unwrap();
    let before = damage_profiles(manager, stock);
    let after = damage_profiles(manager, copied);
    assert!(
        !after.is_empty(),
        "{name}'s first grenade copy still names damage profiles"
    );
    assert!(
        after.iter().all(|(.., each)| *each == mode),
        "{name}'s first grenade copy deals {damage:?} through every damage profile it names"
    );
    let stock_profiles = before
        .iter()
        .map(|(_, tag, _)| *tag)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        after
            .iter()
            .all(|(_, tag, _)| !stock_profiles.contains(tag)),
        "{name}'s first grenade copy names private copies of its damage profiles"
    );
    assert!(
        before.iter().all(|(.., each)| *each != mode),
        "the stock first grenade keeps its own damage type"
    );
    let copies = after
        .iter()
        .map(|(_, tag, _)| *tag)
        .collect::<std::collections::BTreeSet<_>>();
    serde_json::json!({
        "damage_type": format!("{damage:?}"),
        "profile_mode": mode,
        "stock_profiles": stock_profiles.iter().map(|tag| format!("0x{tag:08X}")).collect::<Vec<_>>(),
        "copied_profiles": copies.iter().map(|tag| format!("0x{tag:08X}")).collect::<Vec<_>>(),
    })
}

/// The class ability's recharge read back: its pool applies the recharge key to its row, and its
/// bank has a numeric input row under that key. Returns what `readback.json` records of it.
pub(in super::super) fn read_back_recharge(
    manager: &PackageManager,
    staged: &InvestmentCatalog,
    authored: &SubclassSummary,
    name: &str,
) -> serde_json::Value {
    let class = layout::CLASS_ABILITIES[0];
    let row = *authored
        .entry_rows
        .get(&class)
        .unwrap_or_else(|| panic!("{name}'s class ability equips a row"));
    let key = ability_modifier::recharge_key(RECHARGE.to_bits());
    assert!(
        authored
            .entry_modifiers
            .get(&class)
            .is_some_and(|applied| applied.contains(&(key, row))),
        "{name}'s class ability applies its recharge key 0x{key:08X} to row {row}"
    );
    let bank = staged
        .ability_row(row)
        .and_then(|row| row.bank)
        .unwrap_or_else(|| panic!("{name}'s class ability row {row} reads a bank"));
    let payload = manager.read_tag(tiger_pkg::TagHash(bank)).unwrap();
    let rows = sundial::package_authoring::ability_bank::property_rows(&payload).unwrap();
    let found = rows
        .iter()
        .find(|each| each.key == key)
        .unwrap_or_else(|| panic!("bank 0x{bank:08X} has a row for the recharge key"));
    assert_eq!(
        found.modifier_class, 0x8080_451B,
        "the recharge row changes a numeric input"
    );
    serde_json::json!({
        "row": row,
        "bank": format!("0x{bank:08X}"),
        "key": format!("0x{key:08X}"),
        "multiplier": RECHARGE,
    })
}

/// The authored grenade's tint read back: at every place the stock grenade's effects draw the
/// stock color, the copy draws a private material whose constant holds the edited color, read
/// from the material or the constant buffer it names. Returns what `readback.json` records of
/// it.
pub(in super::super) fn read_back_tint(
    manager: &PackageManager,
    (stock, copied): (u32, u32),
    edit: TintEdit,
    name: &str,
) -> serde_json::Value {
    let stock_tints =
        ability_tint::ability_tints(manager, stock, crate::subclass::SPAWN_DEPTH).unwrap();
    let original = stock_tints
        .iter()
        .find(|tint| edit.starts_from(tint.rgb))
        .unwrap_or_else(|| panic!("the stock grenade still draws with its tint"));
    let expected = edit.apply(original.rgb);
    let twins = twins(manager, stock, copied);
    let read = |tag: u32| manager.read_tag(tiger_pkg::TagHash(tag)).unwrap();
    let float = |payload: &[u8], at: usize| {
        f32::from_le_bytes(payload[at..at + 4].try_into().unwrap()).to_bits()
    };
    for tint_use in &original.uses {
        let graph = *twins.get(&tint_use.graph).unwrap_or_else(|| {
            panic!("{name}'s copy has a twin of graph 0x{:08X}", tint_use.graph)
        });
        let site = ability_palette::particle_sites(manager, &read(graph))
            .unwrap()
            .into_iter()
            .find(|site| {
                (site.binding_hash, site.resource_index, site.offset)
                    == (
                        tint_use.site.binding_hash,
                        tint_use.site.resource_index,
                        tint_use.site.offset,
                    )
            })
            .unwrap_or_else(|| panic!("{name}'s graph 0x{graph:08X} keeps the tinted effect"));
        let system = read(site.system);
        let material = u32::from_le_bytes(system[0x14..0x18].try_into().unwrap());
        assert_ne!(
            material, tint_use.material,
            "{name}'s copy draws the tinted effect with a private material"
        );
        let material = read(material);
        let (payload, at) = match tint_use.store {
            ability_tint::ConstantStore::Inline => (material, tint_use.constant.offset),
            ability_tint::ConstantStore::External { .. } => {
                let header = u32::from_le_bytes(material[0x34C..0x350].try_into().unwrap());
                let data = manager
                    .get_entry(tiger_pkg::TagHash(header))
                    .unwrap_or_else(|| panic!("constant buffer 0x{header:08X} is live"))
                    .reference;
                (read(data), tint_use.constant.offset)
            }
        };
        let found = [0, 4, 8].map(|lane| float(&payload, at + lane));
        assert_eq!(
            found,
            expected.map(f32::to_bits),
            "{name}'s copy holds the recolored tint where the stock one was"
        );
    }
    serde_json::json!({
        "stock": original.rgb,
        "copy": expected,
        "uses": original.uses.len(),
        "hue": edit.hue,
        "colorize": edit.colorize,
    })
}

/// A grenade's grade read back: its copy's effects draw with none of the pixel programs the grade
/// applies to among those of `stocks`, the stock grenade and any projectile its copy fires in
/// place of a stock one, and with private ones that read as valid SM5 programs and write the
/// grade's color. Returns what `readback.json` records of it.
pub(in super::super) fn read_back_grade(
    manager: &PackageManager,
    (stocks, copied): (&[u32], u32),
    name: &str,
) -> serde_json::Value {
    let program = GRADE.program();
    let crate::dxbc::grade::Grade::Colorize(color) = program else {
        unreachable!("the test grade colorizes");
    };
    let stock_programs = stocks
        .iter()
        .flat_map(|stock| effect_programs(manager, *stock))
        .collect::<BTreeSet<_>>();
    let gradable = stock_programs
        .iter()
        .copied()
        .filter(|tag| {
            program_code(manager, *tag)
                .is_some_and(|code| crate::dxbc::grade::grade(&code, program).unwrap().is_some())
        })
        .collect::<BTreeSet<_>>();
    assert!(
        !gradable.is_empty(),
        "{name}'s stock effects draw with a program the grade applies to"
    );
    let copy_programs = effect_programs(manager, copied);
    assert!(
        copy_programs.is_disjoint(&gradable),
        "{name}'s copy 0x{copied:08X} draws with no stock program the grade applies to, but \
         still draws {:08X?}",
        copy_programs.intersection(&gradable).collect::<Vec<_>>()
    );
    let private = copy_programs
        .difference(&stock_programs)
        .copied()
        .collect::<Vec<_>>();
    // A material that multiplies its color takes only the hue, written as one minus it.
    let crate::dxbc::grade::Grade::Hue { hue, .. } = GRADE.neutral_program() else {
        unreachable!("the test grade colorizes");
    };
    let literal = |channels: [f32; 3]| {
        channels
            .iter()
            .map(|channel| channel.to_bits())
            .chain([0])
            .collect::<Vec<_>>()
    };
    let words = [literal(color), literal(hue.map(|channel| 1.0 - channel))];
    for tag in &private {
        let code = program_code(manager, *tag)
            .unwrap_or_else(|| panic!("{name}'s private program 0x{tag:08X} names its bytecode"));
        crate::dxbc::Program::read(&code)
            .unwrap_or_else(|error| panic!("private program 0x{tag:08X}: {error}"));
        let tokens = code
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert!(
            words
                .iter()
                .any(|words| tokens.windows(words.len()).any(|window| window == words)),
            "private program 0x{tag:08X} writes the grade's color"
        );
    }
    assert!(
        !private.is_empty(),
        "{name}'s copy draws with private graded programs"
    );
    serde_json::json!({
        "stock_programs": stock_programs.len(),
        "gradable": gradable.len(),
        "private_programs": private.len(),
        "hue": GRADE.hue,
        "colorize": GRADE.colorize,
    })
}

/// The authored grenade's effect colors read back: its copy draws every use of the stock palette
/// with a private palette of the taken palette's colors, edited, and the stock entity still draws
/// the stock one. Returns what `readback.json` records of it.
pub(in super::super) fn read_back_palette(
    manager: &PackageManager,
    (stock, copied): (u32, u32),
    edit: PaletteEdit,
    name: &str,
) -> serde_json::Value {
    let stock_palettes =
        ability_palette::ability_palettes(manager, stock, crate::subclass::SPAWN_DEPTH).unwrap();
    let copy_palettes =
        ability_palette::ability_palettes(manager, copied, crate::subclass::SPAWN_DEPTH).unwrap();
    let original = stock_palettes
        .iter()
        .find(|palette| palette.header == edit.palette)
        .unwrap_or_else(|| panic!("the stock grenade still draws with 0x{:08X}", edit.palette));
    let pixels = ability_palette::palette_pixels(manager, edit.palette).unwrap();
    let mut expected = ability_palette::palette_pixels(manager, edit.source()).unwrap();
    edit.apply(&mut expected);
    assert_ne!(expected, pixels, "the edit changes the palette's colors");
    assert!(
        copy_palettes
            .iter()
            .all(|palette| palette.header != edit.palette),
        "{name}'s copy no longer draws with the stock palette"
    );
    let recolored = copy_palettes
        .iter()
        .find(|palette| {
            ability_palette::palette_pixels(manager, palette.header).unwrap() == expected
        })
        .unwrap_or_else(|| panic!("{name}'s copy draws with the recolored palette"));
    assert_eq!(
        recolored.uses.len(),
        original.uses.len(),
        "{name}'s copy reaches the recolored palette everywhere the stock one was"
    );
    assert!(
        recolored
            .uses
            .iter()
            .all(|each| original.uses.iter().all(|stock| {
                stock.material != each.material && stock.site.system != each.site.system
            })),
        "{name}'s copy draws through private materials and particle systems"
    );
    serde_json::json!({
        "stock_palette": format!("0x{:08X}", edit.palette),
        "colors_from": edit.from.map(|from| format!("0x{from:08X}")),
        "copy_palette": format!("0x{:08X}", recolored.header),
        "uses": recolored.uses.len(),
        "hue": edit.hue,
        "saturation": edit.saturation,
        "brightness": edit.brightness,
    })
}

/// The retarget subclass's grenade names a copy of its entity, and every entry grants the perks it
/// starts with, its stock ones and the added one, with each naming the grenade replaced by a
/// private copy naming the copy. The stock subclass keeps its perks.
pub(in super::super) fn read_back_retarget(
    staged: &InvestmentCatalog,
    build: &BuildReport,
    authored: &AuthoredRetarget,
    packages: &Path,
) -> serde_json::Value {
    let name = &authored.recipe.name;
    let target = &authored.target;
    let report = build
        .weapons
        .iter()
        .find(|report| report.name == *name)
        .unwrap();
    let subclasses = staged.subclasses(|_| true);
    let own = subclasses
        .iter()
        .find(|subclass| subclass.hash == report.item_hash)
        .unwrap_or_else(|| panic!("{name} reads back as a subclass"));
    let stock = subclasses
        .iter()
        .find(|subclass| subclass.hash == target.base.hash)
        .expect("the retarget base stays installed");
    // A new subclass is for every class, as the New menu makes it.
    assert_eq!(
        own.class_type,
        if authored.recipe.overrides.subclass_every_class {
            3
        } else {
            target.base.class_type
        },
        "{name} takes the class its recipe gives it"
    );
    let copy = *own
        .entry_entities
        .get(&target.grenade)
        .unwrap_or_else(|| panic!("{name}'s grenade names an entity"));
    assert_ne!(
        copy, target.entity,
        "{name}'s grenade names a copy of its entity"
    );
    assert_eq!(
        stock.entry_entities.get(&target.grenade),
        Some(&target.entity),
        "the stock grenade keeps its entity"
    );
    let manager = open_shadowkeep_package_manager(packages).unwrap();
    let globals = investment_globals(&manager);
    let mut replaced = Vec::new();
    let entries = target
        .base
        .entry_perks
        .keys()
        .chain(&target.holders)
        .copied()
        .collect::<BTreeSet<_>>();
    for entry in entries {
        let mut perks = target
            .base
            .entry_perks
            .get(&entry)
            .cloned()
            .unwrap_or_default();
        assert_eq!(
            stock.entry_perks.get(&entry).cloned().unwrap_or_default(),
            perks,
            "the stock subclass keeps entry {entry}'s perks"
        );
        if target.holders.contains(&entry) {
            perks.push(target.perk);
        }
        let granted = own.entry_perks.get(&entry).cloned().unwrap_or_default();
        replaced.extend(
            check_retargeted(
                (&manager, &globals),
                (&perks, &granted),
                (target.entity, copy),
                &format!("{name}'s entry {entry}"),
            )
            .into_iter()
            .map(|(from, to)| (entry, from, to)),
        );
    }
    let mut expected = target
        .holders
        .iter()
        .map(|entry| (*entry, target.perk))
        .collect::<Vec<_>>();
    expected.sort_unstable();
    let mut found = replaced
        .iter()
        .map(|(entry, from, _)| (*entry, *from))
        .collect::<Vec<_>>();
    found.sort_unstable();
    assert_eq!(
        found, expected,
        "{name} replaces every perk that names its grenade"
    );
    serde_json::json!({
        "kind": ItemKind::Subclass,
        "name": name,
        "item_hash": format!("0x{:08X}", report.item_hash),
        "base": format!("{} 0x{:08X}", target.base.name, target.base.hash),
        "grenade": target.base.entry_names.get(&target.grenade),
        "value": authored.value.name,
        "naming_perk": target.perk,
        "stock_entity": format!("0x{:08X}", target.entity),
        "copy_entity": format!("0x{copy:08X}"),
        "retargeted": replaced
            .iter()
            .map(|(entry, from, to)| serde_json::json!({
                "entry": entry,
                "stock_perk": from,
                "private_perk": to,
            }))
            .collect::<Vec<_>>(),
    })
}

/// The authored subclass's screen art read back: its strings name a container of its own, whose
/// top picture is the painted one, whose bottom is the base's own and whose middle is the
/// attunement subclass's top picture. Returns what `readback.json` records of it.
pub(in super::super) fn read_back_screen_art(
    staged: &InvestmentCatalog,
    (item, subclass): (u32, &AuthoredSubclass),
    packages: &Path,
) -> serde_json::Value {
    let name = &subclass.recipe.name;
    let manager = open_shadowkeep_package_manager(packages).unwrap();
    let pictures = |container: u32| {
        let payload =
            crate::icon_edit::read_icon_container(&manager, tiger_pkg::TagHash(container)).unwrap();
        let layer = tiger_pkg::TagHash(u32::from_le_bytes(payload[0x14..0x18].try_into().unwrap()));
        let layer_payload = manager.read_tag(layer).unwrap();
        crate::icon_edit::texture_reference_offsets(&layer_payload, layer)
            .unwrap()
            .into_iter()
            .map(|(_, header)| header)
            .collect::<Vec<_>>()
    };
    let container = staged
        .nameplate_container(item)
        .unwrap_or_else(|| panic!("{name} names screen art in its strings"));
    let base_container = staged.nameplate_container(subclass.base.hash).unwrap();
    assert_ne!(
        container, base_container,
        "{name} has screen art of its own"
    );
    let own = pictures(container);
    let base = pictures(base_container);
    let taken = pictures(
        staged
            .nameplate_container(subclass.attunement.hash)
            .unwrap(),
    );
    assert_eq!(
        own.len(),
        3,
        "{name}'s screen art keeps one picture per attunement"
    );
    assert_ne!(own[0], base[0], "{name}'s top picture is a painted copy");
    let painted = crate::icon_edit::decode_texture(&manager, own[0]).unwrap();
    assert_eq!(
        painted
            .get_pixel(painted.width() / 2, painted.height() / 2)
            .0,
        SCREEN_ART_COLOR,
        "{name}'s top picture holds the picture"
    );
    assert_eq!(own[1], base[1], "{name} keeps its base's bottom picture");
    assert_eq!(
        own[2], taken[0],
        "{name}'s middle picture is the attunement subclass's top one"
    );
    serde_json::json!({
        "container": format!("0x{container:08X}"),
        "base_container": format!("0x{base_container:08X}"),
        "pictures": own.iter().map(|tag| format!("{tag}")).collect::<Vec<_>>(),
    })
}

/// The authored grenade read back: its own text and icon, its perks, its modifiers, and its entity
/// copy with the edited value and the spawned graph it changes. Perks of the list that name the
/// grenade name the copy. Returns what `readback.json` records of it.
pub(in super::super) fn read_back_ability(
    (stock, staged): (&InvestmentCatalog, &InvestmentCatalog),
    authored: &SubclassSummary,
    subclass: &AuthoredSubclass,
    packages: &Path,
) -> serde_json::Value {
    let name = &subclass.recipe.name;
    let base = &subclass.base;
    let ability = layout::GRENADES[1];
    let from = AttunementPath::Bottom.entries();
    let authored_node = AttunementPath::Middle.entries()[usize::from(NODE_POSITION)];
    // The authored grenade's pool equips an ability row of its own, whose pattern names a copy of
    // the stock entity with the edited value. A perk of the list that names the stock entity
    // names the copy through a private copy of its own.
    let value = &subclass.ability_value;
    let copied = *authored
        .entry_entities
        .get(&ability)
        .unwrap_or_else(|| panic!("{name}'s authored grenade names an entity"));
    assert_ne!(
        copied, value.entity,
        "{name}'s authored grenade names a copy of its entity"
    );
    let manager = open_shadowkeep_package_manager(packages).unwrap();
    let globals = investment_globals(&manager);
    let moved = (value.entity, copied);
    // Follow the entity's bank and its native parameter descriptor independently of the
    // authoring reader. The table has 16-byte records: name, reset, applied and add flag.
    let (stock_bank, private_bank) = bank_pair(&manager, value.entity, copied);
    let default_of = |tag| {
        let bank = manager.read_tag(tiger_pkg::TagHash(tag)).unwrap();
        let word = |at| u32::from_le_bytes(bank[at..at + 4].try_into().unwrap());
        let wide = |at| u64::from_le_bytes(bank[at..at + 8].try_into().unwrap());
        let pointer = |at: usize| {
            (at as i64 + i64::from_le_bytes(bank[at..at + 8].try_into().unwrap())) as usize
        };
        let instance = pointer(0x18);
        let count = wide(instance + 0x98) as usize;
        let header = pointer(instance + 0xA0);
        assert_eq!(word(header + 8), 0x8080_4519);
        (0..count)
            .find_map(|index| {
                let row = header + 16 + index * 16;
                (word(row) == subclass.parameter.0).then(|| f32::from_bits(word(row + 4)))
            })
            .expect("the private bank contains the authored parameter")
    };
    assert_eq!(
        default_of(private_bank),
        0.375,
        "the persisted parameter default reaches the private bank"
    );
    assert_ne!(
        default_of(stock_bank),
        0.375,
        "the stock parameter default stays unchanged"
    );
    let mut node_perks = base.entry_perks.get(&from[1]).cloned().unwrap_or_default();
    node_perks.push(subclass.extra_perk);
    let mut retargeted = check_retargeted(
        (&manager, &globals),
        (
            &node_perks,
            &authored
                .entry_perks
                .get(&authored_node)
                .cloned()
                .unwrap_or_default(),
        ),
        moved,
        &format!("{name}'s authored node, from its source's perks and the added one,"),
    );
    // The authored grenade: its own text and the chosen icon, its stock perks less the one its
    // copy replaced, then its two custom perks. The first custom perk is a sandbox perk of its
    // own. The copy is one too, or the stock row again when that perk has no runtime of its own.
    assert_eq!(
        authored
            .entry_descriptions
            .get(&ability)
            .map(String::as_str),
        Some(ABILITY_DESCRIPTION),
        "{name}'s authored grenade has its own description"
    );
    assert_eq!(
        authored.entry_icons.get(&ability),
        subclass.attunement.entry_icons.get(&ABILITY_ICON_ENTRY),
        "{name}'s authored grenade shows the icon it takes"
    );
    let ability_perks = base
        .entry_perks
        .get(&ability)
        .into_iter()
        .flatten()
        .copied()
        .filter(|perk| *perk != subclass.extra_perk)
        .collect::<Vec<_>>();
    let granted = authored
        .entry_perks
        .get(&ability)
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        granted.len(),
        ability_perks.len() + 2,
        "{name}'s authored grenade grants its perks and its two custom perks: {granted:?}"
    );
    retargeted.extend(check_retargeted(
        (&manager, &globals),
        (&ability_perks, &granted[..ability_perks.len()]),
        moved,
        &format!("{name}'s authored grenade"),
    ));
    let own = |perk: u16| {
        !stock.subclasses(|_| true).iter().any(|stock| {
            stock
                .entry_perks
                .values()
                .flatten()
                .any(|each| *each == perk)
        })
    };
    let (custom, copy) = (
        granted[ability_perks.len()],
        granted[ability_perks.len() + 1],
    );
    assert!(
        custom != subclass.custom_effect && own(custom),
        "{name}'s custom perk {custom} is a sandbox perk of its own, not stock {}",
        subclass.custom_effect
    );
    assert!(
        copy == subclass.extra_perk || (copy != custom && own(copy)),
        "{name}'s copy of perk {} grants {copy}",
        subclass.extra_perk
    );
    let modifiers = read_back_modifiers(&manager, staged, authored, subclass);
    // The stock grenade keeps its entity and value, and the copy carries the edited value.
    let stock_base = staged
        .subclasses(|_| true)
        .into_iter()
        .find(|staged| staged.hash == base.hash)
        .expect("the base subclass stays installed");
    assert_eq!(
        stock_base.entry_entities.get(&ability),
        Some(&value.entity),
        "the stock grenade keeps its entity"
    );
    let mut locator = value.edit.locator.clone();
    locator.graph_tag = None;
    let read = |entity: u32| {
        let payload = manager.read_tag(tiger_pkg::TagHash(entity)).unwrap();
        resolve_weapon_runtime_field(&manager, &payload, &locator)
            .unwrap_or_else(|error| panic!("{} of 0x{entity:08X}: {error}", value.name))
            .field
            .value
    };
    assert_eq!(
        read(copied),
        value.edit.value,
        "{name}'s copy carries the edited {}",
        value.name
    );
    assert_eq!(
        read(value.entity),
        value.stock,
        "the stock entity keeps its {}",
        value.name
    );
    let spawn = read_back_spawn(&manager, copied, &subclass.spawn_value, name);
    let swap = read_back_swap(&manager, authored, subclass.swap, name);
    let recharge = read_back_recharge(&manager, staged, authored, name);
    let movement = read_back_movement(&manager, authored, &subclass.movement, name);
    let jump_row = read_back_jump_row(&manager, authored, &subclass.jump_row, name);
    let bank = read_back_bank(&manager, authored, &subclass.bank_value, name);
    let table = read_back_table_palette(&manager, authored, subclass, name);
    let palette = read_back_palette(&manager, (value.entity, copied), subclass.palette, name);
    let tint = read_back_tint(&manager, (value.entity, copied), subclass.tint, name);
    let first = *authored
        .entry_entities
        .get(&layout::GRENADES[0])
        .unwrap_or_else(|| panic!("{name}'s first grenade names an entity"));
    let grade = read_back_grade(&manager, (&[subclass.bank_value.0], first), name);
    let damage = read_back_damage(
        &manager,
        (subclass.bank_value.0, first),
        subclass.damage_type,
        name,
    );
    let authored_grade = read_back_grade(&manager, (&[value.entity], copied), name);
    let third = *authored
        .entry_entities
        .get(&layout::GRENADES[2])
        .unwrap_or_else(|| panic!("{name}'s third grenade names an entity"));
    let swapped_grade = read_back_grade(
        &manager,
        (&[subclass.swap.graph, subclass.swap.replacement], third),
        name,
    );
    serde_json::json!({
        "name": ABILITY_NAME,
        "perks": granted,
        "custom_perk": custom,
        "custom_perk_copies": subclass.custom_effect,
        "edited_stock_perk": subclass.extra_perk,
        "edited_stock_perk_row": copy,
        "modifiers": modifiers,
        "parameter_default": {
            "name": format!("0x{:08X}", subclass.parameter.0),
            "stock_bank": format!("0x{stock_bank:08X}"),
            "private_bank": format!("0x{private_bank:08X}"),
            "stock": default_of(stock_bank),
            "authored": default_of(private_bank),
        },
        "retargeted": retargeted,
        "spawn_value": spawn,
        "swap": swap,
        "bank": bank,
        "table_palette": table,
        "recharge": recharge,
        "movement": movement,
        "jump_row": jump_row,
        "palette": palette,
        "tint": tint,
        "damage": damage,
        "grades": {
            "first": grade,
            "authored": authored_grade,
            "swapped": swapped_grade,
        },
        "ability_value": {
            "field": value.name,
            "stock_entity": format!("0x{:08X}", value.entity),
            "copy_entity": format!("0x{copied:08X}"),
            "stock": format!("{:?}", value.stock),
            "authored": format!("{:?}", value.edit.value),
        },
    })
}

/// The authored grenade's extra charges and parameter value, and the node's charge on the other
/// grenade, read back: pool records on each target's row whose keys name the bank rows the build
/// added. Returns what `readback.json` records of them.
pub(in super::super) fn read_back_modifiers(
    manager: &PackageManager,
    staged: &InvestmentCatalog,
    authored: &SubclassSummary,
    subclass: &AuthoredSubclass,
) -> serde_json::Value {
    let name = &subclass.recipe.name;
    let ability = layout::GRENADES[1];
    let authored_node = AttunementPath::Middle.entries()[usize::from(NODE_POSITION)];
    // Its extra charges and parameter value reach its own row, the copy's, as pool records whose
    // keys name bank rows the build added. The node's charge reaches the other grenade the same
    // way, on that grenade's copy row.
    let row = *authored
        .entry_rows
        .get(&ability)
        .unwrap_or_else(|| panic!("{name}'s authored grenade equips a row"));
    let definition = staged
        .ability_row(row)
        .unwrap_or_else(|| panic!("{name}'s grenade row {row} reads back"));
    let (parameter, parameter_value) = subclass.parameter;
    let entity = definition
        .entity
        .unwrap_or_else(|| panic!("{name}'s grenade row names an entity"));
    let (stock_bank, bank_tag) = bank_pair(manager, subclass.ability_value.entity, entity);
    let payload = manager.read_tag(tiger_pkg::TagHash(bank_tag)).unwrap();
    let rows = sundial::package_authoring::ability_bank::property_rows(&payload).unwrap();
    let charges_key = ability_modifier::charge_key(2);
    // The persisted key is derived from its original bank before that bank gets a private ID.
    let parameter_key =
        ability_modifier::parameter_key(stock_bank, parameter, parameter_value.to_bits(), false);
    let applied = authored
        .entry_modifiers
        .get(&ability)
        .cloned()
        .unwrap_or_default();
    for key in [charges_key, parameter_key] {
        assert!(
            applied.contains(&(key, row)),
            "{name}'s authored grenade applies key 0x{key:08X} to its row {row}: {applied:08X?}"
        );
    }
    let key_row = |key: u32| {
        rows.iter()
            .find(|each| each.key == key)
            .unwrap_or_else(|| panic!("bank 0x{bank_tag:08X} has a row for key 0x{key:08X}"))
    };
    assert_eq!(
        key_row(charges_key).charge,
        Some(2),
        "the charge row adds two"
    );
    assert_eq!(
        key_row(parameter_key)
            .parameters
            .iter()
            .map(|each| (each.name, each.applied, each.add))
            .collect::<Vec<_>>(),
        [(parameter, parameter_value, false)],
        "the parameter row sets the parameter"
    );
    let other_row = *subclass
        .grenade
        .entry_rows
        .get(&layout::GRENADES[0])
        .expect("the taken grenade equips a row");
    let node_key = ability_modifier::charge_key(1);
    // The taken grenade's value gives it an entity and a row of its own, which the node's charge
    // reaches in place of the stock row, or the copy the subclass equips would go without it.
    let copy_row = *authored
        .entry_rows
        .get(&layout::GRENADES[0])
        .expect("the taken grenade's copy equips a row");
    assert_ne!(
        copy_row, other_row,
        "the taken grenade equips a row of its own"
    );
    assert!(
        authored
            .entry_modifiers
            .get(&authored_node)
            .is_some_and(|applied| applied.contains(&(node_key, copy_row))),
        "{name}'s authored node gives the taken grenade's copy its charge on row {copy_row}"
    );
    assert!(
        staged.ability_row(other_row).is_some_and(|other| other
            .keys
            .iter()
            .any(|each| each.key == node_key && each.charges == Some(1))),
        "the taken grenade's bank has a row for one charge"
    );
    serde_json::json!({
        "extra_charges": {"row": row, "key": format!("0x{charges_key:08X}")},
        "parameter": {
            "name": format!("0x{parameter:08X}"),
            "value": parameter_value,
            "key": format!("0x{parameter_key:08X}"),
        },
    })
}

/// The third grenade's swap read back: it names a copy of its entity, which names no stock
/// replaced projectile and names a private copy of the replacement, byte equal to it under a tag
/// of its own. Returns what `readback.json` records of it.
pub(in super::super) fn read_back_swap(
    manager: &PackageManager,
    authored: &SubclassSummary,
    swap: SpawnSwap,
    name: &str,
) -> serde_json::Value {
    let copied = *authored
        .entry_entities
        .get(&layout::GRENADES[2])
        .unwrap_or_else(|| panic!("{name}'s third grenade names an entity"));
    assert_ne!(
        copied, swap.graph,
        "{name}'s third grenade names a copy of its entity"
    );
    let payload = manager.read_tag(tiger_pkg::TagHash(copied)).unwrap();
    let named = ability_spawns::spawned_graphs(manager, copied, &payload).unwrap();
    assert!(
        !named.contains(&swap.replaced),
        "{name}'s copy no longer names projectile 0x{:08X}",
        swap.replaced
    );
    // Every place the stock grenade named the replaced projectile names one private copy of the
    // replacement, which the grade may have recolored below it.
    let place =
        |spawn: &ability_spawns::Spawn| (spawn.binding_hash, spawn.resource_index, spawn.offset);
    let stock = manager.read_tag(tiger_pkg::TagHash(swap.graph)).unwrap();
    let places = ability_spawns::spawns(manager, swap.graph, &stock)
        .unwrap()
        .iter()
        .filter(|spawn| spawn.graph == swap.replaced)
        .map(place)
        .collect::<BTreeSet<_>>();
    assert!(
        !places.is_empty(),
        "the stock grenade names the replaced projectile"
    );
    let named_there = ability_spawns::spawns(manager, copied, &payload)
        .unwrap()
        .iter()
        .filter(|spawn| places.contains(&place(spawn)))
        .map(|spawn| spawn.graph)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        named_there.len(),
        1,
        "{name}'s copy names one projectile where the stock one named 0x{:08X}",
        swap.replaced
    );
    let private = *named_there.first().unwrap();
    assert!(
        private != swap.replacement && private != swap.replaced,
        "{name}'s copy names a private copy of projectile 0x{:08X}",
        swap.replacement
    );
    let replacement = manager
        .read_tag(tiger_pkg::TagHash(swap.replacement))
        .unwrap();
    let copy = manager.read_tag(tiger_pkg::TagHash(private)).unwrap();
    assert!(
        copy.len() == replacement.len() && copy.get(0x96) == Some(&18),
        "{name}'s private projectile 0x{private:08X} is a copy of 0x{:08X}",
        swap.replacement
    );
    // The swap's own damage type reaches every damage profile below the private projectile,
    // through private copies, while the stock projectile and the grenade's own graphs keep
    // their types.
    let damage = swap.damage_type.map(|damage| {
        let mode = crate::subclass::damage_mode(damage);
        let stock = damage_profiles(manager, swap.replacement);
        let retyped = damage_profiles(manager, private);
        assert!(
            !retyped.is_empty(),
            "{name}'s private projectile names damage profiles"
        );
        assert!(
            retyped.iter().all(|(.., each)| *each == mode),
            "{name}'s private projectile deals {damage:?} through every damage profile it names"
        );
        let stock_tags = stock.iter().map(|(_, tag, _)| *tag).collect::<BTreeSet<_>>();
        assert!(
            retyped.iter().all(|(_, tag, _)| !stock_tags.contains(tag)),
            "{name}'s private projectile names private copies of its damage profiles"
        );
        assert!(
            stock.iter().all(|(.., each)| *each != mode),
            "the stock projectile keeps its own damage type"
        );
        let own = damage_profiles(manager, copied)
            .into_iter()
            .filter(|(graph, ..)| {
                !retyped.iter().any(|(retyped_graph, ..)| retyped_graph == graph)
            })
            .collect::<Vec<_>>();
        assert!(
            own.iter().all(|(.., each)| *each != mode),
            "{name}'s third grenade keeps its own damage type outside the swapped projectile"
        );
        serde_json::json!({
            "damage_type": format!("{damage:?}"),
            "profile_mode": mode,
            "retyped_profiles": retyped.iter().map(|(_, tag, _)| format!("0x{tag:08X}")).collect::<BTreeSet<_>>(),
        })
    });
    serde_json::json!({
        "stock_entity": format!("0x{:08X}", swap.graph),
        "copy_entity": format!("0x{copied:08X}"),
        "replaced": format!("0x{:08X}", swap.replaced),
        "replacement": format!("0x{:08X}", swap.replacement),
        "private_copy": format!("0x{private:08X}"),
        "damage": damage,
    })
}

/// The first grenade's copy read back: it binds a private copy of its bank in place of the stock
/// one. The copy validates as a bank under its own tag, holds every row the build gave the stock
/// bank in the same order, the node's charge among them, lays out its blocks as the staged stock
/// bank does for every offset the entity names, and names a private copy of the graph only the
/// bank names, holding the edited value. Returns what `readback.json` records of it.
/// The first grenade's palette that only an impact table reaches, read back: as
/// `read_back_palette` checks, its copy draws a recolored palette everywhere the stock one was,
/// through private materials and particle systems, and the copy's graphs name an impact table the
/// stock grenade's do not, a private copy. Returns what `readback.json` records of it.
pub(in super::super) fn read_back_table_palette(
    manager: &PackageManager,
    authored: &SubclassSummary,
    subclass: &AuthoredSubclass,
    name: &str,
) -> serde_json::Value {
    let stock = subclass.bank_value.0;
    let copied = *authored
        .entry_entities
        .get(&layout::GRENADES[0])
        .unwrap_or_else(|| panic!("{name}'s first grenade names an entity"));
    let palette = read_back_palette(manager, (stock, copied), subclass.table_palette, name);
    let tables = |entity: u32| {
        ability_palette::ability_graphs(manager, entity, crate::subclass::SPAWN_DEPTH)
            .unwrap()
            .into_iter()
            .flat_map(|(graph, payload)| ability_spawns::tables(manager, graph, &payload).unwrap())
            .map(|place| place.graph)
            .collect::<BTreeSet<_>>()
    };
    let stock_tables = tables(stock);
    let private = tables(copied)
        .difference(&stock_tables)
        .copied()
        .collect::<Vec<_>>();
    assert!(
        !private.is_empty(),
        "{name}'s first grenade reaches its recolored graphs through a private impact table"
    );
    for table in &private {
        assert!(
            ability_spawns::is_table(manager, *table),
            "0x{table:08X} is an impact table"
        );
    }
    serde_json::json!({
        "palette": palette,
        "stock_tables": stock_tables.len(),
        "private_tables": private.iter().map(|tag| format!("0x{tag:08X}")).collect::<Vec<_>>(),
    })
}

pub(in super::super) fn read_back_bank(
    manager: &PackageManager,
    authored: &SubclassSummary,
    (entity, bank, value): &(u32, u32, AbilityValue),
    name: &str,
) -> serde_json::Value {
    use sundial::package_authoring::ability_bank::{bank_owner, property_rows, validate};
    use sundial::package_authoring::entity::{
        weapon_component_binding_hashes, weapon_component_bindings,
    };
    let copied = *authored
        .entry_entities
        .get(&layout::GRENADES[0])
        .unwrap_or_else(|| panic!("{name}'s first grenade names an entity"));
    assert_ne!(
        copied, *entity,
        "{name}'s first grenade names a copy of its entity"
    );
    let stock_entity = manager.read_tag(tiger_pkg::TagHash(*entity)).unwrap();
    let copied_entity = manager.read_tag(tiger_pkg::TagHash(copied)).unwrap();
    let mut private = BTreeSet::new();
    for binding in weapon_component_binding_hashes(&stock_entity).unwrap() {
        let stock = weapon_component_bindings(&stock_entity, binding).unwrap();
        let copy = weapon_component_bindings(&copied_entity, binding).unwrap();
        assert_eq!(
            stock.len(),
            copy.len(),
            "binding 0x{binding:08X} keeps its resources"
        );
        for (stock, copy) in stock.iter().zip(&copy) {
            if stock.owner_tag == *bank {
                private.insert(copy.owner_tag);
            }
        }
    }
    assert_eq!(
        private.len(),
        1,
        "{name}'s first grenade binds one bank: {private:X?}"
    );
    let private = *private.first().unwrap();
    assert_ne!(
        private, *bank,
        "{name}'s first grenade binds a private copy of its bank"
    );
    let copy = manager.read_tag(tiger_pkg::TagHash(private)).unwrap();
    validate(&copy).unwrap_or_else(|error| panic!("{name}'s private bank: {error}"));
    assert_eq!(
        bank_owner(&copy).unwrap(),
        private,
        "the private bank names itself"
    );
    let rows = |payload: &[u8]| {
        property_rows(payload)
            .unwrap()
            .iter()
            .map(|row| (row.key, row.handler.get(), row.modifier_class))
            .collect::<Vec<_>>()
    };
    let staged = manager.read_tag(tiger_pkg::TagHash(*bank)).unwrap();
    assert_eq!(
        rows(&copy),
        rows(&staged),
        "{name}'s private bank holds every row the build gave the stock bank"
    );
    // An entity names a bank's blocks by tag, class and offset. The copy names its bank's blocks
    // where the stock grenade names the stock bank's, both moved for the rows they took: the
    // same places, classes and offsets, and the same words at each, the bank's own tag aside.
    let word =
        |payload: &[u8], at: usize| u32::from_le_bytes(payload[at..at + 4].try_into().unwrap());
    let tuples = |payload: &[u8], owner: u32| {
        (0..payload.len().saturating_sub(15))
            .step_by(8)
            .filter(|at| {
                word(payload, *at) == owner
                    && (0x8080_0000..=0x8080_FFFF).contains(&word(payload, at + 4))
            })
            .map(|at| {
                let offset = u64::from_le_bytes(payload[at + 8..at + 16].try_into().unwrap());
                (at, word(payload, at + 4), offset)
            })
            .collect::<Vec<_>>()
    };
    let named = tuples(&copied_entity, private);
    assert!(
        !named.is_empty(),
        "{name}'s first grenade names its private bank's blocks"
    );
    assert_eq!(
        named,
        tuples(&stock_entity, *bank),
        "{name}'s first grenade names its private bank's blocks where the stock grenade names the stock bank's"
    );
    let block = |payload: &[u8], owner: u32, offset: u64| {
        usize::try_from(offset)
            .ok()
            .filter(|offset| offset + 8 <= payload.len())
            .map(|offset| {
                [word(payload, offset), word(payload, offset + 4)]
                    .map(|each| if each == owner { 0 } else { each })
            })
    };
    for (at, _, offset) in &named {
        assert_eq!(
            block(&copy, private, *offset),
            block(&staged, *bank, *offset),
            "{name}'s first grenade finds the stock block at 0x{offset:X}, named from +0x{at:X}"
        );
    }
    let graph = read_back_spawn(manager, copied, value, name);
    serde_json::json!({
        "stock_entity": format!("0x{entity:08X}"),
        "copy_entity": format!("0x{copied:08X}"),
        "stock_bank": format!("0x{bank:08X}"),
        "private_bank": format!("0x{private:08X}"),
        "rows": rows(&copy).len(),
        "named_blocks": named.len(),
        "graph_value": graph,
    })
}

/// The spawned graph's value read back: the ability's copy names a private copy of the graph,
/// holding the edited value, and the stock graph keeps its own. Returns what `readback.json`
/// records of it.
pub(in super::super) fn read_back_spawn(
    manager: &PackageManager,
    copied: u32,
    spawn: &AbilityValue,
    name: &str,
) -> serde_json::Value {
    let copied_payload = manager.read_tag(tiger_pkg::TagHash(copied)).unwrap();
    let spawned = ability_spawns::spawned_graphs(manager, copied, &copied_payload).unwrap();
    assert!(
        !spawned.contains(&spawn.entity),
        "{name}'s copy names a private copy of graph 0x{:08X}, not the stock one",
        spawn.entity
    );
    let mut spawn_locator = spawn.edit.locator.clone();
    spawn_locator.graph_tag = None;
    let read_spawn = |graph: u32| {
        let payload = manager.read_tag(tiger_pkg::TagHash(graph)).ok()?;
        resolve_weapon_runtime_field(manager, &payload, &spawn_locator)
            .ok()
            .map(|resolved| resolved.field.value)
    };
    let private = spawned
        .iter()
        .copied()
        .find(|graph| read_spawn(*graph).as_ref() == Some(&spawn.edit.value))
        .unwrap_or_else(|| {
            panic!(
                "{name}'s copy names no graph with the edited {} of 0x{:08X}",
                spawn.name, spawn.entity
            )
        });
    assert_eq!(
        read_spawn(spawn.entity),
        Some(spawn.stock.clone()),
        "the spawned stock graph keeps its {}",
        spawn.name
    );
    serde_json::json!({
        "field": spawn.name,
        "stock_graph": format!("0x{:08X}", spawn.entity),
        "copy_graph": format!("0x{private:08X}"),
        "authored": format!("{:?}", spawn.edit.value),
    })
}

/// The staged subclass is class neutral and keeps every ability it did not take elsewhere, has
/// the chosen grenade and attunement under their own names, a list of its own and no Collections
/// entry.
#[allow(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
pub(in super::super) fn read_back_subclass(
    (stock, staged): (&InvestmentCatalog, &InvestmentCatalog),
    build: &BuildReport,
    subclass: &AuthoredSubclass,
    packages: &Path,
) -> serde_json::Value {
    let name = &subclass.recipe.name;
    let report = build
        .weapons
        .iter()
        .find(|report| report.name == *name)
        .unwrap();
    assert_eq!(report.kind, ItemKind::Subclass);
    assert!(
        report.collection.is_none(),
        "{name} has no collectible or unlock"
    );
    let authored = staged
        .subclasses(|_| true)
        .into_iter()
        .find(|staged| staged.hash == report.item_hash)
        .unwrap_or_else(|| panic!("{name} reads back as a subclass"));
    assert_eq!(&authored.name, name);
    assert_eq!(authored.class_type, 3, "Every Class is class neutral");
    assert_eq!(
        staged.item_type_name(report.item_hash).as_deref(),
        Some("Guardian Subclass")
    );
    // The manifest retains the donor class for default equipping and Every Class for grants.
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(
            build
                .run_directory
                .join(crate::manifest::MANIFEST_FILE_NAME),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        manifest["project"]["weapons"]
            .as_array()
            .unwrap()
            .iter()
            .find(|weapon| weapon["name"] == name.as_str())
            .map(|weapon| (weapon["class_type"].clone(), weapon["every_class"].clone())),
        Some((
            serde_json::json!(subclass.base.class_type),
            serde_json::json!(true)
        )),
        "{name} records its class and Every Class for the install"
    );
    assert!(
        staged.item_collection_parents(report.item_hash).is_empty(),
        "{name} stays out of Collections"
    );
    // Every subclass the build authors edits its abilities, the retarget one too, so each adds
    // one list.
    let authored_lists = build
        .weapons
        .iter()
        .filter(|report| report.kind == ItemKind::Subclass)
        .count();
    assert_eq!(
        staged.socket_entry_list_count(),
        stock.socket_entry_list_count() + authored_lists,
        "each authored subclass adds one socket-entry list"
    );
    let base = &subclass.base;
    let grenade = layout::GRENADES[0];
    let ability = layout::GRENADES[1];
    let top = AttunementPath::Top.entries();
    let from = AttunementPath::Bottom.entries();
    let authored_node = AttunementPath::Middle.entries()[usize::from(NODE_POSITION)];
    for (&entry, expected) in &base.entry_names {
        let expected = if entry == grenade {
            subclass.grenade.entry_names[&entry].as_str()
        } else if entry == ability {
            ABILITY_NAME
        } else if let Some(position) = top.iter().position(|top| *top == entry) {
            subclass.attunement.entry_names[&from[position]].as_str()
        } else if entry == authored_node {
            NODE_NAME
        } else {
            expected.as_str()
        };
        assert_eq!(
            authored.entry_names.get(&entry).map(String::as_str),
            Some(expected),
            "{name} entry {entry}"
        );
    }
    assert_eq!(
        authored.attunement_names,
        [
            subclass.attunement.attunement_names[AttunementPath::Bottom.index()].clone(),
            base.attunement_names[AttunementPath::Bottom.index()].clone(),
            PATH_NAME.to_owned(),
        ],
        "{name} names each attunement for the subclass it comes from, or by its own name"
    );
    // Its strings give it the damage type its Damage Type Icon chose, read as any subclass's.
    {
        use crate::recipe::RecipeDamageType;
        use sundial::investment::WeaponDamageType;
        let chosen = subclass
            .recipe
            .overrides
            .subclass_damage_type
            .map(|damage| match damage {
                RecipeDamageType::Kinetic => WeaponDamageType::Kinetic,
                RecipeDamageType::Arc => WeaponDamageType::Arc,
                RecipeDamageType::Solar => WeaponDamageType::Solar,
                RecipeDamageType::Void => WeaponDamageType::Void,
            });
        assert!(chosen.is_some(), "{name} chooses a damage type icon");
        assert_ne!(chosen, base.damage_type, "{name} shows another damage type");
        assert_eq!(
            authored.damage_type, chosen,
            "{name} shows the damage type its strings were given"
        );
    }
    serde_json::json!({
        "kind": ItemKind::Subclass,
        "name": name,
        "item_hash": format!("0x{:08X}", report.item_hash),
        "base": format!("{} 0x{:08X}", base.name, base.hash),
        "class": authored.class_type,
        "every_class": subclass.recipe.overrides.subclass_every_class,
        "damage_type_icon": authored.damage_type.map(sundial::investment::WeaponDamageType::label),
        "grenade_from": format!("{} 0x{:08X}", subclass.grenade.name, subclass.grenade.hash),
        "top_attunement_from": format!(
            "{} 0x{:08X}",
            subclass.attunement.name, subclass.attunement.hash
        ),
        "attunements": authored.attunement_names,
        "authored_grenade": read_back_ability((stock, staged), &authored, subclass, packages),
        "screen_art": read_back_screen_art(staged, (report.item_hash, subclass), packages),
        "abilities": authored
            .entry_names
            .iter()
            .map(|(entry, name)| format!("{entry}: {name}"))
            .collect::<Vec<_>>(),
        "collections": staged.item_collection_paths(report.item_hash),
    })
}

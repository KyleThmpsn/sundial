//! Source grid placement. Every source step is accounted for exactly once: placed with its source
//! name and icon, kept native with the reasons it did not translate, kept as a native foundation,
//! or left out as an attribute choice. The item takes the source subclass's name, flavor text and
//! icon cut to a diamond, and no lore tab. A dropped step, a placed step showing another step's
//! icon, a native step without a reason, a donor lore tab or an uncut item icon is a failure.
use super::*;
use crate::icon_edit::ImportedIcon;
use crate::subclass::{EntryEdits, EntryIcon};
use parhelion_import::d1_roi::ability::SubclassCatalog;

const PLACED: [&str; 9] = [
    "super",
    "super_partial",
    "grenade",
    "movement",
    "melee",
    "passive",
    "foundation",
    "super_upgrade",
    "super_upgrade_with_super",
];

fn edits(recipe: &crate::WeaponRecipe, entry: u8) -> Option<&EntryEdits> {
    let abilities = recipe.overrides.subclass_abilities.as_ref()?;
    if let Some(choice) = abilities
        .choices
        .iter()
        .find(|choice| choice.entry == entry)
    {
        return Some(&choice.edits);
    }
    AttunementPath::ALL.into_iter().find_map(|path| {
        let position = path.entries().iter().position(|&own| own == entry)?;
        abilities
            .attunement(path)?
            .node(u8::try_from(position).unwrap())
            .map(|node| &node.edits)
    })
}

/// Returns the entries the preparation kept native, which must stay stock.
pub(super) fn readback(
    source: &Source,
    subclass: &SubclassCatalog,
    provenance: &serde_json::Value,
    recipe: &crate::WeaponRecipe,
) -> Vec<u8> {
    check_item_identity(source, subclass, recipe);
    let records = provenance["grid"].as_array().expect("grid placements");
    let mut seen = BTreeMap::new();
    for record in records {
        let key = (
            record["node"].as_u64().unwrap(),
            record["step"].as_u64().unwrap(),
        );
        assert!(
            seen.insert(key, record).is_none(),
            "step {key:?} is placed twice"
        );
    }
    let mut native = BTreeSet::new();
    for node in &subclass.nodes {
        for step in &node.steps {
            let record = seen
                .remove(&(u64::from(node.node), u64::from(step.step)))
                .unwrap_or_else(|| panic!("step {} was dropped", step.name.trim()));
            let placement = record["placement"].as_str().unwrap();
            let entries = record["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| u8::try_from(entry.as_u64().unwrap()).unwrap())
                .collect::<Vec<_>>();
            let container = step.source["icon"].as_u64().unwrap_or(u64::from(u32::MAX));
            let icon = source
                .icon_png(u32::try_from(container).unwrap(), None)
                .unwrap()
                .map(|png| ImportedIcon::from_bytes(&png).unwrap());
            check_placement(
                recipe,
                record,
                placement,
                step.name.trim(),
                &entries,
                icon.as_ref(),
                &mut native,
            );
        }
    }
    assert!(seen.is_empty(), "placements name steps the source lacks");
    native.into_iter().collect()
}

/// Source HUD art. A placed grenade's tile draws the white glyph the source HUD drew, apart from
/// its node icon. The equipped row's controller definition names that glyph at +170, with byte
/// +174 hiding it, and the source glyph table names its art. The staged copy names a glyph row of
/// its own whose one layer draws that art edge to edge. A tile drawing the stock grenade's layer
/// or the node icon is a failure. Returns each checked tile for the artifact.
pub(super) fn hud_readback(
    source: &Source,
    subclass: &SubclassCatalog,
    provenance: &serde_json::Value,
    recipe: &crate::WeaponRecipe,
    (stock, staged): (
        &sundial::package_authoring::PackageManager,
        &sundial::package_authoring::PackageManager,
    ),
    (donor, copy): (
        &std::collections::BTreeMap<u8, u32>,
        &std::collections::BTreeMap<u8, u32>,
    ),
) -> serde_json::Value {
    use sundial::package_authoring::{PackageManager, ability_hud};
    use tiger_pkg::TagHash;
    let glyph = |manager: &PackageManager, entity: u32| {
        let payload = manager.read_tag(TagHash(entity)).unwrap();
        ability_hud::glyph_site(manager, &payload)
            .unwrap()
            .expect("a grenade has a HUD glyph")
            .key
    };
    let rows = |manager: &PackageManager| {
        let bytes = manager.read_tag(TagHash(ability_hud::GLYPH_TABLE)).unwrap();
        bytes[48..]
            .chunks_exact(112)
            .map(|row| {
                (
                    u32::from_le_bytes(row[..4].try_into().unwrap()),
                    row.to_vec(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let (stock_rows, staged_rows) = (rows(stock), rows(staged));
    let mut checked = Vec::new();
    for record in provenance["grid"].as_array().unwrap() {
        if record["placement"] != "grenade" {
            continue;
        }
        let entry = u8::try_from(record["entries"][0].as_u64().unwrap()).unwrap();
        let step = subclass
            .nodes
            .iter()
            .filter(|node| record["node"] == u64::from(node.node))
            .flat_map(|node| &node.steps)
            .find(|step| record["step"] == u64::from(step.step))
            .unwrap();
        let row = step.source["variants"][0]["ability_grants"]
            .as_array()
            .unwrap()
            .iter()
            .map(|grant| hex::decode(grant.as_str().unwrap()).unwrap())
            .find(|grant| grant[11] != u8::MAX)
            .expect("a grenade step equips its grenade")[11];
        let ability = subclass
            .abilities
            .iter()
            .find(|ability| ability.row == row)
            .unwrap();
        let controller = source.read(ability.controller).unwrap();
        let definition = controller.pointer(24).unwrap();
        assert_eq!(
            controller.u8(definition + 0x174).unwrap(),
            0,
            "the source HUD shows {}",
            step.name.trim()
        );
        let key = controller.u32(definition + 0x170).unwrap();
        let png = source
            .hud_glyph_png(key)
            .unwrap()
            .expect("the source glyph table draws the grenade");
        let expected = ImportedIcon::from_bytes(&png).unwrap();
        let edits = edits(recipe, entry).unwrap();
        let Some(crate::perk::Icon::Image { image, .. }) = &edits.hud_artwork else {
            panic!("{} has no source HUD art", step.name.trim());
        };
        assert_eq!(image, &expected, "{} draws another glyph", step.name.trim());
        let Some(EntryIcon::Artwork {
            artwork: crate::perk::Icon::Image { image: node, .. },
        }) = &edits.icon
        else {
            panic!("{} has no node icon", step.name.trim());
        };
        let stock_key = glyph(stock, donor[&entry]);
        let staged_key = glyph(staged, copy[&entry]);
        assert!(
            !stock_rows.contains_key(&staged_key),
            "the copy names a glyph row of its own"
        );
        let written = &staged_rows[&staged_key];
        assert!(
            written[8..0x1C].chunks(4).all(|slot| slot == [0xFF; 4]),
            "one layer draws the tile"
        );
        assert_ne!(
            written[4..8],
            stock_rows[&stock_key][4..8],
            "the tile draws art of its own"
        );
        let layer = u32::from_le_bytes(written[4..8].try_into().unwrap());
        let (size, alpha) = layer_alpha(staged, layer);
        let difference = |image: &ImportedIcon| {
            let fitted = image.fit_to(size[0], size[1]);
            let total = fitted
                .pixels()
                .zip(&alpha)
                .map(|(pixel, &drawn)| f64::from(pixel[3].abs_diff(drawn)))
                .sum::<f64>();
            total / (f64::from(u32::try_from(alpha.len()).unwrap()) * 255.0)
        };
        let (from_glyph, from_node) = (difference(&expected), difference(node));
        assert!(
            from_glyph < 0.02 && from_glyph < from_node,
            "{} draws the source glyph edge to edge: {from_glyph} against the node icon's {from_node}",
            step.name.trim()
        );
        checked.push(serde_json::json!({"step":step.name.trim(),"entry":entry,
            "source_glyph":format!("{key:08X}"),"stock_glyph":format!("{stock_key:08X}"),
            "staged_glyph":format!("{staged_key:08X}"),"layer":format!("{layer:08X}"),
            "size":size,"alpha_difference":from_glyph,"node_icon_difference":from_node}));
    }
    assert!(!checked.is_empty(), "the source grid places a grenade");
    serde_json::Value::Array(checked)
}

/// The size and alpha of the first texture of an icon layer's first lane.
fn layer_alpha(
    manager: &sundial::package_authoring::PackageManager,
    layer: u32,
) -> ([u32; 2], Vec<u8>) {
    use tiger_pkg::TagHash;
    let layer = manager.read_tag(TagHash(layer)).unwrap();
    let target = |at: usize| {
        let relative = i64::from_le_bytes(layer[at..at + 8].try_into().unwrap());
        at.checked_add_signed(isize::try_from(relative).unwrap())
            .unwrap()
    };
    let lanes = target(0x28);
    let textures = target(lanes + 0x18);
    let header_tag = TagHash(u32::from_le_bytes(
        layer[textures + 0x10..textures + 0x14].try_into().unwrap(),
    ));
    let header = manager.read_tag(header_tag).unwrap();
    let format = u32::from_le_bytes(header[4..8].try_into().unwrap());
    assert!(matches!(format, 28 | 29), "the HUD art stays RGBA8");
    let size = [0x0E, 0x10].map(|at| u32::from(u16::from_le_bytes([header[at], header[at + 1]])));
    let data = manager
        .read_tag(TagHash(manager.get_entry(header_tag).unwrap().reference))
        .unwrap();
    let length = usize::try_from(size[0] * size[1] * 4).unwrap();
    (
        size,
        data[..length]
            .chunks_exact(4)
            .map(|pixel| pixel[3])
            .collect(),
    )
}

fn check_item_identity(source: &Source, subclass: &SubclassCatalog, recipe: &crate::WeaponRecipe) {
    assert_eq!(recipe.name, subclass.name.trim());
    assert_eq!(recipe.flavor, subclass.description.trim());
    assert!(
        recipe.overrides.remove_lore && recipe.overrides.lore.is_none(),
        "a source item has no lore tab"
    );
    let png = source
        .subclass_icon(subclass.item)
        .unwrap()
        .expect("required source subclass icon");
    let original = ImportedIcon::from_bytes(&png).unwrap().fit_to(96, 96);
    let cut = recipe
        .overrides
        .icon_edit
        .imported_image
        .as_ref()
        .expect("the item takes the source icon")
        .fit_to(96, 96);
    for (x, y) in [(0, 0), (95, 0), (0, 95), (95, 95), (10, 10), (85, 85)] {
        assert_eq!(cut.get_pixel(x, y).0[3], 0, "corner {x},{y} stays visible");
    }
    for (x, y) in [(48, 48), (48, 20), (20, 48)] {
        assert_eq!(
            cut.get_pixel(x, y),
            original.get_pixel(x, y),
            "diamond interior {x},{y} differs"
        );
    }
}

fn check_placement(
    recipe: &crate::WeaponRecipe,
    record: &serde_json::Value,
    placement: &str,
    name: &str,
    entries: &[u8],
    icon: Option<&ImportedIcon>,
    native: &mut BTreeSet<u8>,
) {
    if PLACED.contains(&placement) {
        for &entry in entries {
            check_placed_entry(recipe, placement, name, entry, icon);
        }
    } else if placement == "native_equivalent" {
        // The native entry already grants the matched actions, so it stays stock.
        assert!(
            !record["lowering"]["native_equivalents"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        native.extend(entries.iter().copied());
    } else if placement == "native" {
        // A step with gated rows stays native where its tree does not apply the gate.
        assert!(
            !record["unsupported"].as_array().unwrap().is_empty()
                || record["resources"].is_array()
                || record["unapplied_gates"]
                    .as_array()
                    .is_some_and(|gates| !gates.is_empty()),
            "{} stays native without a reason",
            name
        );
        native.extend(entries.iter().copied());
    } else {
        assert!(
            matches!(
                placement,
                "native_foundation" | "left_out_attribute" | "movement_base" | "melee_base"
            ),
            "unknown placement {placement}"
        );
    }
}

fn check_placed_entry(
    recipe: &crate::WeaponRecipe,
    placement: &str,
    name: &str,
    entry: u8,
    icon: Option<&ImportedIcon>,
) {
    let edits =
        edits(recipe, entry).unwrap_or_else(|| panic!("{} has no edits at entry {entry}", name));
    if entry == layout::STAT_PASSIVES {
        assert!(!edits.custom_perks.is_empty());
        return;
    }
    if !placement.starts_with("super_upgrade") {
        assert_eq!(edits.name.as_deref(), Some(name));
    }
    if placement != "super_upgrade_with_super" {
        let Some(EntryIcon::Artwork {
            artwork: crate::perk::Icon::Image { image, .. },
        }) = &edits.icon
        else {
            panic!("{} has no source icon at entry {entry}", name);
        };
        assert_eq!(Some(image), icon, "{} shows another icon", name);
    }
}

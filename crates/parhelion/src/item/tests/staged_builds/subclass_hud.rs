//! Saved subclass colors survive a real package build across every stock donor, and
//! another copy of the same donor can choose a different color. Readback follows the staged
//! catalog, entity component references and sorted glyph rows without the color writer.
//!
//! Failure boundaries: a color-only recipe being discarded, one Super being missed, shared
//! donor mutation, two recipes sharing a key, glyph artwork changing, incorrect color space,
//! a broken row count or ordering, reset retaining an authored glyph, and a middle path with
//! a passive lead node being mistaken for a separate Super. The UI overlay must also preserve
//! the source package's live shared-tag registrations and unused allocated slots.
//!
//! The Voidwalker copies also draw a Generated Icon. Its failure boundaries: the icon keeping the
//! stock art, the wrong color source with and without a HUD color of its own, an icon edit left
//! from the base icon recoloring it, a texture resized away from the stock 160 pixels, and a
//! missing border or transparent corners.
use super::*;
use crate::subclass::{GeneratedIcon, SubclassAbilities};
use sundial::package_authoring::{PackageManager, ability_hud};

mod attached;
mod color;

const GLYPHS: TagHash = TagHash(0x80BC6A79);
type GlyphRows = BTreeMap<u32, Vec<u8>>;

fn rows(manager: &PackageManager) -> BTreeMap<u32, Vec<u8>> {
    let bytes = manager.read_tag(GLYPHS).unwrap();
    let count = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
    assert_eq!(bytes.len(), 48 + count * 112);
    assert_eq!(
        u64::from_le_bytes(bytes[32..40].try_into().unwrap()) as usize,
        count
    );
    let mut previous = None;
    bytes[48..]
        .chunks_exact(112)
        .map(|row| {
            let key = u32::from_le_bytes(row[..4].try_into().unwrap());
            assert!(previous.is_none_or(|previous| previous < key));
            previous = Some(key);
            (key, row.to_vec())
        })
        .collect()
}

fn glyph(manager: &PackageManager, entity: u32) -> Option<u32> {
    let payload = manager.read_tag(TagHash(entity)).unwrap();
    ability_hud::glyph_site(manager, &payload)
        .unwrap()
        .map(|site| site.key)
}

/// The glyphs `entity`'s bank names while `keys` apply, in row order: the key at +0x10 of the
/// modifier of each ungated row of class `80804532` they apply. The HUD record writer reads the
/// field the last one writes, so the tile shows its glyph over the controller's.
fn bank_glyphs(manager: &PackageManager, entity: u32, keys: &[u32]) -> Vec<u32> {
    use sundial::package_authoring::{ability_bank, ability_modifier};
    let payload = manager.read_tag(TagHash(entity)).unwrap();
    let Some(bank) = ability_modifier::entity_bank(&payload).unwrap() else {
        return Vec::new();
    };
    let bank = manager.read_tag(TagHash(bank)).unwrap();
    ability_bank::property_rows(&bank)
        .unwrap()
        .iter()
        .zip(ability_bank::row_modifiers(&bank).unwrap())
        .filter(|(row, (_, gated))| {
            !gated && row.modifier_class == 0x8080_4532 && keys.contains(&row.key)
        })
        .map(|(_, (block, _))| u32::from_le_bytes(bank[block + 16..block + 20].try_into().unwrap()))
        .collect()
}

/// The keys `subclass`'s entry `entry` applies to its own ability row.
fn entry_keys(subclass: &sundial::investment::SubclassSummary, entry: u8) -> Vec<u32> {
    let Some(&row) = subclass.entry_rows.get(&entry) else {
        return Vec::new();
    };
    subclass
        .entry_modifiers
        .get(&entry)
        .into_iter()
        .flatten()
        .filter(|(_, target)| *target == row)
        .map(|(key, _)| *key)
        .collect()
}

fn check_color(row: &[u8], stock: &[u8], rgb: [u8; 3]) -> [f32; 4] {
    assert_eq!(&row[4..32], &stock[4..32], "all glyph layers are preserved");
    assert_eq!(
        &row[80..96],
        &stock[80..96],
        "unrelated glyph properties are preserved"
    );
    assert_eq!(&row[97..], &stock[97..]);
    assert_ne!(row[96] & 1, 0, "the native color-presence flag is set");
    let linear = std::array::from_fn(|channel| {
        let start = 32 + channel * 4;
        f32::from_le_bytes(row[start..start + 4].try_into().unwrap())
    });
    assert_eq!(linear[3], 1.0);
    // Native 15C0C80 publishes RGB raised to 1/2.2. Decode the emitted row through that
    // consumer contract, independently of the writer.
    for (byte, value) in rgb.into_iter().zip(linear) {
        let displayed = (f64::from(value).powf(1.0 / 2.2) * 255.0).round() as u8;
        assert_eq!(displayed, byte);
    }
    linear
}

// New feature failure boundaries: lost persisted entry colors, passive nodes requiring an
// entity, a Super override leaking into inherited tiles, missing presence bits, shared icon
// mutation, stale colors after reset, and UI components outside the replicated ability/node
// hierarchy. A decision's stored value outside the table's value pools, or one written into a
// variant object, crashed the client as the HUD loaded in the world. Stock values must read
// the same afterwards, through pools that moved to make room. Decode the emitted bindings,
// hierarchy and values independently of the native UI writer.
fn cui_array(bytes: &[u8], descriptor: usize, stride: usize, class: u32) -> Vec<usize> {
    let count = u64::from_le_bytes(bytes[descriptor..descriptor + 8].try_into().unwrap()) as usize;
    if count == 0 {
        return Vec::new();
    }
    let delta = i64::from_le_bytes(bytes[descriptor + 8..descriptor + 16].try_into().unwrap());
    let header = (descriptor + 8).checked_add_signed(delta as isize).unwrap();
    assert_eq!(
        u32::from_le_bytes(bytes[header - 4..header].try_into().unwrap()),
        0x80809FBD
    );
    assert_eq!(
        u64::from_le_bytes(bytes[header..header + 8].try_into().unwrap()) as usize,
        count
    );
    assert_eq!(
        u64::from_le_bytes(bytes[header + 8..header + 16].try_into().unwrap()),
        u64::from(class)
    );
    assert!(header + 16 + count * stride <= bytes.len());
    (0..count).map(|i| header + 16 + i * stride).collect()
}

fn cui_hierarchy(
    source: &PackageManager,
    manager: &PackageManager,
    widget: u32,
    hierarchy: u32,
    added: usize,
) -> (BTreeMap<u16, u16>, u16) {
    let tree = manager.read_tag(TagHash(hierarchy)).unwrap();
    assert_eq!(u32::from_le_bytes(tree[28..32].try_into().unwrap()), widget);
    let hierarchy_rows = cui_array(&tree, 8, 4, 0x80804616);
    let nodes: BTreeMap<u16, u16> = hierarchy_rows
        .iter()
        .map(|&at| {
            (
                u16::from_le_bytes(tree[at + 2..at + 4].try_into().unwrap()),
                u16::from_le_bytes(tree[at..at + 2].try_into().unwrap()),
            )
        })
        .collect();
    assert_eq!(
        nodes.len(),
        hierarchy_rows.len(),
        "component identities remain unique"
    );
    let first_new = u16::try_from(nodes.len() - added).unwrap();
    let old_tree = source.read_tag(TagHash(hierarchy)).unwrap();
    for at in cui_array(&old_tree, 8, 4, 0x80804616) {
        let child = u16::from_le_bytes(old_tree[at + 2..at + 4].try_into().unwrap());
        let parent = u16::from_le_bytes(old_tree[at..at + 2].try_into().unwrap());
        assert_eq!(
            nodes[&child], parent,
            "stock widgets stay in their original hierarchy"
        );
    }
    (nodes, first_new)
}

fn cui_field(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// The identity of a widget table's default property object. Its other objects are variants.
const CUI_DEFAULT_OBJECT: [u8; 24] = [
    0xAF, 0x89, 0x9B, 0x7F, 0x3E, 0xFA, 0x1C, 0x70, 0x24, 0x9D, 0xFB, 0x40, 0xE3, 0xBA, 0xBA, 0xEC,
    0xB9, 0xC0, 0x52, 0xAD, 0, 0, 0, 0,
];

/// The rows of the table's pool of four-byte enumeration values, from the header's descriptor.
fn cui_enum_pool(bytes: &[u8]) -> std::ops::Range<usize> {
    let rows = cui_array(bytes, 0x98, 4, 0x80804696);
    rows.first()
        .map_or(0..0, |&first| first..first + rows.len() * 4)
}

/// Each component property's value, by object, component and selector, as the offset its
/// value pointer names, with whether the object is the default one.
fn cui_values(bytes: &[u8]) -> BTreeMap<(usize, u32, u64), (usize, bool)> {
    let mut values = BTreeMap::new();
    for (ordinal, object) in cui_array(bytes, 0x48, 64, 0x8080462A)
        .into_iter()
        .enumerate()
    {
        let default = bytes[object..object + 24] == CUI_DEFAULT_OBJECT;
        for component in cui_array(bytes, object + 32, 24, 0x808046D4) {
            for property in cui_array(bytes, component + 8, 24, 0x80804858) {
                let delta =
                    i64::from_le_bytes(bytes[property + 16..property + 24].try_into().unwrap());
                if delta == 0 {
                    continue;
                }
                let selector =
                    u64::from_le_bytes(bytes[property + 8..property + 16].try_into().unwrap());
                let at = (property + 16).checked_add_signed(delta as isize).unwrap();
                values.insert(
                    (ordinal, cui_field(bytes, component), selector),
                    (at, default),
                );
            }
        }
    }
    values
}

/// Every stock value reads the same after the edit, an enumeration value from the same row of
/// the moved pool, and each new decision stores only its value type, RGBA (7), from the
/// enumeration pool of the default object, as every stock decision does.
fn check_cui_values(stock: &[u8], bytes: &[u8], new_components: std::ops::Range<u16>) {
    let (stock_pool, pool) = (cui_enum_pool(stock), cui_enum_pool(bytes));
    let values = cui_values(bytes);
    for (key, (at, _)) in cui_values(stock) {
        let (moved, _) = values[&key];
        assert_eq!(
            bytes[moved..moved + 4],
            stock[at..at + 4],
            "stock values read the same"
        );
        if stock_pool.contains(&at) {
            assert_eq!(
                moved - pool.start,
                at - stock_pool.start,
                "an enumeration value keeps its row in the moved pool"
            );
        }
    }
    for index in new_components {
        let stored = values
            .iter()
            .filter(|((_, component, _), _)| *component == u32::from(index))
            .collect::<Vec<_>>();
        assert_eq!(stored.len(), 1, "a decision stores its value type alone");
        let (&(_, _, selector), &(at, default)) = stored[0];
        assert!(default, "only the default object holds the decision");
        assert_eq!(selector, 0x201);
        assert!(
            pool.contains(&at),
            "the value type lies in the enumeration pool"
        );
        assert_eq!(
            cui_field(bytes, at),
            7,
            "the decision chooses an RGBA color"
        );
    }
}

fn cui_path(bytes: &[u8], at: usize) -> Vec<u32> {
    let delta = i64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
    if delta == 0 {
        return Vec::new();
    }
    cui_array(
        bytes,
        at.checked_add_signed(delta as isize).unwrap(),
        4,
        0x80804949,
    )
    .into_iter()
    .map(|at| cui_field(bytes, at))
    .collect()
}

fn check_cui_inputs(bytes: &[u8], incoming: &[usize], index: u16, first_new: u16, menu: bool) {
    let field = |at| cui_field(bytes, at);
    for property in [0x202, 0x203, 0x204] {
        assert_eq!(
            incoming
                .iter()
                .filter(|&&at| field(at + 40) == property)
                .count(),
            1
        );
    }
    let input = |property| {
        *incoming
            .iter()
            .find(|&&at| field(at + 40) == property)
            .unwrap()
    };
    let condition = input(0x202);
    assert_eq!(
        field(condition + 16) & 0xFFFF,
        7,
        "presence is a native boolean"
    );
    assert_eq!(cui_path(bytes, condition + 8), [0x600, 0]);
    let ordinal = usize::from(index - first_new);
    let (icon, selector, fallback, property, fallback_path) = match (menu, ordinal) {
        (true, ordinal) => ([0xA5, 0x10A, 0x16F][ordinal], 7, 0x1C3, 0x20C, vec![0x80C]),
        (false, 0) => (0x2B5, 0x10007, 0x2B5, 6, vec![0x600, 0]),
        (false, _) => (0x2A1, 0x20007, first_new, 0x205, Vec::new()),
    };
    assert_eq!(field(condition) as u16, icon);
    assert_eq!(field(condition + 16), selector);
    let yes = input(0x203);
    assert_eq!(field(yes) as u16, icon);
    assert_eq!(field(yes + 16), selector - 1);
    assert_eq!(cui_path(bytes, yes + 8), [0x600, 0]);
    let no = input(0x204);
    assert_eq!(field(no) as u16, fallback);
    assert_eq!(field(no + 16), property);
    assert_eq!(cui_path(bytes, no + 8), fallback_path);
}

fn check_cui_outputs(
    bytes: &[u8],
    outgoing: &[usize],
    nodes: &BTreeMap<u16, u16>,
    index: u16,
    first_new: u16,
    menu: bool,
) {
    let field = |at| cui_field(bytes, at);
    assert!(!outgoing.is_empty());
    assert!(outgoing.iter().all(|&at| field(at + 16) == 0x205));
    let ordinal = usize::from(index - first_new);
    let expected_outputs = if menu {
        vec![([0xA1, 0x106, 0x16B][ordinal], 0x20C)]
    } else if ordinal == 0 {
        vec![(first_new + 1, 0x204)]
    } else {
        vec![(0x29A, 0x203), (0x29B, 0x203)]
    };
    assert_eq!(
        outgoing
            .iter()
            .map(|&at| (field(at + 24) as u16, field(at + 40)))
            .collect::<Vec<_>>(),
        expected_outputs
    );
    for &at in outgoing {
        let target = field(at + 24) as u16;
        assert_eq!(
            nodes[&index], nodes[&target],
            "selection stays in the replicated node or tile group"
        );
    }
}

fn cui_routes(
    source: &PackageManager,
    manager: &PackageManager,
    widget: u32,
    hierarchy: u32,
    added: usize,
) -> serde_json::Value {
    let (nodes, first_new) = cui_hierarchy(source, manager, widget, hierarchy, added);
    let bytes = manager.read_tag(TagHash(widget)).unwrap();
    check_cui_values(
        &source.read_tag(TagHash(widget)).unwrap(),
        &bytes,
        first_new..u16::try_from(nodes.len()).unwrap(),
    );
    let classes = cui_array(&bytes, 0x38, 16, 0x80804622);
    let bindings = cui_array(&bytes, 0x58, 56, 0x808046D8);
    let names = cui_array(&bytes, 0x18, 4, 0x80804618);
    assert_eq!(classes.len(), names.len());
    let field = |at| cui_field(&bytes, at);
    let path = |at| cui_path(&bytes, at);
    let mut receipts = Vec::new();
    for index in first_new..u16::try_from(nodes.len()).unwrap() {
        let class = classes
            .iter()
            .find(|&&at| (field(at + 8) >> 16) as u16 == index)
            .unwrap();
        assert_eq!(
            u64::from_le_bytes(bytes[*class..*class + 8].try_into().unwrap()),
            0xFBB44B29
        );
        let incoming = bindings
            .iter()
            .filter(|&&at| field(at + 24) as u16 == index)
            .copied()
            .collect::<Vec<_>>();
        check_cui_inputs(&bytes, &incoming, index, first_new, added == 3);
        let outgoing = bindings
            .iter()
            .filter(|&&at| field(at) as u16 == index)
            .copied()
            .collect::<Vec<_>>();
        check_cui_outputs(&bytes, &outgoing, &nodes, index, first_new, added == 3);
        receipts.push(serde_json::json!({"component": index, "parent": nodes[&index],
            "inputs": incoming.iter().map(|&at| serde_json::json!({"source":field(at), "path":path(at+8),
                "property":field(at+16), "input":field(at+40)})).collect::<Vec<_>>(),
            "outputs": outgoing.iter().map(|&at| serde_json::json!({"target":field(at+24),"property":field(at+40)})).collect::<Vec<_>>() }));
    }
    serde_json::json!({"widget":format!("{widget:08X}"),"hierarchy":format!("{hierarchy:08X}"),"switches":receipts})
}

fn check_node_color(
    manager: &PackageManager,
    stock: &(u32, Vec<u8>),
    tag: u32,
    color: Option<[u8; 3]>,
) {
    let (stock_tag, original) = stock;
    assert_eq!(manager.read_tag(TagHash(*stock_tag)).unwrap(), *original);
    let Some(rgb) = color else {
        assert_eq!(tag, *stock_tag);
        return;
    };
    let bytes = manager.read_tag(TagHash(tag)).unwrap();
    assert_ne!(tag, *stock_tag);
    assert_eq!(
        &bytes[0x14..0x30],
        &original[0x14..0x30],
        "all menu artwork layers stay intact"
    );
    assert_ne!(bytes[0x70] & 1, 0);
    let displayed: [u8; 3] = std::array::from_fn(|c| {
        let at = 0x30 + c * 4;
        let v = f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        (f64::from(v).powf(1.0 / 2.2) * 255.0).round() as u8
    });
    assert_eq!(displayed, rgb);
}

fn check_ability_glyph(
    (source, staged): (&PackageManager, &PackageManager),
    (stock_entity, entity): (Option<u32>, Option<u32>),
    (stock_keys, keys): (&[u32], &[u32]),
    (stock_rows, written_rows): (&GlyphRows, &GlyphRows),
    color: Option<[u8; 3]>,
    theme: Option<[u8; 3]>,
    is_super: bool,
) -> Option<u32> {
    let Some(stock_entity) = stock_entity else {
        assert!(entity.is_none());
        return None;
    };
    let entity = entity.expect("the authored entry still equips its ability");
    let Some(stock_key) = glyph(source, stock_entity) else {
        assert_eq!(glyph(staged, entity), None);
        return None;
    };
    let key = glyph(staged, entity).expect("the ability still has a glyph");
    let Some(rgb) = color else {
        assert_eq!(key, stock_key);
        assert_eq!(entity, stock_entity);
        return Some(key);
    };
    // A variant row of the bank decides the glyph the stock tile shows, and so the art the
    // private row copies. Every such row must name the private row, or the tile keeps its stock
    // glyph and color.
    let stock_variants = bank_glyphs(source, stock_entity, stock_keys);
    let shown = stock_variants.last().copied().unwrap_or(stock_key);
    let variants = bank_glyphs(staged, entity, keys);
    assert_eq!(variants.len(), stock_variants.len());
    assert!(
        variants.iter().all(|named| *named == key),
        "the bank's variant rows name the private glyph"
    );
    check_color(&written_rows[&key], &stock_rows[&shown], rgb);
    if is_super {
        let expected = theme.unwrap_or_else(|| published(&stock_rows[&shown]));
        let row = &written_rows[&key];
        assert_ne!(row[0x60] & 2, 0, "a Super has a separate inherited theme");
        let actual: [u8; 3] = std::array::from_fn(|c| {
            let at = 0x30 + c * 4;
            (f64::from(f32::from_le_bytes(row[at..at + 4].try_into().unwrap())).powf(1.0 / 2.2)
                * 255.0)
                .round() as u8
        });
        assert_eq!(actual, expected);
    }
    Some(key)
}

/// The entry that takes an icon with no glyph of its own: the second grenade.
const ICON_ENTRY: u8 = 8;

/// An icon no glyph row holds reaches the HUD through a private glyph row of the ability's own,
/// named by its controller, that draws the icon container's primary layer alone and keeps the
/// stock row's colors.
fn check_icon_art(
    (source, staged): (&PackageManager, &PackageManager),
    (stock_entity, entity): (u32, u32),
    (stock_rows, written_rows): (&GlyphRows, &GlyphRows),
    container: u32,
) -> u32 {
    let stock_key = glyph(source, stock_entity).expect("the grenade has a glyph");
    let key = glyph(staged, entity).expect("the copy still has a glyph");
    assert_ne!(entity, stock_entity, "the icon gives the ability a copy");
    assert!(
        !stock_rows.contains_key(&key),
        "the copy names a glyph of its own"
    );
    let icon = source.read_tag(TagHash(container)).unwrap();
    let row = &written_rows[&key];
    assert_eq!(
        row[4..8],
        icon[0x14..0x18],
        "the tile draws the icon's primary layer"
    );
    assert!(
        row[8..0x1C].chunks(4).all(|slot| slot == [0xFF; 4]),
        "no other layer draws over it"
    );
    assert_eq!(
        row[0x20..],
        stock_rows[&stock_key][0x20..],
        "the stock colors stay"
    );
    key
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES, SUNDIAL_TEST_ARTIFACTS and SUNDIAL_TEST_REVISION"]
#[allow(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn per_ability_colors_reach_private_tree_icons_and_hud_with_independent_inheritance() {
    use crate::subclass::{AttunementPath, EntryEdits, Place};
    let packages = crate::test_support::stock_packages();
    let artifacts = PathBuf::from(std::env::var_os("SUNDIAL_TEST_ARTIFACTS").unwrap());
    let revision = std::env::var("SUNDIAL_TEST_REVISION").unwrap();
    let ignored = crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|s| (*s).to_owned())
        .collect::<Vec<_>>();
    let view = crate::workflow::FilteredPackageView::create(&packages, &ignored).unwrap();
    let install = view.path().parent().unwrap();
    fs::write(install.join("destiny2.exe"), []).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let cache = temporary.path().join("catalog.json");
    let baseline = InvestmentCatalog::load_with_cache_path(install, &cache, true, |_| {}).unwrap();
    let stock = baseline.subclasses(crate::package_profile::is_stock_item_definition);
    assert!(!stock.is_empty());
    let base = stock.iter().find(|s| s.name == "Voidwalker").unwrap();
    let source = open_manager(view.path()).unwrap();
    let source_rows = rows(&source);
    let colors = [
        (Place::Ability(7), [255, 32, 48]),
        (Place::Ability(2), [30, 220, 80]),
        (Place::Ability(4), [40, 100, 255]),
        (Place::Ability(10), [255, 180, 20]),
        (Place::Node(AttunementPath::Top, 0), [170, 50, 230]),
        (Place::Node(AttunementPath::Top, 1), [20, 230, 220]),
        (Place::Node(AttunementPath::Middle, 0), [250, 80, 190]),
    ];
    let theme = [80, 140, 220];
    let foundations = [
        (
            crate::subclass::layout::BASE_MOVEMENT,
            "Movement edits remain available with a subclass color.",
        ),
        (
            crate::subclass::layout::STAT_PASSIVES,
            "Stat passive edits remain available with a subclass color.",
        ),
    ];
    for (entry, _) in foundations {
        assert!(
            !base.entry_icons.contains_key(&entry),
            "the native foundation has no menu icon"
        );
    }
    // A node of the base whose icon no glyph row holds, such as a passive attunement node.
    let passive = base
        .entry_icons
        .keys()
        .copied()
        .find(|entry| {
            base.entry_entities
                .get(entry)
                .and_then(|&entity| glyph(&source, entity))
                .is_none()
        })
        .expect("the base has a node with no HUD glyph");
    let mut recipes = Vec::new();
    for (index, global) in [Some(theme), None, Some(theme)].into_iter().enumerate() {
        let mut recipe = saved_recipe(base, 100 + index, (global, false));
        let abilities = recipe
            .overrides
            .subclass_abilities
            .get_or_insert_with(Default::default);
        for (place, color) in colors {
            abilities.set_edits(
                base.hash,
                place,
                EntryEdits {
                    color: Some(color),
                    ..Default::default()
                },
            );
        }
        // Coloring the visible nodes must also allow independent edits to an iconless entry.
        if index == 0 {
            for (entry, description) in foundations {
                abilities.set_edits(
                    base.hash,
                    Place::Ability(entry),
                    EntryEdits {
                        description: Some(description.into()),
                        ..Default::default()
                    },
                );
            }
        }
        // The second grenade takes that node's icon, with no color of its own.
        if index == 1 {
            abilities.set_edits(
                base.hash,
                Place::Ability(ICON_ENTRY),
                EntryEdits {
                    icon: Some(crate::subclass::EntryIcon::Ability {
                        subclass: base.hash,
                        entry: passive,
                    }),
                    ..Default::default()
                },
            );
        }
        let mut recipe: crate::WeaponRecipe =
            serde_json::from_slice(&serde_json::to_vec(&recipe).unwrap()).unwrap();
        if index == 2 {
            let abilities = recipe.overrides.subclass_abilities.as_mut().unwrap();
            for (place, _) in colors {
                abilities.set_edits(base.hash, place, EntryEdits::default());
            }
            abilities.hud_color = None;
            if abilities.is_empty() {
                recipe.overrides.subclass_abilities = None;
            }
        }
        recipes.push(serde_json::from_slice(&serde_json::to_vec(&recipe).unwrap()).unwrap());
    }
    let source_icons = base
        .entry_icons
        .iter()
        .map(|(&e, &tag)| (e, (tag, source.read_tag(TagHash(tag)).unwrap())))
        .collect::<BTreeMap<_, _>>();
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: view.path().to_path_buf(),
        staging_root: artifacts.join("ability-colors"),
        ignore_installed_authored_overlays: false,
        recipes,
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    for artifact in &build.artifacts {
        view.add_overlay(&build.run_directory.join(&artifact.file_name))
            .unwrap();
    }
    let staged = open_manager(view.path()).unwrap();
    let catalog = InvestmentCatalog::load_with_cache_path(install, &cache, true, |_| {}).unwrap();
    let written_rows = rows(&staged);
    check_stock_rows(&source_rows, &written_rows);
    let mut receipt = Vec::new();
    for (index, global) in [Some(theme), None, None].into_iter().enumerate() {
        let name = format!("HUD Color {}", 100 + index);
        let subclasses = catalog.subclasses(|_| true);
        let item = subclasses.iter().find(|s| s.name == name).unwrap();
        for (entry, description) in foundations {
            assert!(!item.entry_icons.contains_key(&entry));
            assert_eq!(
                item.entry_entities.get(&entry),
                base.entry_entities.get(&entry),
                "an inherited color does not replace the foundation's ability"
            );
            assert_eq!(
                item.entry_modifiers.get(&entry),
                base.entry_modifiers.get(&entry)
            );
            if index == 0 {
                assert_eq!(
                    item.entry_descriptions.get(&entry).map(String::as_str),
                    Some(description),
                    "the iconless entry's authored description survives the package reload"
                );
            } else {
                assert_eq!(
                    item.entry_descriptions.get(&entry),
                    base.entry_descriptions.get(&entry)
                );
            }
            receipt.push(serde_json::json!({
                "item":name, "entry":entry, "icon":null,
                "description":item.entry_descriptions.get(&entry),
                "entity":item.entry_entities.get(&entry),
                "modifiers":item.entry_modifiers.get(&entry)
            }));
        }
        for place in Place::all() {
            let entry = crate::subclass::place_entry(place);
            let own = if index == 2 {
                None
            } else {
                colors.iter().find(|(p, _)| *p == place).map(|(_, c)| *c)
            };
            let expected = own.or(global);
            let tag = item.entry_icons[&entry];
            if index == 1 && entry == ICON_ENTRY {
                // The node's icon, which no glyph row holds, is what its HUD tile draws.
                check_node_color(&staged, &source_icons[&passive], tag, None);
                let key = check_icon_art(
                    (&source, &staged),
                    (base.entry_entities[&entry], item.entry_entities[&entry]),
                    (&source_rows, &written_rows),
                    base.entry_icons[&passive],
                );
                receipt.push(serde_json::json!({"item":name,"entry":entry,"icon_from":passive,"glyph":format!("{key:08X}")}));
                continue;
            }
            check_node_color(&staged, &source_icons[&entry], tag, expected);
            let is_super = matches!(entry, 10 | 20);
            let key = check_ability_glyph(
                (&source, &staged),
                (
                    base.entry_entities.get(&entry).copied(),
                    item.entry_entities.get(&entry).copied(),
                ),
                (&entry_keys(base, entry), &entry_keys(item, entry)),
                (&source_rows, &written_rows),
                own.or(global.filter(|_| is_super)),
                global,
                is_super,
            );
            receipt.push(serde_json::json!({"item":name,"entry":entry,"icon":format!("{tag:08X}"),"color":expected,"glyph":key.map(|k|format!("{k:08X}"))}));
        }
    }
    let ui = [
        (0x80BC7482, 0x80BC7483, 3),
        (0x80B47381, 0x80EFC14F, 3),
        (0x80BC6F57, 0x80BC6F5A, 2),
        (0x80BC6FB5, 0x80BC6FB6, 2),
        (0x80BC7261, 0x80BC7262, 2),
    ]
    .map(|(w, h, n)| cui_routes(&source, &staged, w, h, n));
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&build.manifest_path).unwrap()).unwrap();
    fs::write(build.run_directory.join("ability-colors-readback.json"),serde_json::to_vec_pretty(&serde_json::json!({
        "source_build":"86657.20.08.23.1800.d2_rc___release","revision":revision,"inputs":packages,
        "recipes":snapshot.request.recipes,"manifest":manifest,"abilities":receipt,"ui":ui,
        "limits":"Package readback. Rendered selection, cooldown, activation and cleanup need in-game acceptance."
    })).unwrap()).unwrap();
    eprintln!("Ability color readback: {}", build.run_directory.display());
}

fn check_stock_rows(stock: &BTreeMap<u32, Vec<u8>>, written: &BTreeMap<u32, Vec<u8>>) {
    for (key, row) in stock {
        assert_eq!(
            &written[key], row,
            "stock glyph {key:08X} retains its art and color"
        );
    }
}

fn saved_recipe(
    base: &sundial::investment::SubclassSummary,
    index: usize,
    (color, generated): (Option<[u8; 3]>, bool),
) -> crate::WeaponRecipe {
    let mut recipe = crate::WeaponRecipe::new_unbound_kind(crate::ItemKind::Subclass).unwrap();
    recipe.set_donor(base.hash, base.name.clone());
    recipe
        .rename_authored_item(format!("HUD Color {index}"))
        .unwrap();
    recipe.overrides.subclass_abilities = Some(SubclassAbilities {
        hud_color: color.or(Some([11, 22, 33])),
        ..Default::default()
    });
    recipe.overrides.subclass_icon = generated.then(GeneratedIcon::default);
    // A hue shift left from editing the base icon, which the generated icon must not take.
    if generated {
        recipe.overrides.icon_edit.hue_shift_degrees = 105;
    }
    let saved = serde_json::to_vec(&recipe).unwrap();
    let mut loaded: crate::WeaponRecipe = serde_json::from_slice(&saved).unwrap();
    if color.is_none() {
        let abilities = loaded.overrides.subclass_abilities.as_mut().unwrap();
        abilities.hud_color = None;
        if abilities.is_empty() {
            loaded.overrides.subclass_abilities = None;
        }
    }
    serde_json::from_slice(&serde_json::to_vec(&loaded).unwrap()).unwrap()
}

/// The sRGB color a glyph row publishes, through the native `1/2.2` loader.
fn published(row: &[u8]) -> [u8; 3] {
    std::array::from_fn(|channel| {
        let start = 32 + channel * 4;
        let linear = f32::from_le_bytes(row[start..start + 4].try_into().unwrap());
        (f64::from(linear).powf(1.0 / 2.2) * 255.0).round() as u8
    })
}

/// The pixels of an icon container's primary texture, following its references without the
/// icon writer: the layer at `+0x14`, its first lane's first texture header, and the header's
/// pixel resource. Only RGBA8, which stock subclass icons use.
fn primary_pixels(manager: &PackageManager, container: u32) -> ([usize; 2], Vec<u8>) {
    let container = manager.read_tag(TagHash(container)).unwrap();
    let layer_tag = u32::from_le_bytes(container[0x14..0x18].try_into().unwrap());
    let layer = manager.read_tag(TagHash(layer_tag)).unwrap();
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
    assert!(matches!(format, 28 | 29), "the icon stays RGBA8");
    let size = [0x0E, 0x10].map(|at| usize::from(u16::from_le_bytes([header[at], header[at + 1]])));
    let data = manager
        .read_tag(TagHash(manager.get_entry(header_tag).unwrap().reference))
        .unwrap();
    (size, data[..size[0] * size[1] * 4].to_vec())
}

/// Checks a Generated Icon's texture: the stock 160 pixels, transparent outside the diamond, the
/// stock grey border at its left corner and `fill` at its middle. Returns those pixels.
fn check_generated_icon(
    manager: &PackageManager,
    container: u32,
    fill: [u8; 3],
) -> serde_json::Value {
    let (size, pixels) = primary_pixels(manager, container);
    assert_eq!(size, [160, 160], "the diamond keeps the stock icon's size");
    let at = |x: usize, y: usize| <[u8; 4]>::try_from(&pixels[(y * 160 + x) * 4..][..4]).unwrap();
    for corner in [at(0, 0), at(159, 0), at(0, 159), at(159, 159)] {
        assert_eq!(corner[3], 0, "outside the diamond is transparent");
    }
    assert_eq!(
        at(2, 80),
        [180, 180, 180, 255],
        "the border is the stock grey"
    );
    let middle = at(80, 80);
    assert_eq!(middle[3], 255);
    for (drawn, expected) in middle.into_iter().zip(fill) {
        assert!(
            drawn.abs_diff(expected) <= 1,
            "the middle is the HUD color: {middle:?} for {fill:?}"
        );
    }
    serde_json::json!({"container": format!("{container:08X}"), "middle": middle, "border": at(2, 80)})
}

/// Independently decode the shared-tag directory and its allocation, not PackageLayout.
fn shared_tags(file: &Path) -> (Vec<u8>, Vec<u8>) {
    let bytes = fs::read(file).unwrap();
    let u32_at = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let u64_at = |at| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()) as usize;
    let start = u32_at(0x110);
    let end = start + u32_at(0x114);
    let count = u64_at(start + 0x30);
    let relative = i64::from_le_bytes(bytes[start + 0x38..start + 0x40].try_into().unwrap());
    let header = (start + 0x38)
        .checked_add_signed(isize::try_from(relative).unwrap())
        .unwrap();
    let capacity = u64_at(header);
    assert!(capacity >= count);
    assert_eq!(u32_at(header - 4), 0x80809FBD);
    assert_eq!(u64_at(header + 8), 0x80809A13);
    assert_eq!(header + 16 + capacity * 8, end);
    let rows = header + 16;
    let padding = bytes[rows + count * 8..end].to_vec();
    assert!(padding.iter().all(|&byte| byte == 0xFF));
    (bytes[rows..rows + count * 8].to_vec(), padding)
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES, SUNDIAL_TEST_ARTIFACTS and SUNDIAL_TEST_REVISION"]
fn subclass_hud_colors_survive_saved_recipes_and_private_package_builds() {
    let packages = crate::test_support::stock_packages();
    let artifacts =
        PathBuf::from(std::env::var_os("SUNDIAL_TEST_ARTIFACTS").expect("artifact root"));
    let revision = std::env::var("SUNDIAL_TEST_REVISION").expect("source revision");
    let ignored = crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let view = crate::workflow::FilteredPackageView::create(&packages, &ignored).unwrap();
    let install = view.path().parent().unwrap();
    fs::write(install.join("destiny2.exe"), []).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let cache = temporary.path().join("catalog.json");
    let baseline = InvestmentCatalog::load_with_cache_path(install, &cache, true, |_| {}).unwrap();
    let subclasses = baseline.subclasses(crate::package_profile::is_stock_item_definition);
    assert!(
        !subclasses.is_empty(),
        "the configured corpus must contain subclasses"
    );
    let base = subclasses
        .iter()
        .find(|subclass| subclass.name == "Voidwalker")
        .unwrap();
    let cases = subclasses
        .iter()
        .map(|base| (base, Some([32, 210, 150])))
        .chain([(base, Some([240, 80, 170])), (base, None)])
        .collect::<Vec<_>>();
    // The second color and the reset draw a Generated Icon, in their own color and in the Super's.
    let generated = subclasses.len()..cases.len();
    let source = open_manager(view.path()).unwrap();
    let stock_rows = rows(&source);
    let stock_glyphs = cases
        .iter()
        .map(|(base, _)| {
            assert!(
                base.entry_entities
                    .get(&10)
                    .and_then(|&entity| glyph(&source, entity))
                    .is_some(),
                "{} needs its shared Super",
                base.name
            );
            [10, 20].map(|entry| {
                (
                    entry,
                    base.entry_entities.get(&entry).copied(),
                    base.entry_entities
                        .get(&entry)
                        .and_then(|&entity| glyph(&source, entity)),
                )
            })
        })
        .collect::<Vec<_>>();
    let recipes = cases
        .iter()
        .enumerate()
        .map(|(index, &(base, color))| {
            saved_recipe(base, index, (color, generated.contains(&index)))
        })
        .collect();
    drop(source);
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: view.path().to_path_buf(),
        staging_root: artifacts.join("subclass-hud"),
        ignore_installed_authored_overlays: false,
        recipes,
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    let registrations = shared_tags(&packages.join("w64_ui_01e3_5.pkg"));
    assert!(
        !registrations.1.is_empty(),
        "the audited UI source exercises spare shared-tag capacity"
    );
    assert_eq!(
        shared_tags(&build.run_directory.join("w64_ui_01e3_6.pkg")),
        registrations
    );
    for artifact in &build.artifacts {
        view.add_overlay(&build.run_directory.join(&artifact.file_name))
            .unwrap();
    }
    let staged = open_manager(view.path()).unwrap();
    let catalog = InvestmentCatalog::load_with_cache_path(install, &cache, true, |_| {}).unwrap();
    let subclasses = catalog.subclasses(|_| true);
    let written_rows = rows(&staged);
    check_stock_rows(&stock_rows, &written_rows);
    let mut private_keys = BTreeSet::new();
    let mut receipt = Vec::new();
    for (index, &(base, color)) in cases.iter().enumerate() {
        let name = format!("HUD Color {index}");
        let authored = subclasses
            .iter()
            .find(|subclass| subclass.name == name)
            .unwrap();
        if generated.contains(&index) {
            let super_key = stock_glyphs[index][0].2.unwrap();
            let fill = color.unwrap_or_else(|| published(&stock_rows[&super_key]));
            let container = catalog.weapon_icon_container(authored.hash).unwrap();
            let pixels = check_generated_icon(&staged, container, fill);
            receipt.push(
                serde_json::json!({"item": name, "base": base.name, "generated_icon": pixels}),
            );
        }
        for (entry, stock_entity, stock_key) in stock_glyphs[index] {
            let Some(stock_key) = stock_key else {
                assert_eq!(
                    authored.entry_entities.get(&entry).copied(),
                    stock_entity,
                    "{} keeps its passive lead node",
                    base.name
                );
                receipt.push(serde_json::json!({"item": name, "base": base.name, "entry": entry, "passive": true}));
                continue;
            };
            let stock_entity = stock_entity.unwrap();
            let entity = authored.entry_entities[&entry];
            let key = glyph(&staged, entity).unwrap();
            let Some(rgb) = color else {
                assert_eq!(key, stock_key, "reset uses the donor glyph");
                assert_eq!(entity, stock_entity);
                continue;
            };
            assert_ne!(entity, stock_entity);
            assert!(!stock_rows.contains_key(&key));
            assert!(
                private_keys.insert(key),
                "each authored Super owns its glyph key"
            );
            let linear = check_color(&written_rows[&key], &stock_rows[&stock_key], rgb);
            assert_eq!(glyph(&staged, stock_entity), Some(stock_key));
            receipt.push(serde_json::json!({
                "item": name, "base": base.name, "entry": entry, "entity": format!("{entity:08X}"),
                "glyph": format!("{key:08X}"), "srgb": rgb, "linear_rgba": linear,
            }));
        }
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&build.manifest_path).unwrap()).unwrap();
    fs::write(build.run_directory.join("subclass-hud-colors.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "source_build": "86657.20.08.23.1800.d2_rc___release",
        "revision": revision, "manifest": manifest,
        "recipes": snapshot.request.recipes,
        "inputs": packages, "readback": receipt,
        "limits": "Package readback only. In-game selection, activation and unequip remain visual acceptance checks.",
    })).unwrap()).unwrap();
    eprintln!("HUD color readback: {}", build.run_directory.display());
}

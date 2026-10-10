//! The glyph an ability's HUD tile shows. An ability's energy controller names it by a key at
//! +0x1C8 of both its instance and its definition. The key is a row of the HUD glyph table
//! `80BC6A79` (class `80804A55`, 112-byte rows sorted by key, the layer tag at row +4), whose layer
//! holds the white art the HUD draws. The Subclass screen shows the node record's icon instead,
//! so changing that icon leaves the HUD as it was.
//!
//! A bank row can name the glyph in its place while its key applies, as the dodges, barricades,
//! rifts and melees do for each variant. The HUD record writer `BB1A20` copies the glyph from
//! runtime +0x1C8 (`BB1BC1`), which is accumulator +0x48, the field such a row writes. The rows
//! apply in ascending order, so the last applied one is the glyph the tile shows.
use std::collections::BTreeSet;

use crate::entity::{weapon_component_binding_hashes, weapon_component_bindings};
use crate::package_payload::{u32_at, u64_at};
use crate::package_runtime::reader::PackageManager;
use tiger_pkg::TagHash;

/// The HUD glyph table, and the class it is checked against.
pub const GLYPH_TABLE: u32 = 0x80BC_6A79;
const TABLE_CLASS: u32 = 0x8080_4A55;
/// Where the glyph key sits in an energy controller's instance and definition.
pub const GLYPH_FIELD: usize = 0x1C8;
const ROWS: usize = 48;
const ROW_SIZE: usize = 112;
/// The modifier class of a bank row naming the glyph, and where its modifier holds the key.
const VARIANT_CLASS: u32 = 0x8080_4532;
const VARIANT_GLYPH: usize = 0x10;

/// A bank row naming the glyph while its key applies: its index, its key, the bank offset of
/// the glyph key it names, and that key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VariantGlyph {
    pub row: u16,
    pub key: u32,
    pub offset: usize,
    pub glyph: u32,
}

/// The rows of `bank` naming a glyph that `keys` apply, in the order they apply, leaving out rows
/// an inner key gates. The last one names the glyph the tile shows.
pub fn variant_glyphs(bank: &[u8], keys: &[u32]) -> Result<Vec<VariantGlyph>, String> {
    selected_variants(bank, Some(keys))
}

/// Every glyph override of an attached ability, including conditionally selected variants.
/// Changing their glyphs leaves their keys, gates and activation conditions intact.
pub fn all_variant_glyphs(bank: &[u8]) -> Result<Vec<VariantGlyph>, String> {
    selected_variants(bank, None)
}

fn selected_variants(bank: &[u8], keys: Option<&[u32]>) -> Result<Vec<VariantGlyph>, String> {
    if !crate::ability::bank::has_property_rows(bank)? {
        return Ok(Vec::new());
    }
    let rows = crate::ability::bank::property_rows(bank)?;
    let modifiers = crate::ability::bank::row_modifiers(bank)?;
    let mut found = Vec::new();
    for (index, (row, &(block, gated))) in rows.iter().zip(&modifiers).enumerate() {
        if row.modifier_class != VARIANT_CLASS
            || keys.is_some_and(|keys| gated || !keys.contains(&row.key))
        {
            continue;
        }
        let offset = block + VARIANT_GLYPH;
        found.push(VariantGlyph {
            row: u16::try_from(index).map_err(|_| "The bank has too many rows")?,
            key: row.key,
            offset,
            glyph: u32_at(bank, offset)?,
        });
    }
    Ok(found)
}

/// Where an ability names its glyph: one resource of a component binding, its instance and
/// definition offsets in the owner, and the key both hold.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlyphSite {
    pub owner: u32,
    pub binding_hash: u32,
    pub resource_index: usize,
    pub instance: usize,
    pub definition: usize,
    pub key: u32,
}

impl GlyphSite {
    /// The key's offsets from the start of the resource: in the instance, then the definition.
    pub fn offsets(&self) -> Result<[u32; 2], String> {
        let definition = (self.definition + GLYPH_FIELD)
            .checked_sub(self.instance)
            .and_then(|offset| u32::try_from(offset).ok())
            .ok_or("The glyph's definition lies before its instance")?;
        Ok([GLYPH_FIELD as u32, definition])
    }
}

/// The keys of the HUD glyph table's rows.
pub fn glyph_keys(manager: &PackageManager) -> Result<BTreeSet<u32>, String> {
    if manager
        .get_entry(TagHash(GLYPH_TABLE))
        .is_none_or(|entry| entry.reference != TABLE_CLASS)
    {
        return Err(format!("HUD glyph table 0x{GLYPH_TABLE:08X} has changed"));
    }
    let table = manager
        .read_tag(TagHash(GLYPH_TABLE))
        .map_err(|error| error.to_string())?;
    let count = usize::try_from(u64_at(&table, 8)?).map_err(|_| "HUD glyph count overflow")?;
    if ROWS + count * ROW_SIZE != table.len() {
        return Err("The HUD glyph table has an unexpected layout".into());
    }
    (0..count)
        .map(|row| u32_at(&table, ROWS + row * ROW_SIZE))
        .collect()
}

/// The glyph `entity`'s energy controller names: the one component resource whose instance and
/// definition hold the same glyph key at [`GLYPH_FIELD`]. `None` for an entity with none.
pub fn glyph_site(manager: &PackageManager, entity: &[u8]) -> Result<Option<GlyphSite>, String> {
    let keys = glyph_keys(manager)?;
    glyph_site_with_keys(manager, entity, &keys)
}

fn glyph_site_with_keys(
    manager: &PackageManager,
    entity: &[u8],
    keys: &BTreeSet<u32>,
) -> Result<Option<GlyphSite>, String> {
    let mut found: Option<GlyphSite> = None;
    for binding_hash in weapon_component_binding_hashes(entity)? {
        for binding in weapon_component_bindings(entity, binding_hash)? {
            let owner = manager
                .read_tag(TagHash(binding.owner_tag))
                .map_err(|error| error.to_string())?;
            let Ok(instance) = usize::try_from(binding.resource_offset) else {
                continue;
            };
            // An instance names its definition by offset at +8. A component without that shape
            // reads out of range here and is passed over.
            let Some(definition) = instance
                .checked_add(8)
                .and_then(|at| u64_at(&owner, at).ok())
                .and_then(|definition| usize::try_from(definition).ok())
            else {
                continue;
            };
            let key = |block: usize| {
                block
                    .checked_add(GLYPH_FIELD)
                    .and_then(|at| u32_at(&owner, at).ok())
            };
            let (Some(own), Some(defined)) = (key(instance), key(definition)) else {
                continue;
            };
            if own != defined || !keys.contains(&own) {
                continue;
            }
            let site = GlyphSite {
                owner: binding.owner_tag,
                binding_hash,
                resource_index: binding.resource_index,
                instance,
                definition,
                key: own,
            };
            match found {
                Some(existing) if existing.key != site.key => {
                    return Err(format!(
                        "The entity names two HUD glyphs, 0x{:08X} and 0x{:08X}",
                        existing.key, site.key
                    ));
                }
                Some(_) => {}
                None => found = Some(site),
            }
        }
    }
    Ok(found)
}

/// Attached graphs sharing a controller are variants of one presentation choice. Each graph
/// still takes a private copy when authored. Reachability does not prove which input or gameplay
/// state activates it, and discovery never adds a HUD slot.
#[derive(Clone, Debug)]
pub struct AttachedGlyph {
    pub graphs: Vec<u32>,
    pub site: GlyphSite,
    pub target: Option<crate::ability::AbilityTarget>,
}

/// The descendants with HUD controllers, through the same bounded graph and impact-table
/// traversal used by ability authoring. Neither a weapon hash nor a guessed graph name selects
/// them. A malformed mounted source is an error, not an empty list.
pub fn attached_glyphs(
    manager: &PackageManager,
    entity: u32,
    depth: usize,
) -> Result<Vec<AttachedGlyph>, String> {
    let keys = glyph_keys(manager)?;
    let mut found = Vec::<AttachedGlyph>::new();
    for (graph, payload) in super::palette::ability_graphs(manager, entity, depth)? {
        if graph == entity {
            continue;
        }
        let Some(site) = glyph_site_with_keys(manager, &payload, &keys)? else {
            continue;
        };
        let same_controller = |group: &&mut AttachedGlyph| {
            (
                group.site.owner,
                group.site.instance,
                group.site.definition,
                group.site.key,
            ) == (site.owner, site.instance, site.definition, site.key)
        };
        if let Some(group) = found.iter_mut().find(same_controller) {
            group.graphs.push(graph);
        } else {
            found.push(AttachedGlyph {
                graphs: vec![graph],
                site,
                target: super::modifier::entity_bank(&payload)?
                    .and_then(super::modifier::bank_slot),
            });
        }
    }
    Ok(found)
}

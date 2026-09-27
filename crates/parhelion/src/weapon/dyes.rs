//! Custom shader dyes.
//!
//! A shader's dye rows name dyes by their row in the art-dye table (investment globals slot 67).
//! A row holds a key that the entity assignments map to a relation, which names the dye, whose
//! scope carries 27 constant vectors (`crate::dye` names where each value sits) and binds its
//! detail texture pair. The scope's constant buffer repeats those vectors for a static dye.
//!
//! A custom dye copies one stock dye's records into an asset group of its own: the relation and
//! its shared-tag companion, the dye, the scope and the constant buffer's header and data. The
//! edits write their values into the scope's vectors and rebind its detail textures. The
//! companion lists what loads with the relation, which is those records and the textures the
//! edited scope binds, as a stock companion does. A new table row names the copy, the entity
//! assignments map its key to the new relation, and the shader's rows point at the new row.
use super::resolve::ResolvedWeapon;
use super::sources::ProjectSources;
use super::*;
use crate::dye::{
    DyeChannel, DyeEdit, DyeSurface, DyeTextureEdit, slot_of_key, surface_edit, texture_edit,
};
use crate::shared_tag_memory::{SharedTagDependencies, build_shared_tag_companion_payload};
use sundial::package_authoring::weapon_entity::{
    append_weapon_entity_assignment, weapon_entity_assignment,
};

const DYE_TABLE_CLASS: u32 = 0x8080_5DE8;
const DYE_TABLE_ROW_CLASS: u32 = 0x8080_5DEC;
const DYE_TABLE_ROW_SIZE: usize = 8;
const DYE_TABLE_KEY_OFFSET: usize = 4;
const RELATION_CLASS: u32 = 0x8080_744A;
const RELATION_DYE_OFFSET: usize = 0x10;
const DYE_CLASS: u32 = 0x8080_71CD;
const DYE_SCOPE_OFFSET: usize = 0x0C;
const SCOPE_CLASS: u32 = 0x8080_71F3;
const SCOPE_TEXTURES: usize = 0x40;
const SCOPE_TEXTURE_ROW_CLASS: u32 = 0x8080_7211;
const SCOPE_TEXTURE_ROW_SIZE: usize = 8;
const SCOPE_VECTORS: usize = 0x88;
const SCOPE_VECTOR_CLASS: u32 = 0x8080_0090;
const SCOPE_BUFFER_OFFSET: usize = 0xBC;
const VECTOR_COUNT: usize = 27;
const VECTOR_SIZE: usize = 16;
/// The detail texture pairs a scope binds: armor dyes at 3 and 4, cloth dyes at 5 and 6 and suit
/// dyes at 7 and 8, the detail texture first.
const DETAIL_SLOTS: [(u32, u32); 3] = [(3, 4), (5, 6), (7, 8)];

/// The art-dye table the shader rows index.
pub(super) struct DyeTable {
    pub(super) tag: TagHash,
    payload: Vec<u8>,
    count: usize,
    rows: usize,
}

impl DyeTable {
    pub(super) fn load(manager: &PackageManager, tag: TagHash) -> AuthoringResult<Self> {
        if manager
            .get_entry(tag)
            .is_none_or(|entry| entry.reference != DYE_TABLE_CLASS)
        {
            return Err(invalid(format!(
                "Art-dye table {tag} has an unexpected class"
            )));
        }
        let payload = read_tag(manager, tag, "art-dye table")?;
        let (count, _, rows, class) = array_at(&payload, 8)?;
        if class != DYE_TABLE_ROW_CLASS || rows + count * DYE_TABLE_ROW_SIZE != payload.len() {
            return Err(invalid("The art-dye table is not a terminal native array"));
        }
        Ok(Self {
            tag,
            payload,
            count,
            rows,
        })
    }

    fn key(&self, index: u16) -> AuthoringResult<u32> {
        if usize::from(index) >= self.count {
            return Err(invalid(format!("Dye {index} is outside the art-dye table")));
        }
        read_u32(
            &self.payload,
            self.rows + usize::from(index) * DYE_TABLE_ROW_SIZE + DYE_TABLE_KEY_OFFSET,
        )
    }

    /// The table with a row for each custom dye, named by its key, or `None` without any.
    pub(super) fn with(&self, plan: &DyePlan) -> AuthoringResult<Option<Vec<u8>>> {
        if plan.dyes.is_empty() {
            return Ok(None);
        }
        let mut table = self.payload.clone();
        let (count, header, _, _) = array_at(&table, 8)?;
        for (offset, dye) in plan.dyes.iter().enumerate() {
            if usize::from(dye.index) != count + offset {
                return Err(validation("Custom dye rows are out of order"));
            }
            table.extend_from_slice(&dye.key.to_le_bytes());
            table.extend_from_slice(&dye.key.to_le_bytes());
        }
        set_array_count(&mut table, 8, header, count + plan.dyes.len())?;
        let size = table.len() as u64;
        write_u64(&mut table, 0, size)?;
        Ok(Some(table))
    }
}

/// A stock dye's records, from the relation the entity assignments name to the constant buffer.
struct StockDye {
    relation: (TagHash, Vec<u8>),
    dye: (TagHash, Vec<u8>),
    scope: (TagHash, Vec<u8>),
    buffer_header: (TagHash, Vec<u8>),
    buffer_data: (TagHash, Vec<u8>),
    /// Where the scope's 27 inline vectors start.
    vectors: usize,
}

impl StockDye {
    fn read(sources: &ProjectSources, table: &DyeTable, index: u16) -> AuthoringResult<Self> {
        let manager = &sources.manager;
        let key = table.key(index)?;
        let relation = weapon_entity_assignment(&sources.stock_entity_assignments, key)
            .map_err(invalid)?
            .ok_or_else(|| invalid(format!("Dye {index} has no entity assignment")))?;
        let typed = |tag: u32, class: u32, label: &str| -> AuthoringResult<(TagHash, Vec<u8>)> {
            let tag = TagHash(tag);
            if manager
                .get_entry(tag)
                .is_none_or(|entry| entry.reference != class)
            {
                return Err(invalid(format!(
                    "Dye {index} {label} {tag} is not class 0x{class:08X}"
                )));
            }
            Ok((tag, read_tag(manager, tag, label)?))
        };
        let relation = typed(relation, RELATION_CLASS, "relation")?;
        let dye = typed(
            read_u32(&relation.1, RELATION_DYE_OFFSET)?,
            DYE_CLASS,
            "dye",
        )?;
        let scope = typed(read_u32(&dye.1, DYE_SCOPE_OFFSET)?, SCOPE_CLASS, "scope")?;
        let (count, _, vectors, class) = array_at(&scope.1, SCOPE_VECTORS)?;
        if count != VECTOR_COUNT || class != SCOPE_VECTOR_CLASS {
            return Err(invalid(format!(
                "Dye {index} does not use the 27-vector Shadowkeep layout"
            )));
        }
        // The buffer header's entry names its data, and the data's names its header.
        let header_tag = TagHash(read_u32(&scope.1, SCOPE_BUFFER_OFFSET)?);
        let data_tag = manager
            .get_entry(header_tag)
            .map(|entry| TagHash(entry.reference))
            .ok_or_else(|| invalid(format!("Dye {index} has no constant buffer")))?;
        if manager
            .get_entry(data_tag)
            .is_none_or(|entry| entry.reference != header_tag.0)
        {
            return Err(invalid(format!(
                "Dye {index} constant buffer data does not name its header"
            )));
        }
        let buffer_header = (
            header_tag,
            read_tag(manager, header_tag, "dye buffer header")?,
        );
        let buffer_data = (data_tag, read_tag(manager, data_tag, "dye buffer data")?);
        if buffer_data.1.len() != VECTOR_COUNT * VECTOR_SIZE {
            return Err(invalid(format!(
                "Dye {index} constant buffer holds {} bytes",
                buffer_data.1.len()
            )));
        }
        bindings(&scope.1).map_err(|error| invalid(format!("Dye {index} {error}")))?;
        Ok(Self {
            relation,
            dye,
            scope,
            buffer_header,
            buffer_data,
            vectors,
        })
    }
}

/// A scope's texture bindings: each row's slot and texture tag.
fn bindings(scope: &[u8]) -> AuthoringResult<Vec<(u32, u32)>> {
    if read_u64(scope, SCOPE_TEXTURES)? == 0 {
        return Ok(Vec::new());
    }
    let (count, _, rows, class) = array_at(scope, SCOPE_TEXTURES)?;
    if class != SCOPE_TEXTURE_ROW_CLASS {
        return Err(invalid("texture bindings are unexpected"));
    }
    (0..count)
        .map(|slot| rows + slot * SCOPE_TEXTURE_ROW_SIZE)
        .map(|row| Ok((read_u32(scope, row)?, read_u32(scope, row + 4)?)))
        .collect()
}

/// The texture headers a scope binds and their data, which load with the dye's relation.
fn bound_textures(manager: &PackageManager, scope: &[u8]) -> AuthoringResult<Vec<TagHash>> {
    let mut textures = Vec::new();
    for (_, tag) in bindings(scope)? {
        let header = TagHash(tag);
        if matches!(header.0, 0 | u32::MAX) {
            continue;
        }
        let data = manager
            .get_entry(header)
            .map(|entry| TagHash(entry.reference))
            .ok_or_else(|| invalid(format!("Dye texture {header} is missing")))?;
        textures.extend([header, data]);
    }
    Ok(textures)
}

/// Binds `tag` at `slot`, replacing the slot's texture or adding the binding.
fn bind_texture(
    manager: &PackageManager,
    scope: Vec<u8>,
    slot: u32,
    tag: u32,
) -> AuthoringResult<Vec<u8>> {
    if manager
        .get_entry(TagHash(tag))
        .is_none_or(|entry| entry.file_type != 32 || !matches!(entry.file_subtype, 1..=3))
    {
        return Err(invalid(format!(
            "Texture 0x{tag:08X} is not a texture in the installed packages"
        )));
    }
    let mut scope = scope;
    let mut rows = bindings(&scope)?;
    if read_u64(&scope, SCOPE_TEXTURES)? != 0 {
        let (_, _, start, _) = array_at(&scope, SCOPE_TEXTURES)?;
        if let Some(index) = rows.iter().position(|(bound, _)| *bound == slot) {
            write_u32(&mut scope, start + index * SCOPE_TEXTURE_ROW_SIZE + 4, tag)?;
            return Ok(scope);
        }
    }
    rows.push((slot, tag));
    let bytes = rows
        .iter()
        .flat_map(|(slot, tag)| slot.to_le_bytes().into_iter().chain(tag.to_le_bytes()))
        .collect::<Vec<_>>();
    crate::tag_payload::append_native_array(
        &mut scope,
        SCOPE_TEXTURES,
        SCOPE_TEXTURE_ROW_CLASS,
        rows.len(),
        &bytes,
    )?;
    synchronize_payload_size(scope)
}

/// One authored dye: a stock dye's records with new surface values.
pub(super) struct CustomDye {
    /// Its row in the art-dye table.
    pub(super) index: u16,
    /// The entity-assignment key of its relation.
    pub(super) key: u32,
    stock: StockDye,
    /// The scope with its edited vectors and bindings, and the constant buffer data to match.
    scope: Vec<u8>,
    buffer_data: Vec<u8>,
    /// The texture headers and data the edited scope binds.
    textures: Vec<TagHash>,
}

/// Every custom dye a build adds, in table order.
#[derive(Default)]
pub(super) struct DyePlan {
    pub(super) dyes: Vec<CustomDye>,
}

/// Gives each shader with dye edits custom dyes and points its rows at them. Dyes with the same
/// stock source and edits are shared, and a stock dye with no edits is reused as it is.
pub(super) fn plan(
    sources: &ProjectSources,
    resolved: &mut [ResolvedWeapon],
) -> AuthoringResult<DyePlan> {
    let table = &sources.dye_table;
    let mut plan = DyePlan::default();
    let mut shared = BTreeMap::<(u16, Vec<u8>), u16>::new();
    for donor in resolved.iter_mut() {
        let overrides = &donor.weapon.overrides;
        if donor.weapon.kind != ItemKind::Shader
            || (overrides.dye_edits.is_empty() && overrides.dye_texture_edits.is_empty())
        {
            continue;
        }
        let edits = overrides.dye_edits.clone();
        let texture_edits = overrides.dye_texture_edits.clone();
        let mut arrays = match &donor.weapon.overrides.render_dye_rows {
            Some(arrays) => arrays.clone(),
            None => gear::shader_dye_rows(&donor.definition)?,
        };
        for row in arrays.iter_mut().flatten() {
            let Some((gear, channel)) = slot_of_key(row.channel_index) else {
                continue;
            };
            // What this gear type's dye takes: its own edits over the ones for every gear type.
            // Edits for every gear type alone resolve to themselves, so older recipes keep their
            // custom dye keys.
            let channel_edits = DyeSurface::ALL
                .into_iter()
                .filter_map(|surface| surface_edit(&edits, gear, channel, surface))
                .collect::<Vec<_>>();
            let textures = texture_edit(&texture_edits, gear, channel);
            if channel_edits.is_empty() && textures.is_none() {
                continue;
            }
            // Surface edits alone sign as they always did, so older recipes keep their dye keys.
            let signature = match textures {
                None => serde_json::to_vec(&channel_edits),
                Some(textures) => serde_json::to_vec(&(&channel_edits, textures)),
            }
            .map_err(|error| invalid(format!("Dye edits: {error}")))?;
            let source = row.dye_reference_index;
            if let Some(&index) = shared.get(&(source, signature.clone())) {
                row.dye_reference_index = index;
                continue;
            }
            let index = u16::try_from(table.count + plan.dyes.len())
                .map_err(|_| invalid("The art-dye table would exceed 65,535 rows"))?;
            let stock = StockDye::read(sources, table, source)
                .map_err(|error| donor.weapon.in_recipe(error))?;
            let (scope, buffer_data, textures) =
                edited(&sources.manager, &stock, channel, &channel_edits, textures)
                    .map_err(|error| donor.weapon.in_recipe(error))?;
            plan.dyes.push(CustomDye {
                index,
                key: fnv1_name_hash(&format!(
                    "parhelion/dye/{source}/{channel:?}/{}",
                    String::from_utf8_lossy(&signature)
                )),
                stock,
                scope,
                buffer_data,
                textures,
            });
            shared.insert((source, signature), index);
            row.dye_reference_index = index;
        }
        donor.dye_rows = Some(arrays);
    }
    Ok(plan)
}

/// The stock scope and constant buffer with the edits written into both copies of the vectors and
/// the scope's detail textures rebound, and the textures the edited scope binds. An animated
/// dye's buffer starts empty and the game fills it, so only its vectors change.
fn edited(
    manager: &PackageManager,
    stock: &StockDye,
    channel: DyeChannel,
    edits: &[DyeEdit],
    textures: Option<DyeTextureEdit>,
) -> AuthoringResult<(Vec<u8>, Vec<u8>, Vec<TagHash>)> {
    let mut scope = stock.scope.1.clone();
    let mut data = stock.buffer_data.1.clone();
    let inline = scope
        .get(stock.vectors..stock.vectors + VECTOR_COUNT * VECTOR_SIZE)
        .ok_or_else(|| invalid("Dye vectors extend past their scope"))?
        .to_vec();
    let mirrored = inline == data;
    let writes = edits
        .iter()
        .flat_map(DyeEdit::writes)
        .chain(textures.iter().flat_map(DyeTextureEdit::writes));
    for (vector, lane, value) in writes {
        if vector >= VECTOR_COUNT || lane >= 4 {
            return Err(invalid("A dye edit writes past the dye's vectors"));
        }
        let at = vector * VECTOR_SIZE + lane * 4;
        write_bytes(&mut scope, stock.vectors + at, &value.to_le_bytes())?;
        if mirrored {
            write_bytes(&mut data, at, &value.to_le_bytes())?;
        }
    }
    if let Some(textures) = textures {
        // The scope's own detail pair, or its channel's when it binds none.
        let bound = bindings(&scope)?;
        let (detail, normal) = DETAIL_SLOTS
            .into_iter()
            .find(|(detail, normal)| {
                bound
                    .iter()
                    .any(|(slot, _)| slot == detail || slot == normal)
            })
            .unwrap_or(DETAIL_SLOTS[channel.index()]);
        for (slot, tag) in [(detail, textures.detail), (normal, textures.normal)] {
            if let Some(tag) = tag {
                scope = bind_texture(manager, scope, slot, tag)?;
            }
        }
    }
    let textures = bound_textures(manager, &scope)?;
    Ok((scope, data, textures))
}

/// Adds each custom dye's records to an asset group of its own and maps its key to its relation.
pub(super) fn author(
    manager: &PackageManager,
    plan: &DyePlan,
    packages: &mut crate::asset_packages::AssetPackages,
    mut assignments: Vec<u8>,
) -> AuthoringResult<Vec<u8>> {
    if plan.dyes.is_empty() {
        return Ok(assignments);
    }
    // Any stock companion carries the envelope a companion's dependency list is written into.
    let envelope = read_tag(
        manager,
        RUNTIME_DEPENDENCY_COMPANION,
        "shared-tag companion envelope",
    )?;
    // The dependency list grows the envelope by far less than a block, as linking assumes.
    let companion_bound = envelope
        .len()
        .checked_add(crate::format::BLOCK_SIZE)
        .ok_or_else(|| invalid("Companion size overflow"))?;
    for dye in &plan.dyes {
        let stock = &dye.stock;
        let index = packages.reserve_group([
            stock.relation.1.len(),
            companion_bound,
            stock.dye.1.len(),
            dye.scope.len(),
            stock.buffer_header.1.len(),
            dye.buffer_data.len(),
        ])?;
        let package = &mut packages.packages[index];
        let first = package.tags.len();
        let tag = |offset: usize| -> AuthoringResult<TagHash> {
            u16::try_from(first + offset)
                .map(|entry| TagHash::new(package.id, entry))
                .map_err(|_| invalid("A custom dye's asset tag index does not fit 16 bits"))
        };
        let [relation, companion, dye_tag, scope, header, data] = [0, 1, 2, 3, 4, 5].map(tag);
        let (relation, companion, dye_tag, scope, header, data) =
            (relation?, companion?, dye_tag?, scope?, header?, data?);
        let mut relation_payload = stock.relation.1.clone();
        write_u32(&mut relation_payload, RELATION_DYE_OFFSET, dye_tag.0)?;
        let mut dye_payload = stock.dye.1.clone();
        write_u32(&mut dye_payload, DYE_SCOPE_OFFSET, scope.0)?;
        let mut scope_payload = dye.scope.clone();
        write_u32(&mut scope_payload, SCOPE_BUFFER_OFFSET, header.0)?;
        let dependencies = [relation, companion, dye_tag, scope, header, data]
            .into_iter()
            .chain(dye.textures.iter().copied())
            .map(u32::from)
            .collect::<SharedTagDependencies>();
        let companion_payload =
            build_shared_tag_companion_payload(&envelope, companion, relation, &dependencies)?;
        let spec = |template: TagHash, payload: Vec<u8>| NewTagSpec {
            template_tag: template,
            payload,
            storage: crate::NewTagStorageMode::InheritTemplate,
        };
        package.tags.extend([
            spec(stock.relation.0, relation_payload),
            spec(RUNTIME_DEPENDENCY_COMPANION, companion_payload),
            spec(stock.dye.0, dye_payload),
            spec(stock.scope.0, scope_payload),
            spec(stock.buffer_header.0, stock.buffer_header.1.clone()),
            spec(stock.buffer_data.0, dye.buffer_data.clone()),
        ]);
        // The buffer header and data name each other in their entries.
        package.references.extend([
            crate::NewTagReferenceOverride {
                new_tag_ordinal: first + 4,
                reference: crate::NewTagReference::Appended(first + 5),
            },
            crate::NewTagReferenceOverride {
                new_tag_ordinal: first + 5,
                reference: crate::NewTagReference::Appended(first + 4),
            },
        ]);
        assignments =
            append_weapon_entity_assignment(assignments, dye.key, relation.0).map_err(invalid)?;
    }
    Ok(assignments)
}

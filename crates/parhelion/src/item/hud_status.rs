//! HUD statuses of the project's own, shown by the Status Icon of an effect's graph.
//!
//! A Status Icon (`0x80804211`) names its status by hash, in its data (`0x80804212`) at
//! [`NAME_HASH`]. The HUD finds the status's icon in the status table [`TABLE`] by that hash, and
//! its name as the string with that hash, which the stock statuses keep in banks such as
//! [`NAMES`]. A status of the project's own takes a hash after every name in [`NAMES`], its name
//! there in every locale, and a row in [`TABLE`] copying the row of the stock status whose icon
//! it shows. The graph's private copy then names the new hash, as the program's other asset
//! patches change their owners.
use super::*;
use crate::{
    NewTagReference, NewTagReferenceOverride, appended_tags::AppendedTagAllocator,
    package_profile::PARHELION_ASSET_PACKAGE_ID, tag_payload::read_u16,
};
use sundial::package_authoring::sandbox_perk::program::{
    Asset, HudStatus, NativeAssetPatch, NativeAssetResourcePatch, Program,
};

/// The HUD status table, in `w64_ui_02af`.
const TABLE: TagHash = TagHash(0x80D5_FFD3);
const TABLE_CLASS: u32 = 0x8080_4A55;
/// The bank holding the names of seven stock statuses, Arc Shield among them. Its header is in
/// `w64_globals_06dc` and its thirteen locales in `w64_globals_03ab`. Two HUD resources
/// (`80BC62C2`, `80BC62C3`) name it.
const NAMES: TagHash = TagHash(0x815B_961E);
const NAMES_CLASS: u32 = 0x8080_9A88;
const STATUS_ICON: u32 = 0x8080_4211;
const STATUS_ICON_DATA: u32 = 0x8080_4212;
/// The class of the icon layer a status table row names.
const ICON_LAYER_CLASS: u32 = 0x8080_4A69;
/// The status's name hash, from the start of the Status Icon data.
const NAME_HASH: usize = 0x80;

/// The stock table and names a status of the project's own joins.
struct Stock {
    table: Vec<u8>,
    rows: BTreeMap<u32, Vec<u8>>,
    names: Vec<u8>,
    name_hashes: Vec<u32>,
}

fn stock(manager: &PackageManager) -> AuthoringResult<Stock> {
    if TABLE.pkg_id() != crate::package_profile::HUD_STATUS_TABLE_PACKAGE_ID
        || NAMES.pkg_id() != crate::package_profile::HUD_STATUS_NAME_PACKAGE_ID
        || manager
            .get_entry(TABLE)
            .is_none_or(|entry| entry.reference != TABLE_CLASS)
        || manager
            .get_entry(NAMES)
            .is_none_or(|entry| entry.reference != NAMES_CLASS)
    {
        return Err(invalid("The HUD status table or names moved"));
    }
    let table = read_tag(manager, TABLE, "HUD status table")?;
    let rows = crate::hud_icon::assets::table_rows(&table)?;
    let names = read_tag(manager, NAMES, "HUD status names")?;
    let (count, _, hashes, _) = array_at(&names, 8)?;
    let name_hashes = (0..count)
        .map(|index| read_u32(&names, hashes + index * 4))
        .collect::<AuthoringResult<Vec<_>>>()?;
    Ok(Stock {
        table,
        rows,
        names,
        name_hashes,
    })
}

/// The hash a status of the project's own takes. It follows every stock name in [`NAMES`], so
/// its name appends there in hash order, and it is apart from every row of [`TABLE`].
fn hash(stock: &Stock, status: &HudStatus) -> AuthoringResult<u32> {
    let last = *stock
        .name_hashes
        .last()
        .ok_or_else(|| invalid("The HUD status names are empty"))?;
    // An image of the author's own makes a status of its own, even under a name another has.
    let image = status.image.as_deref().map_or_else(String::new, |image| {
        format!(
            "/image-{:08X}",
            sundial::package_authoring::fnv1_name_hash(image)
        )
    });
    for salt in 0u32..u32::MAX {
        let hash = sundial::package_authoring::fnv1_name_hash(&format!(
            "parhelion/hud-status/{:08X}{image}/{}/{salt}",
            status.icon.unwrap_or(0),
            status.name
        ));
        if hash > last
            && hash != sundial::package_authoring::FNV1_EMPTY_HASH
            && !stock.rows.contains_key(&hash)
        {
            return Ok(hash);
        }
    }
    Err(invalid("No HUD status hash is free"))
}

/// One Status Icon of a graph: where its name hash is, as an asset patch addresses it.
struct StatusIcon {
    binding_hash: u32,
    resource_index: u16,
    offset: u32,
    current: u32,
}

fn status_icons(manager: &PackageManager, graph: u32) -> AuthoringResult<Vec<StatusIcon>> {
    let entity = read_tag(manager, TagHash(graph), "HUD status graph")?;
    let mut seen = BTreeSet::new();
    let mut icons = Vec::new();
    for binding_hash in sundial::package_authoring::entity::weapon_component_binding_hashes(&entity)
        .map_err(invalid)?
    {
        for binding in weapon_component_bindings(&entity, binding_hash).map_err(invalid)? {
            if binding.concrete_class != STATUS_ICON
                || !seen.insert((binding.owner_tag, binding.resource_offset))
            {
                continue;
            }
            let owner = read_tag(manager, TagHash(binding.owner_tag), "Status Icon owner")?;
            let instance = usize::try_from(binding.resource_offset)
                .map_err(|_| invalid("Status Icon offset overflow"))?;
            // The instance names its data and the data names the instance back.
            let data = usize::try_from(read_u64(&owner, instance + 8)?)
                .map_err(|_| invalid("Status Icon data offset overflow"))?;
            if read_u32(&owner, instance)? != binding.owner_tag
                || read_u32(&owner, instance + 4)? != STATUS_ICON_DATA
                || read_u32(&owner, data)? != binding.owner_tag
                || read_u32(&owner, data + 4)? != STATUS_ICON
                || read_u64(&owner, data + 8)? != binding.resource_offset
            {
                return Err(invalid(format!(
                    "Status Icon 0x{:08X}@0x{instance:X} does not have the audited data link",
                    binding.owner_tag
                )));
            }
            let at = data + NAME_HASH;
            icons.push(StatusIcon {
                binding_hash,
                resource_index: u16::try_from(binding.resource_index)
                    .map_err(|_| invalid("Status Icon resource index overflow"))?,
                offset: u32::try_from(at - instance)
                    .map_err(|_| invalid("Status Icon name offset overflow"))?,
                current: read_u32(&owner, at)?,
            });
        }
    }
    Ok(icons)
}

/// A stock HUD status, whose icon a status of the project's own may show.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StockStatus {
    pub hash: u32,
    pub name: String,
}

/// Every stock HUD status with a name, in name order. The names are in many string banks, so
/// this reads every bank's English text once.
pub(crate) fn stock_statuses(manager: &PackageManager) -> AuthoringResult<Vec<StockStatus>> {
    let stock = stock(manager)?;
    let mut names = BTreeMap::<u32, String>::new();
    for (tag, _) in manager.get_all_by_reference(NAMES_CLASS) {
        let Ok(header) = manager.read_tag(tag) else {
            continue;
        };
        let Ok((count, _, hashes, _)) = array_at(&header, 8) else {
            continue;
        };
        let wanted = (0..count)
            .filter_map(|index| {
                let hash = read_u32(&header, hashes + index * 4).ok()?;
                (stock.rows.contains_key(&hash) && !names.contains_key(&hash))
                    .then_some((index, hash))
            })
            .collect::<Vec<_>>();
        if wanted.is_empty() {
            continue;
        }
        let Ok(english) = read_u32(&header, LOCALIZATION_DATA_TAG_START) else {
            continue;
        };
        let Ok(data) = manager.read_tag(TagHash(english)) else {
            continue;
        };
        for (index, hash) in wanted {
            if let Ok(name) = decode_localized_value_at(&data, index)
                && !name.trim().is_empty()
            {
                names.insert(hash, name);
            }
        }
    }
    let mut statuses = names
        .into_iter()
        .map(|(hash, name)| StockStatus { hash, name })
        .collect::<Vec<_>>();
    statuses.sort_by(|a, b| a.name.cmp(&b.name).then(a.hash.cmp(&b.hash)));
    Ok(statuses)
}

/// The stock statuses `graph`'s Status Icons show, none when it has no Status Icon.
pub(crate) fn shown_statuses(manager: &PackageManager, graph: u32) -> AuthoringResult<Vec<u32>> {
    let mut shown = status_icons(manager, graph)?
        .into_iter()
        .map(|icon| icon.current)
        .collect::<Vec<_>>();
    shown.sort_unstable();
    shown.dedup();
    Ok(shown)
}

/// The patches that make `asset`'s graph show its HUD status of the project's own, none when it
/// keeps its stock status.
fn asset_patches(
    manager: &PackageManager,
    stock: &Stock,
    asset: &Asset,
) -> AuthoringResult<Vec<NativeAssetResourcePatch>> {
    let Some(status) = &asset.hud_status else {
        return Ok(Vec::new());
    };
    let hash = hash(stock, status)?;
    let icons = status_icons(manager, asset.graph)?;
    if icons.is_empty() {
        return Err(invalid(format!(
            "Effect 0x{:08X} has no Status Icon, so it shows nothing on the HUD",
            asset.graph
        )));
    }
    Ok(icons
        .into_iter()
        .map(|icon| NativeAssetResourcePatch {
            binding_hash: icon.binding_hash,
            resource_index: icon.resource_index,
            offset: icon.offset,
            expected: icon.current.to_le_bytes().to_vec(),
            bytes: hash.to_le_bytes().to_vec(),
            imported_particle: None,
        })
        .collect())
}

/// The patches that make `asset`'s graph show its HUD status of the project's own, for the copy
/// its component settings make.
pub(super) fn value_copy_patches(
    manager: &PackageManager,
    asset: &Asset,
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    if asset.hud_status.is_none() {
        return Ok(Vec::new());
    }
    Ok(asset_patches(manager, &stock(manager)?, asset)?
        .into_iter()
        .map(|patch| WeaponRuntimeResourcePatch {
            binding_hash: patch.binding_hash,
            resource_index: patch.resource_index,
            offset: patch.offset,
            bytes: patch.bytes,
            graph_values: Vec::new(),
            graph_removals: Vec::new(),
            graph_trajectories: None,
        })
        .collect())
}

/// `program`'s asset edits with each HUD status's patches joined to its action's edit. An asset
/// with component settings gets its HUD status in the copy those make instead
/// ([`value_copy_patches`]), since an action's graph is copied once.
pub(super) fn asset_edits(
    manager: &PackageManager,
    program: &Program,
) -> AuthoringResult<Vec<NativeAssetPatch>> {
    let mut edits = program.native_asset_patches.clone();
    if program
        .actions
        .iter()
        .filter_map(|action| action.asset())
        .all(|asset| asset.hud_status.is_none())
    {
        return Ok(edits);
    }
    let stock = stock(manager)?;
    for (action_index, action) in program.actions.iter().enumerate() {
        let Some(asset) = action.asset() else {
            continue;
        };
        if !asset.values.is_empty() {
            continue;
        }
        let patches = asset_patches(manager, &stock, asset)?;
        if patches.is_empty() {
            continue;
        }
        match edits
            .iter_mut()
            .find(|edit| edit.action_index == action_index)
        {
            Some(edit) => edit.patches.extend(patches),
            None => edits.push(NativeAssetPatch {
                action_index,
                source_graph: asset.graph,
                patches,
                appends: Vec::new(),
                remove_owners: Vec::new(),
            }),
        }
    }
    Ok(edits)
}

/// Every asset of the programs that carries a HUD status of the project's own.
fn status_assets<'a>(
    programs: impl IntoIterator<Item = &'a Program>,
) -> Vec<(&'a Asset, &'a HudStatus)> {
    programs
        .into_iter()
        .flat_map(|program| program.actions.iter().filter_map(|action| action.asset()))
        .filter_map(|asset| Some((asset, asset.hud_status.as_ref()?)))
        .collect()
}

/// The stock status whose row a status of the project's own copies: the one it chose, or else
/// the one its graph's Status Icon names.
fn icon_source(
    manager: &PackageManager,
    stock: &Stock,
    asset: &Asset,
    status: &HudStatus,
) -> AuthoringResult<u32> {
    let icon = match status.icon {
        Some(icon) => icon,
        None => {
            let shown = status_icons(manager, asset.graph)?
                .into_iter()
                .map(|icon| icon.current)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let &[icon] = shown.as_slice() else {
                return Err(invalid(format!(
                    "Effect 0x{:08X} shows no single stock status. Choose its HUD icon.",
                    asset.graph
                )));
            };
            icon
        }
    };
    if !stock.rows.contains_key(&icon) {
        return Err(invalid(format!(
            "HUD status {:?} names icon 0x{icon:08X}, which no stock status has",
            status.name
        )));
    }
    Ok(icon)
}

/// The icon layer of a stock status's row.
fn row_layer(stock: &Stock, icon: u32) -> AuthoringResult<TagHash> {
    Ok(TagHash(read_u32(&stock.rows[&icon], 4)?))
}

/// The HUD icon layers the project's own images make, by image and by the stock status whose
/// layer each copies. A copy keeps its stock layer's layout and replaces each texture with a
/// private one holding the image fitted to that texture's size. The nodes join `nodes`, the
/// asset package's first group, as the ammunition HUD icons do.
pub(super) fn author_images<'a>(
    manager: &PackageManager,
    programs: impl IntoIterator<Item = &'a Program>,
    nodes: &mut Vec<NewTagSpec>,
    references: &mut Vec<NewTagReferenceOverride>,
) -> AuthoringResult<BTreeMap<(String, u32), TagHash>> {
    let assets = status_assets(programs);
    let mut layers = BTreeMap::new();
    if assets.iter().all(|(_, status)| status.image.is_none()) {
        return Ok(layers);
    }
    let stock = stock(manager)?;
    let allocator = AppendedTagAllocator::new(PARHELION_ASSET_PACKAGE_ID, 0);
    for (asset, status) in assets {
        let Some(image) = &status.image else {
            continue;
        };
        let icon = icon_source(manager, &stock, asset, status)?;
        if layers.contains_key(&(image.clone(), icon)) {
            continue;
        }
        let source = decode_image(image)?;
        let template = row_layer(&stock, icon)?;
        if manager
            .get_entry(template)
            .is_none_or(|entry| entry.reference != ICON_LAYER_CLASS)
        {
            return Err(invalid(format!(
                "HUD icon layer {template} is not an icon layer"
            )));
        }
        let mut layer = read_tag(manager, template, "HUD icon layer")?;
        let mut textures = 0;
        for at in (0..layer.len().saturating_sub(3)).step_by(4) {
            let texture = TagHash(read_u32(&layer, at)?);
            let Some(entry) = manager.get_entry(texture) else {
                continue;
            };
            if entry.file_type != 32 || entry.file_subtype != 1 {
                continue;
            }
            let header = read_tag(manager, texture, "HUD icon texture")?;
            let width = u32::from(read_u16(&header, 14)?);
            let height = u32::from(read_u16(&header, 16)?);
            let data = TagHash(entry.reference);
            // The stock HUD textures hold plain RGBA (DXGI 28 or its sRGB twin 29), four bytes a
            // pixel, in their own data tag rather than a large buffer.
            if header.len() != 40
                || u64::from(read_u32(&header, 0)?) != u64::from(width * height * 4)
                || !matches!(read_u32(&header, 4)?, 28 | 29)
                || read_u32(&header, 36)? != u32::MAX
                || manager
                    .get_entry(data)
                    .is_none_or(|entry| entry.file_type != 40 || entry.file_subtype != 1)
            {
                return Err(invalid(format!(
                    "HUD icon texture {texture} is not an uncompressed RGBA texture"
                )));
            }
            let ordinal = nodes.len();
            let header_tag = allocator.assigned_tag(ordinal + 1, "HUD status icon", "HUD icon")?;
            nodes.extend([
                NewTagSpec {
                    template_tag: data,
                    payload: crate::image_import::fit(&source, width, height).into_raw(),
                    storage: crate::NewTagStorageMode::InheritTemplate,
                },
                NewTagSpec {
                    template_tag: texture,
                    payload: header,
                    storage: crate::NewTagStorageMode::InheritTemplate,
                },
            ]);
            references.extend([
                NewTagReferenceOverride {
                    new_tag_ordinal: ordinal,
                    reference: NewTagReference::Appended(ordinal + 1),
                },
                NewTagReferenceOverride {
                    new_tag_ordinal: ordinal + 1,
                    reference: NewTagReference::Appended(ordinal),
                },
            ]);
            write_u32(&mut layer, at, header_tag.0)?;
            textures += 1;
        }
        if textures == 0 {
            return Err(invalid(format!(
                "HUD icon layer {template} names no texture"
            )));
        }
        let tag = allocator.assigned_tag(nodes.len(), "HUD status icon", "HUD icon")?;
        nodes.push(NewTagSpec {
            template_tag: template,
            payload: layer,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
        layers.insert((image.clone(), icon), tag);
    }
    Ok(layers)
}

/// The size an embedded HUD status image may take, as the ammunition HUD's does.
const IMAGE_LIMIT: usize = 128 * 1024;

fn decode_image(image: &str) -> AuthoringResult<image::RgbaImage> {
    use base64::Engine as _;
    if image.len() > IMAGE_LIMIT {
        return Err(invalid("A HUD status image exceeds the size limit"));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(image)
        .map_err(|error| invalid(format!("A HUD status image is not base64: {error}")))?;
    crate::image_import::decode_png(&bytes).map_err(invalid)
}

/// The status table and name bank with every HUD status the programs show, none when no program
/// has one. Each status's row copies the row of the stock status whose icon it shows, and names
/// the layer its own image made, from [`author_images`], when it has one.
pub(super) fn replacements<'a>(
    manager: &PackageManager,
    programs: impl IntoIterator<Item = &'a Program>,
    layers: &BTreeMap<(String, u32), TagHash>,
) -> AuthoringResult<Vec<ReplacementSpec>> {
    let assets = status_assets(programs);
    if assets.is_empty() {
        return Ok(Vec::new());
    }
    let stock = stock(manager)?;
    let mut statuses = BTreeMap::<u32, (String, u32, Option<TagHash>)>::new();
    for (asset, status) in assets {
        if status.name.trim().is_empty() {
            return Err(invalid("A HUD status needs a name"));
        }
        let icon = icon_source(manager, &stock, asset, status)?;
        let layer = match &status.image {
            Some(image) => Some(*layers.get(&(image.clone(), icon)).ok_or_else(|| {
                invalid(format!("HUD status {:?} has no icon layer", status.name))
            })?),
            None => None,
        };
        let hash = hash(&stock, status)?;
        let entry = (status.name.clone(), icon, layer);
        if let Some(previous) = statuses.insert(hash, entry.clone())
            && previous != entry
        {
            return Err(invalid("Two HUD statuses took the same hash"));
        }
    }
    let rows = statuses
        .iter()
        .map(|(&hash, (_, icon, layer))| {
            let mut row = stock.rows[icon].clone();
            write_u32(&mut row, 0, hash)?;
            if let Some(layer) = layer {
                write_u32(&mut row, 4, layer.0)?;
            }
            Ok((hash, row))
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    let table = crate::hud_icon::assets::insert_rows(stock.table, stock.rows, rows)?;
    let values = statuses
        .iter()
        .map(|(&hash, (name, _, _))| (hash, name.as_str()))
        .collect::<Vec<_>>();
    let mut replacements = vec![
        ReplacementSpec {
            tag: TABLE,
            payload: synchronize_payload_size(table)?,
        },
        ReplacementSpec {
            tag: NAMES,
            payload: synchronize_payload_size(append_localized_header(&stock.names, &values)?)?,
        },
    ];
    let mut locales = BTreeSet::new();
    for offset in (LOCALIZATION_DATA_TAG_START..LOCALIZATION_DATA_TAG_END).step_by(4) {
        let tag = TagHash(read_u32(&stock.names, offset)?);
        if tag.pkg_id() != crate::package_profile::HUD_STATUS_NAME_DATA_PACKAGE_ID {
            return Err(invalid(format!(
                "HUD status names locale {tag} moved out of package {:04x}",
                crate::package_profile::HUD_STATUS_NAME_DATA_PACKAGE_ID
            )));
        }
        if !locales.insert(tag) {
            continue;
        }
        let data = read_tag(manager, tag, "HUD status names locale")?;
        let (_, _, parts, _) = array_at(&data, 8)?;
        let part = data
            .get(parts..parts + LOCALIZATION_PART_ROW_SIZE)
            .ok_or_else(|| invalid("HUD status names locale has no part"))?
            .to_vec();
        replacements.push(ReplacementSpec {
            tag,
            payload: synchronize_payload_size(append_localized_data(&data, &part, &values)?)?,
        });
    }
    Ok(replacements)
}

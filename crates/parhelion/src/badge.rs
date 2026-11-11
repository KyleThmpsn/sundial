use crate::progression::begin_array_block;
use crate::progression::presentation::*;
mod custom;
mod mark;
mod membership;
mod root;
pub(crate) use custom::{append_custom_badges, append_source_badges};
pub(crate) use mark::artwork as mark;
pub(crate) use membership::set_sunrise_members;
pub(crate) use root::arrange as arrange_root;

use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use std::mem::size_of;

use sundial::package_authoring::mod_content::Importer;
use sundial::package_authoring::{
    investment_schema::{
        ITEM_ICON_CONTAINER_OFFSET, ITEM_ICON_ROW_CLASS, ITEM_ICON_ROW_SIZE,
        OBJECTIVE_STRING_PROGRESS_REFERENCE_OFFSET, OBJECTIVE_STRING_ROW_CLASS,
        OBJECTIVE_STRING_ROW_SIZE, PRESENTATION_NODE_DEFINITION_ROW_CLASS,
        RECORD_DEFINITION_ROW_SIZE as RECORD_ROW_SIZE, RECORD_HASH_OFFSET,
        RECORD_OBJECTIVE_INDEX_ROW_CLASS as RECORD_OBJECTIVE_ROW_CLASS, RECORD_STRING_ROW_SIZE,
    },
    is_valid_package_tag,
};
use tiger_pkg::TagHash;

use crate::{
    AuthoringResult,
    error::{invalid, validation},
    item::STOCK_ITEM_ICON_COUNT,
    progression::{
        BADGES_ROOT_NODE_HASH, BADGES_ROOT_NODE_INDEX, BADGES_ROOT_OBJECTIVE_INDEX,
        NESTED_ARRAY_TRAILER, NUMERIC_ADD_INSTRUCTION, NUMERIC_AND_INSTRUCTION,
        NUMERIC_FLAG_INSTRUCTION, NUMERIC_PROGRAM_ROW_CLASS, NUMERIC_VALUE_INSTRUCTION,
        OBJECTIVE_COMPLETION_VALUE_OFFSET, OBJECTIVE_ROW_CLASS, OBJECTIVE_ROW_SIZE,
        PRESENTATION_NODE_CHILD_NODE_ROW_CLASS, PRESENTATION_NODE_CHILD_NODE_ROW_SIZE,
        PRESENTATION_NODE_CHILD_NODES_OFFSET, PRESENTATION_NODE_COLLECTIBLE_ROW_CLASS,
        PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE, PRESENTATION_NODE_COLLECTIBLES_OFFSET,
        PRESENTATION_NODE_HASH_OFFSET, PRESENTATION_NODE_INDEX_ROW_CLASS,
        PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET, PRESENTATION_NODE_POINTER_FIELDS,
        PRESENTATION_NODE_RECORD_INDEX_OFFSET, PRESENTATION_NODE_ROW_SIZE,
        PRESENTATION_NODE_STRING_DESCRIPTION_REFERENCE_OFFSET,
        PRESENTATION_NODE_STRING_ICON_OFFSET, PRESENTATION_NODE_STRING_NAME_REFERENCE_OFFSET,
        PRESENTATION_NODE_STRING_ROW_CLASS, PRESENTATION_NODE_STRING_ROW_SIZE,
        STOCK_PRESENTATION_NODE_COUNT, numeric_program_layout, numeric_program_stack_depth,
        shared_numeric_instruction_template, validate_shared_expression_table,
    },
    tag_payload::{
        array_at, contains_u32_at_offset, contains_u32_row_key, read_i32, read_u8, read_u16,
        read_u32, read_u64, relative_target, set_array_count, write_i32, write_localized_reference,
        write_relative_pointer, write_u16, write_u32, write_u64,
    },
};

pub(crate) const LUNAR_BADGE_ICON_ROW_INDEX: usize = 0x2F3E;
const LUNAR_BADGE_ICON_KEY: u32 = 0xC2C1_A351;
const SUNRISE_BADGE_ICON_KEY: u32 = 0x5355_4943;

const ACE_BADGE_GROUP_NODE_INDEX: usize = 518;
const ACE_BADGE_TITAN_NODE_INDEX: usize = 519;
const ACE_BADGE_HUNTER_NODE_INDEX: usize = 520;
const ACE_BADGE_WARLOCK_NODE_INDEX: usize = 521;
const ACE_BADGE_RECORD_INDICES: [u16; 4] = [2237, 259, 260, 261];
const STOCK_RECORD_COUNT: usize = 2_242;
const RECORD_ROW_CLASS: u32 = 0x8080_7452;
const RECORD_POINTER_FIELDS: [usize; 9] = [0x18, 0x30, 0x40, 0x68, 0x78, 0x88, 0x98, 0xA8, 0xC0];
const RECORD_OBJECTIVE_DESCRIPTOR_OFFSET: usize = 0x30;
const RECORD_STRING_ROW_CLASS: u32 = 0x8080_5A99;
const RECORD_STRING_POINTER_FIELDS: [usize; 2] = [0x48, 0x58];
const ACE_BADGE_RECORD_HASHES: [u32; 4] = [0x6984_793E, 0x4BB4_68ED, 0x4D7F_9B2B, 0x6756_8088];
const SUNRISE_BADGE_RECORD_HASHES: [u32; 4] = [0x5355_5252, 0x5355_5254, 0x5355_5248, 0x5355_5257];

/// The arrays an objective row owns: its programs at `+0x08`, `+0x18` and `+0x38` and its perk
/// flags at `+0x48`. Rows inserted before the arrays move every one of them.
const OBJECTIVE_POINTER_FIELDS: [usize; 4] = [0x08, 0x18, 0x38, 0x48];
/// The program an objective's text row owns at `+0x30`.
const OBJECTIVE_STRING_POINTER_FIELDS: [usize; 1] = [0x30];
pub(crate) const STOCK_OBJECTIVE_COUNT: usize = 8_483;
const STOCK_BADGES_ROOT_COMPLETION_VALUE: i32 = 19;
const BADGES_ROOT_ACCOUNT_VALUE_INDEX: u16 = 3_824;
const BADGES_ROOT_CHARACTER_VALUE_INDEX: u16 = 3_825;
const ACE_BADGE_OBJECTIVE_TEMPLATE_INDEX: usize = 6_289;
const ACE_BADGE_OBJECTIVE_TEMPLATE_HASH: u32 = 0xE72F_FAB8;
const SUNRISE_BADGE_OBJECTIVE_HASH: u32 = 0x5355_4F42;

const SUNRISE_BADGE_TITAN_NODE_INDEX: usize = STOCK_PRESENTATION_NODE_COUNT + 1;
const SUNRISE_BADGE_HUNTER_NODE_INDEX: usize = STOCK_PRESENTATION_NODE_COUNT + 2;
const SUNRISE_BADGE_WARLOCK_NODE_INDEX: usize = STOCK_PRESENTATION_NODE_COUNT + 3;
pub(crate) const SUNRISE_BADGE_NODE_HASHES: [u32; 4] =
    [0x5355_4E42, 0x5355_4E54, 0x5355_4E48, 0x5355_4E57];
pub(crate) const SUNRISE_BADGE_NAME_HASH: u32 = 0x8582_56D5;
pub(crate) const SUNRISE_BADGE_DESCRIPTION_HASH: u32 = 0x8F21_C532;
pub(crate) const SUNRISE_BADGE_NAME: &str = "Project Sunrise";
pub(crate) const SUNRISE_BADGE_DESCRIPTION: &str =
    "Artifacts forged from a future written outside the lines.";
const SUNRISE_CLASS_FLAG_OPERANDS: [u16; 3] = [0x108, 0xEF, 0x10F];

/// The badge a build's mod adds. The main mod's badge keeps the Project Sunrise identity, and an
/// importer's takes one derived from its mod ID, so each mod's badge holds its own rows, strings
/// and icon whichever mods load beside it.
#[derive(Clone, Copy)]
pub(crate) struct Identity {
    pub node_hashes: [u32; 4],
    pub record_hashes: [u32; 4],
    pub objective_hash: u32,
    pub name_hash: u32,
    pub description_hash: u32,
    pub icon_key: u32,
}

impl Identity {
    const SUNRISE: Self = Self {
        node_hashes: SUNRISE_BADGE_NODE_HASHES,
        record_hashes: SUNRISE_BADGE_RECORD_HASHES,
        objective_hash: SUNRISE_BADGE_OBJECTIVE_HASH,
        name_hash: SUNRISE_BADGE_NAME_HASH,
        description_hash: SUNRISE_BADGE_DESCRIPTION_HASH,
        icon_key: SUNRISE_BADGE_ICON_KEY,
    };

    /// The badge of the family member the build is compiling.
    pub(crate) fn current() -> Self {
        crate::package_profile::member().map_or(Self::SUNRISE, Self::importer)
    }

    /// The badge of `importer`'s items, in its own mod or in one mod for every source.
    pub(crate) fn importer(importer: Importer) -> Self {
        let namespace = format!("badge/{}", importer.id());
        let hash = |field: &str| crate::presentation::text_hash(&namespace, field);
        Self {
            node_hashes: ["group", "titan", "hunter", "warlock"].map(hash),
            record_hashes: [
                "record/group",
                "record/titan",
                "record/hunter",
                "record/warlock",
            ]
            .map(hash),
            objective_hash: hash("objective"),
            name_hash: hash("name"),
            description_hash: hash("description"),
            icon_key: hash("icon"),
        }
    }
}

/// The name and description of the badge the build is compiling: the runtime's for the main mod,
/// and an importer's source game for an importer.
pub(crate) fn text(branding: crate::branding::Branding) -> (&'static str, &'static str) {
    match crate::package_profile::member() {
        None => (branding.name(), branding.description()),
        Some(importer) => source_text(importer),
    }
}

/// The name and description of `importer`'s badge.
pub(crate) fn source_text(importer: Importer) -> (&'static str, &'static str) {
    match importer {
        Importer::HaloReach => ("Halo: Reach", "Items brought over from Halo: Reach."),
        Importer::D2Mot => (
            "Destiny 2: Monument of Triumph",
            "Items brought over from Destiny 2: Monument of Triumph.",
        ),
        Importer::Marathon => ("Marathon", "Items brought over from Marathon."),
        Importer::D1Roi => (
            "Destiny 1: Rise of Iron",
            "Items brought over from Destiny 1: Rise of Iron.",
        ),
    }
}

/// The source badge `weapon`'s item joins in the main mod: in a build without mod folders, where
/// one mod holds every source's items, and for an importer that builds with the main mod. An
/// importer in its own mod gives its items that mod's badge.
pub(crate) fn source_badge(weapon: &crate::item::WeaponCloneSpec) -> Option<Importer> {
    if weapon.joins_sunrise_badge() {
        crate::mod_family::build::source_of(weapon)
    } else {
        None
    }
}

/// Whether `weapon`'s item joins the badge of the mod it builds in: Project Sunrise for the main
/// mod and the importer's own badge for an importer's mod.
pub(crate) fn joins_own_badge(weapon: &crate::item::WeaponCloneSpec) -> bool {
    weapon.joins_sunrise_badge() && crate::mod_family::build::source_of(weapon).is_none()
}

#[derive(Clone, Copy)]
struct Layout {
    node_start: usize,
    record_start: usize,
    objective_start: usize,
    node_hashes: [u32; 4],
    record_hashes: [u32; 4],
    objective_hash: u32,
    name_hash: u32,
    description_hash: u32,
}
impl Layout {
    /// The badge after the stock rows, as each member's build adds it against stock.
    fn of(identity: Identity) -> Self {
        Self {
            node_start: STOCK_PRESENTATION_NODE_COUNT,
            record_start: STOCK_RECORD_COUNT,
            objective_start: STOCK_OBJECTIVE_COUNT,
            node_hashes: identity.node_hashes,
            record_hashes: identity.record_hashes,
            objective_hash: identity.objective_hash,
            name_hash: identity.name_hash,
            description_hash: identity.description_hash,
        }
    }

    #[cfg(test)]
    fn sunrise() -> Self {
        Self::of(Identity::SUNRISE)
    }

    fn current() -> Self {
        Self::of(Identity::current())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SunriseProjectMetadata {
    pub badge_node_hashes: [u32; 4],
    pub badge_name_hash: u32,
    pub badge_description_hash: u32,
    pub badge_icon_tag: TagHash,
    pub watermark_layer_tag: TagHash,
    pub watermarked_icon_containers: Vec<TagHash>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SunriseBadgePlacement {
    pub pages: crate::collection::CollectionPages,
    pub classes: crate::collection::Classes,
    pub donor_collectible_index: usize,
    pub authored_collectible_index: usize,
    pub acquisition: crate::progression::Acquisition,
    /// Where the entry goes among the stock entries of the pages it joins.
    pub position: Option<crate::collection::Position>,
}

pub(crate) struct SunriseBadgeGraph {
    pub nodes: Vec<u8>,
    pub node_strings: Vec<u8>,
    pub objectives: Vec<u8>,
    pub objective_strings: Vec<u8>,
    pub records: Vec<u8>,
    pub record_strings: Vec<u8>,
}

pub(crate) struct SunriseBadgeGraphInput<'a> {
    pub stock_nodes: Vec<u8>,
    pub stock_node_strings: Vec<u8>,
    pub stock_objectives: Vec<u8>,
    pub stock_objective_strings: Vec<u8>,
    pub stock_records: Vec<u8>,
    pub stock_record_strings: Vec<u8>,
    pub shared_expression_pools: &'a [u8],
    pub localization_table_index: u32,
    pub badge_icon_index: u16,
    pub placements: &'a [SunriseBadgePlacement],
}

pub(crate) fn sunrise_badge_collectible_parents(
    pages: crate::collection::CollectionPages,
    classes: crate::collection::Classes,
) -> Vec<u16> {
    classes
        .iter()
        .map(|class| {
            [
                SUNRISE_BADGE_TITAN_NODE_INDEX,
                SUNRISE_BADGE_HUNTER_NODE_INDEX,
                SUNRISE_BADGE_WARLOCK_NODE_INDEX,
            ][usize::from(class)] as u16
        })
        .chain(pages.iter())
        .collect()
}

pub(crate) fn append_badge_icon_row(
    mut icons: Vec<u8>,
    container_tag: TagHash,
) -> AuthoringResult<(Vec<u8>, u16)> {
    let icon_key = Identity::current().icon_key;
    let donor_container = validate_lunar_badge_icon_donor(&icons, icon_key)?;
    let (count, header, rows, class) = array_at(&icons, 8)?;
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(ITEM_ICON_ROW_SIZE)
                .ok_or_else(|| invalid("Item-icon row size overflowed"))?,
        )
        .ok_or_else(|| invalid("Item-icon row range overflowed"))?;
    if class != ITEM_ICON_ROW_CLASS
        || count != STOCK_ITEM_ICON_COUNT
        || rows_end != icons.len()
        || !is_valid_package_tag(container_tag)
        || container_tag == donor_container
        || contains_u32_row_key(&icons, rows, count, ITEM_ICON_ROW_SIZE, icon_key)?
    {
        return Err(invalid(
            "Sunrise badge icon row cannot be appended to the installed item-icon table",
        ));
    }
    let donor = rows + LUNAR_BADGE_ICON_ROW_INDEX * ITEM_ICON_ROW_SIZE;
    let template = icons[donor..donor + ITEM_ICON_ROW_SIZE].to_vec();
    icons.extend_from_slice(&template);
    write_u32(&mut icons, rows_end, icon_key)?;
    write_u32(
        &mut icons,
        rows_end + ITEM_ICON_CONTAINER_OFFSET,
        u32::from(container_tag),
    )?;
    set_array_count(&mut icons, 8, header, count + 1)?;
    let icon_index = u16::try_from(count)
        .map_err(|_| invalid("Sunrise badge icon index does not fit 16 bits"))?;
    Ok((icons, icon_index))
}

pub(crate) fn author_sunrise_badge_graph(
    input: SunriseBadgeGraphInput<'_>,
) -> AuthoringResult<SunriseBadgeGraph> {
    author_graph(input, &Layout::current())
}

fn author_graph(
    input: SunriseBadgeGraphInput<'_>,
    layout: &Layout,
) -> AuthoringResult<SunriseBadgeGraph> {
    let SunriseBadgeGraphInput {
        stock_nodes,
        stock_node_strings,
        stock_objectives,
        stock_objective_strings,
        stock_records,
        stock_record_strings,
        shared_expression_pools,
        localization_table_index,
        badge_icon_index,
        placements,
    } = input;
    let first = placements
        .first()
        .ok_or_else(|| invalid("Project Sunrise requires at least one authored collectible"))?;
    let acquisitions = placements
        .iter()
        .map(|placement| placement.acquisition)
        .collect::<Vec<_>>();
    let (objectives, objective_strings, objective_index) = append_objective(
        stock_objectives,
        stock_objective_strings,
        shared_expression_pools,
        &acquisitions,
        localization_table_index,
        layout,
    )?;
    let (records, record_strings) =
        append_records(stock_records, stock_record_strings, objective_index, layout)?;
    let mut nodes = append_nodes(stock_nodes, first.authored_collectible_index, layout)?;
    for placement in placements.iter().skip(1) {
        for leaf in [
            (layout.node_start + 1),
            (layout.node_start + 2),
            (layout.node_start + 3),
        ] {
            append_collectible_child_to_node(
                &mut nodes,
                leaf,
                first.authored_collectible_index,
                placement.authored_collectible_index,
            )?;
        }
    }
    if layout.node_start == STOCK_PRESENTATION_NODE_COUNT {
        // Each page takes the entries placed first, then those placed after its own.
        let mut pages = BTreeMap::<(u16, bool), Vec<(usize, usize)>>::new();
        for placement in placements {
            for page in placement
                .pages
                .iter()
                .filter(|&page| usize::from(page) < STOCK_PRESENTATION_NODE_COUNT)
            {
                let lead = crate::collection::leads(
                    placement.position,
                    usize::from(page),
                    PRESENTATION_NODE_COLLECTIBLES_OFFSET,
                );
                pages.entry((page, lead)).or_default().push((
                    placement.donor_collectible_index,
                    placement.authored_collectible_index,
                ));
            }
        }
        // Every row copies its donor's stock row before any is written, since an importer's
        // entries placed first take stock rows (`collection::front`). Those go in before any
        // placed after the stock entries, so the family reads each page's stock count from its
        // length.
        let groups = pages
            .into_iter()
            .map(|((page, first), members)| {
                let page = usize::from(page);
                let rows = crate::collection::front::donor_rows(&nodes, page, &members)?;
                Ok((page, first, rows))
            })
            .collect::<AuthoringResult<Vec<_>>>()?;
        let (first, after): (Vec<_>, Vec<_>) = groups.into_iter().partition(|(_, first, _)| *first);
        for (page, _, rows) in first {
            crate::collection::front::place_first(&mut nodes, page, &rows)?;
        }
        for (page, _, rows) in after {
            let (_, existing) = collectible_children(&nodes, page)?;
            insert_collectible_children(&mut nodes, page, existing.len(), &rows)?;
        }
    }
    let node_strings = append_strings(
        stock_node_strings,
        localization_table_index,
        badge_icon_index,
        layout,
    )?;
    let mut graph = SunriseBadgeGraph {
        nodes,
        node_strings,
        objectives,
        objective_strings,
        records,
        record_strings,
    };
    membership::set_members(&mut graph, placements, shared_expression_pools, layout)?;
    Ok(graph)
}

/// The badges a member's build adds for `weapons`: its own badge when any of them has a
/// Collections entry, one for each custom badge name those items use, and in a build without mod
/// folders one for each importer whose items join a source badge (`append_source_badges`).
pub(crate) fn count<'a>(
    weapons: impl IntoIterator<Item = &'a crate::item::WeaponCloneSpec>,
) -> usize {
    let collected = weapons
        .into_iter()
        .filter(|weapon| weapon.has_collections())
        .collect::<Vec<_>>();
    if collected.is_empty() {
        return 0;
    }
    let custom = collected
        .iter()
        .filter_map(|weapon| weapon.overrides.badge.as_ref())
        .map(|badge| badge.name.as_str())
        .collect::<BTreeSet<_>>();
    let sources = collected
        .iter()
        .filter_map(|weapon| source_badge(weapon))
        .collect::<BTreeSet<_>>();
    1 + custom.len() + sources.len()
}

/// Extends the stock badge-root objective with an acquired-all term for each badge in `groups`
/// and raises its target by as many. `children` is how many of those badges this build adds to
/// the root itself.
///
/// The stock root reads its persisted account/character badge totals. Project Sunrise has no
/// native persisted objective-value slot, so its contribution is the conjunction of every
/// authored collection unlock: zero until the badge is complete, then one. A badge with no unlocks
/// is always complete and counts as a constant one. Built items are always acquired, so every
/// build counts its badges this way, and the main mod on Sunrise's mod loader counts every family
/// member's badges, its own and the importers'.
pub(crate) fn patch_badge_objectives(
    mut objectives: Vec<u8>,
    nodes: &[u8],
    shared_expression_pools: &[u8],
    groups: &[Vec<u16>],
    children: usize,
) -> AuthoringResult<Vec<u8>> {
    for authored_unlock_indices in groups {
        if authored_unlock_indices
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
            != authored_unlock_indices.len()
        {
            return Err(invalid(
                "Badge-root completion requires distinct authored unlock flags",
            ));
        }
    }

    let (objective_count, _, objective_rows, objective_class) = array_at(&objectives, 8)?;
    let (node_count, _, node_rows, _) = array_at(nodes, 8)?;
    if objective_class != OBJECTIVE_ROW_CLASS
        || usize::from(BADGES_ROOT_OBJECTIVE_INDEX) >= objective_count
        || BADGES_ROOT_NODE_INDEX >= node_count
    {
        return Err(invalid("The stock badge-root objective is unavailable"));
    }
    let root_node = node_rows + BADGES_ROOT_NODE_INDEX * PRESENTATION_NODE_ROW_SIZE;
    if read_u32(nodes, root_node + PRESENTATION_NODE_HASH_OFFSET)? != BADGES_ROOT_NODE_HASH
        || read_u16(nodes, root_node + PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET)?
            != BADGES_ROOT_OBJECTIVE_INDEX
    {
        return Err(invalid("The stock badge-root objective link changed"));
    }
    // On Sunrise's mod loader the main mod's root also keeps the stock rows an importer's badge
    // takes, so only the rows naming badges this build added are counted.
    let (child_count, _, child_rows, child_class) =
        array_at(nodes, root_node + PRESENTATION_NODE_CHILD_NODES_OFFSET)?;
    let authored_children = (0..child_count)
        .map(|position| {
            read_u16(
                nodes,
                child_rows + position * PRESENTATION_NODE_CHILD_NODE_ROW_SIZE,
            )
        })
        .collect::<AuthoringResult<Vec<_>>>()?
        .into_iter()
        .filter(|child| usize::from(*child) >= STOCK_PRESENTATION_NODE_COUNT)
        .count();
    if child_class != PRESENTATION_NODE_CHILD_NODE_ROW_CLASS
        || child_count < STOCK_BADGES_ROOT_COMPLETION_VALUE as usize + children
        || authored_children != children
        || children > groups.len()
    {
        return Err(invalid(
            "The badge root does not hold one child for each badge this build adds",
        ));
    }

    let root_objective =
        objective_rows + usize::from(BADGES_ROOT_OBJECTIVE_INDEX) * OBJECTIVE_ROW_SIZE;
    let completion = root_objective + OBJECTIVE_COMPLETION_VALUE_OFFSET;
    let primary = numeric_program_layout(&objectives, root_objective + 0x08)?;
    let secondary = numeric_program_layout(&objectives, root_objective + 0x38)?;
    if read_i32(&objectives, completion)? != STOCK_BADGES_ROOT_COMPLETION_VALUE
        || primary.tokens != [(NUMERIC_VALUE_INSTRUCTION, BADGES_ROOT_ACCOUNT_VALUE_INDEX)]
        || secondary.tokens != [(NUMERIC_VALUE_INSTRUCTION, BADGES_ROOT_CHARACTER_VALUE_INDEX)]
        || read_u64(&objectives, root_objective + 0x48)? != 0
    {
        return Err(invalid(
            "The stock badge-root target or current-value expression changed",
        ));
    }

    let (pool_rows, _) = validate_shared_expression_table(shared_expression_pools, true)?;
    let flag_template = shared_numeric_instruction_template(
        shared_expression_pools,
        pool_rows,
        NUMERIC_FLAG_INSTRUCTION,
        None,
    )?;
    let and_template = shared_numeric_instruction_template(
        shared_expression_pools,
        pool_rows,
        NUMERIC_AND_INSTRUCTION,
        Some(u16::MAX),
    )?;
    let add_template = shared_numeric_instruction_template(
        shared_expression_pools,
        pool_rows,
        NUMERIC_ADD_INSTRUCTION,
        Some(u16::MAX),
    )?;

    let mut instructions = Vec::new();
    instructions.push(primary.instructions[0].serialized);
    for authored_unlock_indices in groups {
        if authored_unlock_indices.is_empty() {
            let constant =
                shared_numeric_instruction_template(shared_expression_pools, pool_rows, 11, None)?;
            instructions.push(constant.with_semantics(11, 1).serialized);
        }
        for (position, flag) in authored_unlock_indices.iter().copied().enumerate() {
            instructions.push(
                flag_template
                    .with_semantics(NUMERIC_FLAG_INSTRUCTION, flag)
                    .serialized,
            );
            if position != 0 {
                instructions.push(and_template.serialized);
            }
        }
        instructions.push(add_template.serialized);
    }
    let expected_tokens = instructions
        .iter()
        .map(|instruction| {
            (
                instruction[0],
                u16::from_le_bytes([instruction[4], instruction[5]]),
            )
        })
        .collect::<Vec<_>>();
    append_numeric_program(
        &mut objectives,
        root_objective + 0x08,
        &instructions,
        "badge-root current-value expression",
    )?;
    write_i32(
        &mut objectives,
        completion,
        STOCK_BADGES_ROOT_COMPLETION_VALUE + groups.len() as i32,
    )?;

    let final_primary = numeric_program_layout(&objectives, root_objective + 0x08)?;
    let final_secondary = numeric_program_layout(&objectives, root_objective + 0x38)?;
    if final_primary.tokens != expected_tokens
        || numeric_program_stack_depth(&final_primary.tokens)? != 1
        || final_secondary.tokens != secondary.tokens
        || read_i32(&objectives, completion)?
            != STOCK_BADGES_ROOT_COMPLETION_VALUE + groups.len() as i32
    {
        return Err(validation(
            "Badge-root target and current-value expression did not serialize faithfully",
        ));
    }
    Ok(objectives)
}

fn validate_lunar_badge_icon_donor(icons: &[u8], icon_key: u32) -> AuthoringResult<TagHash> {
    let (count, _, rows, class) = array_at(icons, 8)?;
    if class != ITEM_ICON_ROW_CLASS
        || count != STOCK_ITEM_ICON_COUNT
        || LUNAR_BADGE_ICON_ROW_INDEX >= count
        || read_u32(
            icons,
            rows + LUNAR_BADGE_ICON_ROW_INDEX * ITEM_ICON_ROW_SIZE,
        )? != LUNAR_BADGE_ICON_KEY
        || contains_u32_row_key(icons, rows, count, ITEM_ICON_ROW_SIZE, icon_key)?
    {
        return Err(invalid(
            "Sunrise badge icon authoring requires the audited 15,725-row icon table and Lunar donor",
        ));
    }
    let container = TagHash(read_u32(
        icons,
        rows + LUNAR_BADGE_ICON_ROW_INDEX * ITEM_ICON_ROW_SIZE + ITEM_ICON_CONTAINER_OFFSET,
    )?);
    if !is_valid_package_tag(container) {
        return Err(invalid("The stock Lunar badge icon container is invalid"));
    }
    Ok(container)
}

#[derive(Clone, Debug)]
struct PresentationDescriptorClone {
    field: usize,
    segment: Vec<u8>,
}

#[allow(clippy::cognitive_complexity)]
fn append_nodes(
    mut nodes: Vec<u8>,
    authored_collectible_index: usize,
    layout: &Layout,
) -> AuthoringResult<Vec<u8>> {
    let (node_count, main_header, node_rows, node_class) = array_at(&nodes, 8)?;
    let rows_end = node_rows
        .checked_add(
            node_count
                .checked_mul(PRESENTATION_NODE_ROW_SIZE)
                .ok_or_else(|| invalid("Presentation-node fixed-row size overflowed"))?,
        )
        .ok_or_else(|| invalid("Presentation-node fixed-row range overflowed"))?;
    if node_class != PRESENTATION_NODE_DEFINITION_ROW_CLASS
        || node_count != layout.node_start
        || rows_end > nodes.len()
    {
        return Err(invalid(
            "Sunrise badge authoring requires the audited 924-row presentation table",
        ));
    }
    for hash in layout.node_hashes {
        if contains_u32_at_offset(
            &nodes,
            node_rows,
            node_count,
            PRESENTATION_NODE_ROW_SIZE,
            PRESENTATION_NODE_HASH_OFFSET,
            hash,
        )? {
            return Err(invalid(format!(
                "Sunrise presentation hash 0x{hash:08X} already exists"
            )));
        }
    }
    let template_indices = [
        ACE_BADGE_GROUP_NODE_INDEX,
        ACE_BADGE_TITAN_NODE_INDEX,
        ACE_BADGE_HUNTER_NODE_INDEX,
        ACE_BADGE_WARLOCK_NODE_INDEX,
    ];
    let templates = template_indices
        .iter()
        .map(|index| {
            nodes[node_rows + index * PRESENTATION_NODE_ROW_SIZE
                ..node_rows + (index + 1) * PRESENTATION_NODE_ROW_SIZE]
                .to_vec()
        })
        .collect::<Vec<_>>();
    for (template, expected_record) in templates.iter().zip(ACE_BADGE_RECORD_INDICES) {
        if read_u16(template, PRESENTATION_NODE_RECORD_INDEX_OFFSET)? != expected_record {
            return Err(invalid(
                "Ace badge template no longer has its audited backing-record link",
            ));
        }
    }

    let mut top_level_headers = BTreeSet::new();
    for index in 0..node_count {
        let row = node_rows + index * PRESENTATION_NODE_ROW_SIZE;
        for field in PRESENTATION_NODE_POINTER_FIELDS {
            if let Some(header) = crate::tag_payload::nested_header(&nodes, row + field, rows_end)?
                && header >= rows_end
            {
                top_level_headers.insert(header);
            }
        }
    }
    let top_level_headers = top_level_headers.into_iter().collect::<Vec<_>>();
    let mut descriptor_clones = Vec::with_capacity(template_indices.len());
    for template_index in template_indices {
        let row = node_rows + template_index * PRESENTATION_NODE_ROW_SIZE;
        let mut clones = Vec::new();
        for field in PRESENTATION_NODE_POINTER_FIELDS {
            let Some(header) = crate::tag_payload::nested_header(&nodes, row + field, rows_end)?
            else {
                continue;
            };
            if header < rows_end {
                return Err(invalid(
                    "Ace presentation template has a nonempty array inside fixed rows",
                ));
            }
            let end = top_level_headers
                .iter()
                .copied()
                .find(|candidate| *candidate > header)
                .unwrap_or(nodes.len());
            clones.push(PresentationDescriptorClone {
                field,
                segment: nodes
                    .get(header..end)
                    .ok_or_else(|| invalid("Ace presentation array is truncated"))?
                    .to_vec(),
            });
        }
        descriptor_clones.push(clones);
    }

    let fixed_shift = PRESENTATION_NODE_ROW_SIZE * layout.node_hashes.len();
    nodes.splice(rows_end..rows_end, std::iter::repeat_n(0, fixed_shift));
    for index in 0..node_count {
        let row = node_rows + index * PRESENTATION_NODE_ROW_SIZE;
        for field in PRESENTATION_NODE_POINTER_FIELDS {
            let Some(target) = crate::tag_payload::nested_header(&nodes, row + field, rows_end)?
            else {
                continue;
            };
            if target < rows_end {
                return Err(invalid(
                    "Presentation-node nested array unexpectedly targets fixed rows",
                ));
            }
            write_relative_pointer(&mut nodes, row + field + 8, target + fixed_shift)?;
        }
    }
    for (position, template) in templates.iter().enumerate() {
        let row = rows_end + position * PRESENTATION_NODE_ROW_SIZE;
        nodes[row..row + PRESENTATION_NODE_ROW_SIZE].copy_from_slice(template);
        for clone in &descriptor_clones[position] {
            let header = begin_array_block(&mut nodes);
            nodes.extend_from_slice(&clone.segment);
            write_relative_pointer(&mut nodes, row + clone.field + 8, header)?;
        }
        write_u32(
            &mut nodes,
            row + PRESENTATION_NODE_HASH_OFFSET,
            layout.node_hashes[position],
        )?;
        write_u16(
            &mut nodes,
            row + PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET,
            u16::MAX,
        )?;
        write_u16(
            &mut nodes,
            row + PRESENTATION_NODE_RECORD_INDEX_OFFSET,
            u16::try_from(layout.record_start + position)
                .map_err(|_| invalid("Sunrise badge record index does not fit 16 bits"))?,
        )?;
    }
    set_array_count(
        &mut nodes,
        8,
        main_header,
        layout.node_start + layout.node_hashes.len(),
    )?;

    let authored_rows = rows_end;
    let group_row = authored_rows;
    let (group_parent_count, _, group_parents, group_parent_class) =
        array_at(&nodes, group_row + 0x18)?;
    if group_parent_count != 1 || group_parent_class != PRESENTATION_NODE_INDEX_ROW_CLASS {
        return Err(invalid(
            "Ace group template has an incompatible parent array",
        ));
    }
    write_u16(&mut nodes, group_parents, BADGES_ROOT_NODE_INDEX as u16)?;
    let (group_child_count, _, group_children, group_child_class) =
        array_at(&nodes, group_row + PRESENTATION_NODE_CHILD_NODES_OFFSET)?;
    if group_child_count != 3 || group_child_class != PRESENTATION_NODE_CHILD_NODE_ROW_CLASS {
        return Err(invalid(
            "Ace group template has an incompatible class-child array",
        ));
    }
    for (position, flag_operand) in SUNRISE_CLASS_FLAG_OPERANDS.iter().copied().enumerate() {
        let child = group_children + position * PRESENTATION_NODE_CHILD_NODE_ROW_SIZE;
        write_u16(
            &mut nodes,
            child,
            u16::try_from((layout.node_start + 1) + position)
                .map_err(|_| invalid("Sunrise class node index does not fit 16 bits"))?,
        )?;
        let (condition_count, _, tokens, condition_class) = array_at(&nodes, child + 8)?;
        if condition_count != 1
            || condition_class != NUMERIC_PROGRAM_ROW_CLASS
            || read_u8(&nodes, tokens)? != NUMERIC_FLAG_INSTRUCTION
        {
            return Err(invalid(
                "Ace class child has an incompatible flag condition",
            ));
        }
        write_u16(&mut nodes, tokens + 4, flag_operand)?;
    }

    for position in 0..3 {
        let leaf_row = authored_rows + (position + 1) * PRESENTATION_NODE_ROW_SIZE;
        let (parent_count, _, parents, parent_class) = array_at(&nodes, leaf_row + 0x18)?;
        if parent_count != 1 || parent_class != PRESENTATION_NODE_INDEX_ROW_CLASS {
            return Err(invalid("Ace class leaf has an incompatible parent array"));
        }
        write_u16(&mut nodes, parents, layout.node_start as u16)?;
        replace_with_single_collectible_child(
            &mut nodes,
            leaf_row + PRESENTATION_NODE_COLLECTIBLES_OFFSET,
            authored_collectible_index,
        )?;
    }

    // Each badge takes its place in the Badges root once the build has added them all
    // (`arrange_root`).
    Ok(nodes)
}

fn append_strings(
    mut strings: Vec<u8>,
    localization_table_index: u32,
    badge_icon_index: u16,
    layout: &Layout,
) -> AuthoringResult<Vec<u8>> {
    let (count, header, rows, class) = array_at(&strings, 8)?;
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(PRESENTATION_NODE_STRING_ROW_SIZE)
                .ok_or_else(|| invalid("Presentation-string row size overflowed"))?,
        )
        .ok_or_else(|| invalid("Presentation-string row range overflowed"))?;
    if class != PRESENTATION_NODE_STRING_ROW_CLASS
        || count != layout.node_start
        || rows_end != strings.len()
    {
        return Err(invalid(
            "Sunrise badge authoring requires the terminal 924-row presentation-string table",
        ));
    }
    for hash in layout.node_hashes {
        if contains_u32_at_offset(
            &strings,
            rows,
            count,
            PRESENTATION_NODE_STRING_ROW_SIZE,
            0,
            hash,
        )? {
            return Err(invalid(format!(
                "Sunrise presentation-string hash 0x{hash:08X} already exists"
            )));
        }
    }
    for (position, source_index) in [
        ACE_BADGE_GROUP_NODE_INDEX,
        ACE_BADGE_TITAN_NODE_INDEX,
        ACE_BADGE_HUNTER_NODE_INDEX,
        ACE_BADGE_WARLOCK_NODE_INDEX,
    ]
    .into_iter()
    .enumerate()
    {
        let source = rows + source_index * PRESENTATION_NODE_STRING_ROW_SIZE;
        let template = strings[source..source + PRESENTATION_NODE_STRING_ROW_SIZE].to_vec();
        strings.extend_from_slice(&template);
        let authored = rows_end + position * PRESENTATION_NODE_STRING_ROW_SIZE;
        write_u32(&mut strings, authored, layout.node_hashes[position])?;
        if position == 0 {
            write_u16(
                &mut strings,
                authored + PRESENTATION_NODE_STRING_ICON_OFFSET,
                badge_icon_index,
            )?;
            write_localized_reference(
                &mut strings,
                authored + PRESENTATION_NODE_STRING_NAME_REFERENCE_OFFSET,
                localization_table_index,
                layout.name_hash,
            )?;
            write_localized_reference(
                &mut strings,
                authored + PRESENTATION_NODE_STRING_DESCRIPTION_REFERENCE_OFFSET,
                localization_table_index,
                layout.description_hash,
            )?;
        }
    }
    set_array_count(
        &mut strings,
        8,
        header,
        layout.node_start + layout.node_hashes.len(),
    )?;
    Ok(strings)
}

fn append_objective(
    objectives: Vec<u8>,
    objective_strings: Vec<u8>,
    shared_expression_pools: &[u8],
    members: &[crate::progression::Acquisition],
    localization_table_index: u32,
    layout: &Layout,
) -> AuthoringResult<(Vec<u8>, Vec<u8>, u16)> {
    // A member acquired unconditionally counts through a constant 1, so the badge is complete
    // whatever the account holds.
    let flags = members
        .iter()
        .filter_map(|member| member.flag())
        .collect::<Vec<_>>();
    if flags.iter().copied().collect::<BTreeSet<_>>().len() != flags.len() {
        return Err(invalid(
            "Sunrise badge objective requires distinct authored unlock flags",
        ));
    }
    let template_indices = [ACE_BADGE_OBJECTIVE_TEMPLATE_INDEX];
    let template_hashes = [ACE_BADGE_OBJECTIVE_TEMPLATE_HASH];
    let authored_hashes = [layout.objective_hash];
    let mut objectives = append_fixed_rows_without_donor_dependencies(
        objectives,
        layout.objective_start,
        OBJECTIVE_ROW_SIZE,
        OBJECTIVE_ROW_CLASS,
        &OBJECTIVE_POINTER_FIELDS,
        &[0x08],
        &template_indices,
        &template_hashes,
        &authored_hashes,
        0,
        "objective-definition table",
    )?;
    let mut objective_strings = append_fixed_rows_without_donor_dependencies(
        objective_strings,
        layout.objective_start,
        OBJECTIVE_STRING_ROW_SIZE,
        OBJECTIVE_STRING_ROW_CLASS,
        &OBJECTIVE_STRING_POINTER_FIELDS,
        &[],
        &template_indices,
        &template_hashes,
        &authored_hashes,
        0,
        "objective-string table",
    )?;
    let (_, _, objective_rows, _) = array_at(&objectives, 8)?;
    let (_, _, string_rows, _) = array_at(&objective_strings, 8)?;
    let objective_row = objective_rows + layout.objective_start * OBJECTIVE_ROW_SIZE;
    let string_row = string_rows + layout.objective_start * OBJECTIVE_STRING_ROW_SIZE;
    let completion = i32::try_from(members.len())
        .map_err(|_| invalid("Sunrise badge objective target does not fit i32"))?;
    write_i32(
        &mut objectives,
        objective_row + OBJECTIVE_COMPLETION_VALUE_OFFSET,
        completion,
    )?;
    write_localized_reference(
        &mut objective_strings,
        string_row + OBJECTIVE_STRING_PROGRESS_REFERENCE_OFFSET,
        localization_table_index,
        layout.name_hash,
    )?;

    let (pool_rows, _) = validate_shared_expression_table(shared_expression_pools, true)?;
    let flag_template = shared_numeric_instruction_template(
        shared_expression_pools,
        pool_rows,
        NUMERIC_FLAG_INSTRUCTION,
        None,
    )?;
    let add_template = shared_numeric_instruction_template(
        shared_expression_pools,
        pool_rows,
        NUMERIC_ADD_INSTRUCTION,
        Some(u16::MAX),
    )?;
    let mut instructions = Vec::with_capacity(members.len().saturating_mul(2));
    for (position, member) in members.iter().enumerate() {
        let (opcode, operand) = member.token();
        instructions.push(flag_template.with_semantics(opcode, operand).serialized);
        if position != 0 {
            instructions.push(add_template.serialized);
        }
    }
    if instructions.is_empty() {
        let constant =
            shared_numeric_instruction_template(shared_expression_pools, pool_rows, 11, None)?;
        instructions.push(constant.with_semantics(11, 0).serialized);
    }
    append_numeric_program(
        &mut objectives,
        objective_row + 0x08,
        &instructions,
        "Sunrise badge objective expression",
    )?;
    let objective_index = u16::try_from(layout.objective_start)
        .map_err(|_| invalid("Sunrise badge objective index does not fit 16 bits"))?;
    Ok((objectives, objective_strings, objective_index))
}

#[cfg(test)]
fn append_sunrise_badge_records(
    records: Vec<u8>,
    record_strings: Vec<u8>,
    objective_index: u16,
) -> AuthoringResult<(Vec<u8>, Vec<u8>)> {
    append_records(records, record_strings, objective_index, &Layout::sunrise())
}

fn append_records(
    records: Vec<u8>,
    record_strings: Vec<u8>,
    objective_index: u16,
    layout: &Layout,
) -> AuthoringResult<(Vec<u8>, Vec<u8>)> {
    let template_indices = ACE_BADGE_RECORD_INDICES
        .iter()
        .copied()
        .map(usize::from)
        .collect::<Vec<_>>();
    let mut records = append_fixed_rows_without_donor_dependencies(
        records,
        layout.record_start,
        RECORD_ROW_SIZE,
        RECORD_ROW_CLASS,
        &RECORD_POINTER_FIELDS,
        &[RECORD_OBJECTIVE_DESCRIPTOR_OFFSET],
        &template_indices,
        &ACE_BADGE_RECORD_HASHES,
        &layout.record_hashes,
        RECORD_HASH_OFFSET,
        "record-definition table",
    )?;
    let record_strings = append_fixed_rows_without_donor_dependencies(
        record_strings,
        layout.record_start,
        RECORD_STRING_ROW_SIZE,
        RECORD_STRING_ROW_CLASS,
        &RECORD_STRING_POINTER_FIELDS,
        &[],
        &template_indices,
        &ACE_BADGE_RECORD_HASHES,
        &layout.record_hashes,
        0,
        "record-string table",
    )?;
    let (_, _, record_rows, _) = array_at(&records, 8)?;
    for position in 0..layout.record_hashes.len() {
        let row = record_rows + (layout.record_start + position) * RECORD_ROW_SIZE;
        append_single_u16_array(
            &mut records,
            row + RECORD_OBJECTIVE_DESCRIPTOR_OFFSET,
            RECORD_OBJECTIVE_ROW_CLASS,
            objective_index,
        )?;
    }
    validate_records(&records, &record_strings, objective_index, layout)?;
    Ok((records, record_strings))
}

#[cfg(test)]
fn validate_sunrise_badge_records(
    records: &[u8],
    record_strings: &[u8],
    objective_index: u16,
) -> AuthoringResult<()> {
    validate_records(records, record_strings, objective_index, &Layout::sunrise())
}

fn validate_records(
    records: &[u8],
    record_strings: &[u8],
    objective_index: u16,
    layout: &Layout,
) -> AuthoringResult<()> {
    let (record_count, _, record_rows, record_class) = array_at(records, 8)?;
    let (string_count, _, string_rows, string_class) = array_at(record_strings, 8)?;
    let expected_count = layout.record_start + layout.record_hashes.len();
    if record_count != expected_count
        || string_count != expected_count
        || record_class != RECORD_ROW_CLASS
        || string_class != RECORD_STRING_ROW_CLASS
    {
        return Err(validation(
            "Sunrise record definitions and strings are not aligned at 2242 -> 2246",
        ));
    }
    for (position, expected_hash) in layout.record_hashes.iter().copied().enumerate() {
        let target_index = layout.record_start + position;
        let target = record_rows + target_index * RECORD_ROW_SIZE;
        let string = string_rows + target_index * RECORD_STRING_ROW_SIZE;
        if read_u32(records, target + RECORD_HASH_OFFSET)? != expected_hash
            || read_u32(record_strings, string)? != expected_hash
        {
            return Err(validation(
                "Sunrise record definition/string identity pairing is inconsistent",
            ));
        }
        let (objective_count, objective_header, objective_rows, objective_class) =
            array_at(records, target + RECORD_OBJECTIVE_DESCRIPTOR_OFFSET)?;
        if objective_header < 4 || read_u32(records, objective_header - 4)? >> 16 != 0x8080 {
            return Err(validation(
                "Sunrise badge record objective array lacks its native header marker",
            ));
        }
        if objective_count != 1
            || objective_class != RECORD_OBJECTIVE_ROW_CLASS
            || read_u16(records, objective_rows)? != objective_index
        {
            return Err(validation(
                "Sunrise badge record does not reference its authored objective",
            ));
        }
        for &field in &RECORD_POINTER_FIELDS {
            if field != RECORD_OBJECTIVE_DESCRIPTOR_OFFSET
                && read_u64(records, target + field)? != 0
            {
                return Err(validation(
                    "Sunrise badge record retained a donor dependency",
                ));
            }
        }
        for &field in &RECORD_STRING_POINTER_FIELDS {
            if read_u64(record_strings, string + field)? != 0 {
                return Err(validation(
                    "Sunrise record-string clone unexpectedly acquired a nested dependency",
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presentation_child_array_terminator_handles_every_alignment_class() {
        for child_count in [3, 4, 5, 6, 15, 31, 55, 71] {
            let child_end = 16 + child_count * PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE;
            let mut segment = vec![0xA5; child_end];
            append_presentation_child_array_terminator(&mut segment).unwrap();
            let expected_end = (child_end + size_of::<u32>() + 15) & !15;
            let sentinel_start = expected_end - size_of::<u32>();
            assert_eq!(segment.len(), expected_end, "child count {child_count}");
            assert!(
                segment[child_end..sentinel_start]
                    .iter()
                    .all(|byte| *byte == 0)
            );
            assert_eq!(
                &segment[sentinel_start..expected_end],
                &PRESENTATION_CHILD_ARRAY_SENTINEL.to_le_bytes()
            );
            assert_eq!(
                validate_presentation_child_array_terminator(&segment, child_end).unwrap(),
                expected_end
            );
        }
    }

    #[test]
    fn rejects_the_legacy_fixed_eight_byte_child_trailer() {
        let child_count = 7;
        let child_end = 16 + child_count * PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE;
        let legacy_end = (child_end + NESTED_ARRAY_TRAILER.len() + 15) & !15;
        let mut segment = vec![0xA5; child_end];
        segment.resize(legacy_end - NESTED_ARRAY_TRAILER.len(), 0);
        segment.extend_from_slice(&NESTED_ARRAY_TRAILER);
        assert!(validate_presentation_child_array_terminator(&segment, child_end).is_err());
    }

    fn synthetic_badge_record_tables() -> (Vec<u8>, Vec<u8>) {
        fn fixed_table(row_size: usize, row_class: u32) -> (Vec<u8>, usize) {
            let header = 0x20;
            let rows = header + 16;
            let mut data = vec![0; rows + STOCK_RECORD_COUNT * row_size];
            write_u64(&mut data, 8, STOCK_RECORD_COUNT as u64).unwrap();
            write_relative_pointer(&mut data, 16, header).unwrap();
            write_u64(&mut data, header, STOCK_RECORD_COUNT as u64).unwrap();
            write_u32(&mut data, header + 8, row_class).unwrap();
            (data, rows)
        }

        let (mut records, record_rows) = fixed_table(RECORD_ROW_SIZE, RECORD_ROW_CLASS);
        let (mut strings, string_rows) =
            fixed_table(RECORD_STRING_ROW_SIZE, RECORD_STRING_ROW_CLASS);
        for (position, source_index) in ACE_BADGE_RECORD_INDICES
            .iter()
            .copied()
            .map(usize::from)
            .enumerate()
        {
            let record = record_rows + source_index * RECORD_ROW_SIZE;
            let string = string_rows + source_index * RECORD_STRING_ROW_SIZE;
            write_u32(
                &mut records,
                record + RECORD_HASH_OFFSET,
                ACE_BADGE_RECORD_HASHES[position],
            )
            .unwrap();
            write_u32(&mut strings, string, ACE_BADGE_RECORD_HASHES[position]).unwrap();
            append_single_u16_array(
                &mut records,
                record + RECORD_OBJECTIVE_DESCRIPTOR_OFFSET,
                RECORD_OBJECTIVE_ROW_CLASS,
                6_286 + position as u16,
            )
            .unwrap();
        }
        (records, strings)
    }

    #[test]
    fn objective_array_marker_survives_every_preceding_alignment() {
        for length in 32..64 {
            let mut data = vec![0xA5; length];
            append_single_u16_array(&mut data, 0, RECORD_OBJECTIVE_ROW_CLASS, 8483).unwrap();
            let (count, header, rows, class) = array_at(&data, 0).unwrap();
            assert_eq!(header % 16, 0);
            assert_eq!(
                read_u32(&data, header - 4).unwrap(),
                PRESENTATION_CHILD_ARRAY_SENTINEL
            );
            assert_eq!((count, class), (1, RECORD_OBJECTIVE_ROW_CLASS));
            assert_eq!(read_u16(&data, rows).unwrap(), 8483);
            assert!(data[16..length].iter().all(|byte| *byte == 0xA5));
        }
    }

    #[test]
    fn rejects_missing_authored_record_objective_marker() {
        let (records, strings) = synthetic_badge_record_tables();
        let objective_index = STOCK_OBJECTIVE_COUNT as u16;
        let (mut records, strings) =
            append_sunrise_badge_records(records, strings, objective_index).unwrap();
        let (_, _, rows, _) = array_at(&records, 8).unwrap();
        let (_, header, _, _) = array_at(
            &records,
            rows + STOCK_RECORD_COUNT * RECORD_ROW_SIZE + RECORD_OBJECTIVE_DESCRIPTOR_OFFSET,
        )
        .unwrap();
        write_u32(&mut records, header - 4, 0).unwrap();
        assert!(validate_sunrise_badge_records(&records, &strings, objective_index).is_err());
    }
}

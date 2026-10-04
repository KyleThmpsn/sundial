//! Class support from Shadowkeep armor's class-and-slot equip flags.
use super::*;
use crate::collection::Classes;
use crate::progression::{
    PRESENTATION_NODE_CHILD_NODE_ROW_CLASS, PRESENTATION_NODE_CHILD_NODE_ROW_SIZE,
    PRESENTATION_NODE_CHILD_NODES_OFFSET, PRESENTATION_NODE_COLLECTIBLES_OFFSET,
    PRESENTATION_NODE_HASH_OFFSET, PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET,
    PRESENTATION_NODE_ROW_SIZE, presentation_ancestor_nodes,
};

pub(super) fn classes(
    definition: &[u8],
    selected: Option<crate::ArmorClass>,
) -> AuthoringResult<Classes> {
    let inherited =
        sundial::package_authoring::investment_schema::armor_equipment_class(definition)
            .map_err(invalid)?;
    let class = selected.map_or(inherited, crate::ArmorClass::native_class);
    Ok(class.map_or(Classes::ALL, Classes::one))
}

pub(super) fn apply_class(
    definition: &mut Vec<u8>,
    selected: crate::ArmorClass,
) -> AuthoringResult<()> {
    *definition = sundial::package_authoring::investment_schema::set_armor_equipment_class(
        definition,
        selected.native_class(),
    )
    .map_err(invalid)?;
    Ok(())
}

/// Clone a native category for the runtime node and a native armor set for its rows.
pub(super) fn page(
    sources: &sources::ProjectSources,
    page: crate::collection::GearPage,
    index: u16,
) -> AuthoringResult<placements::GearPagePlan> {
    let class = page
        .class()
        .ok_or_else(|| invalid("Armor page has no class"))?;
    let (count, _, rows, _) = array_at(&sources.stock_nodes, 8)?;
    let node = |hash| {
        (0..count)
            .find(|&i| {
                read_u32(
                    &sources.stock_nodes,
                    rows + i * PRESENTATION_NODE_ROW_SIZE + PRESENTATION_NODE_HASH_OFFSET,
                )
                .ok()
                    == Some(hash)
            })
            .ok_or_else(|| invalid("The native Armor Collections branch is unavailable"))
    };
    let armor = node(0x5FAB_0042)?;
    let parent = node([0x305A_5226, 0xDF3B_D502, 0x4BB1_6895][usize::from(class)])?;
    let ancestors = presentation_ancestor_nodes(&sources.stock_nodes, &[parent as u16])?;
    if !ancestors.contains(&armor) {
        return Err(invalid(
            "The native armor class is outside Collections Armor",
        ));
    }
    let (siblings, _, children, child_class) = array_at(
        &sources.stock_nodes,
        rows + parent * PRESENTATION_NODE_ROW_SIZE + PRESENTATION_NODE_CHILD_NODES_OFFSET,
    )?;
    if siblings == 0 || child_class != PRESENTATION_NODE_CHILD_NODE_ROW_CLASS {
        return Err(invalid(
            "The armor class has no compatible Collections categories",
        ));
    }
    let sibling = read_u16(
        &sources.stock_nodes,
        children + (siblings - 1) * PRESENTATION_NODE_CHILD_NODE_ROW_SIZE,
    )?;
    for template in 0..count {
        let row = rows + template * PRESENTATION_NODE_ROW_SIZE;
        if read_u16(
            &sources.stock_nodes,
            row + PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET,
        )? == u16::MAX
            || read_u64(
                &sources.stock_nodes,
                row + PRESENTATION_NODE_CHILD_NODES_OFFSET,
            )? != 0
            || read_u64(
                &sources.stock_nodes,
                row + PRESENTATION_NODE_COLLECTIBLES_OFFSET,
            )? == 0
        {
            continue;
        }
        if !presentation_ancestor_nodes(&sources.stock_nodes, &[template as u16])?.contains(&parent)
        {
            continue;
        }
        let members =
            crate::progression::node_collectible_children(&sources.stock_nodes, template as u16)?;
        let (parent_count, _, parent_rows, _) = array_at(&sources.stock_nodes, row + 0x18)?;
        if parent_count != 1 {
            return Err(invalid("Armor set template must have one category parent"));
        }
        let category = read_u16(&sources.stock_nodes, parent_rows)?;
        let category_row = rows + usize::from(category) * PRESENTATION_NODE_ROW_SIZE;
        let (parents, _, category_parents, _) =
            array_at(&sources.stock_nodes, category_row + 0x18)?;
        if parents != 1
            || usize::from(read_u16(&sources.stock_nodes, category_parents)?) != parent
            || read_u32(&sources.stock_nodes, category_row + 0x2C)? != 0
            || read_u32(&sources.stock_nodes, row + 0x2C)? != 1
        {
            continue;
        }
        if members.len() > crate::collection::ARMOR_SET_SIZE {
            continue;
        }
        return Ok(placements::GearPagePlan {
            page,
            index,
            template: if page.set().is_some() {
                template as u16
            } else {
                category
            },
            parent: parent as u16,
            sibling,
            donor: members[0],
            children: Vec::new(),
        });
    }
    Err(invalid(
        "The armor class has no counted Collections page template",
    ))
}

/// Exotic armor is a flat class page below Exotic / Armor, without ordinary armor sets.
pub(super) fn exotic_pages(
    sources: &sources::ProjectSources,
    classes: Classes,
) -> AuthoringResult<crate::collection::CollectionPages> {
    let (count, _, rows, _) = array_at(&sources.stock_nodes, 8)?;
    let node = |hash| {
        (0..count)
            .find(|&index| {
                read_u32(
                    &sources.stock_nodes,
                    rows + index * PRESENTATION_NODE_ROW_SIZE + PRESENTATION_NODE_HASH_OFFSET,
                )
                .ok()
                    == Some(hash)
            })
            .ok_or_else(|| invalid("Native Exotic Armor Collections branch is unavailable"))
    };
    let armor = node(0x6AA5_1A40)?;
    let exotic = node(0x3FB0_E331)?;
    if !presentation_ancestor_nodes(&sources.stock_nodes, &[armor as u16])?.contains(&exotic) {
        return Err(invalid("Native Exotic Armor ancestry is incompatible"));
    }
    let mut pages = Vec::new();
    for class in classes.iter() {
        let page = node([0x9AE4_A516, 0xA4DA_5372, 0x5DC5_FD5F][usize::from(class)])?;
        let (parents, _, parent_rows, _) = array_at(
            &sources.stock_nodes,
            rows + page * PRESENTATION_NODE_ROW_SIZE + 0x18,
        )?;
        if parents != 1 || usize::from(read_u16(&sources.stock_nodes, parent_rows)?) != armor {
            return Err(invalid("Exotic Armor class has incompatible ancestry"));
        }
        pages.push(page as u16);
    }
    crate::collection::CollectionPages::new(pages)
}

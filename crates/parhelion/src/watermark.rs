//! Shared authoring plan for Parhelion's Project Sunrise weapon-icon watermark.
//!
//! Shadowkeep item icon rows select a complete icon container. The container keeps the donor's
//! primary weapon art and rarity background in separate fields, while offset `0x20` selects the
//! season/expansion overlay layer. This module authors that layer once and then clones only the
//! requested donor containers, selecting the authored rarity background and Sunrise watermark.

mod custom;
mod placement;
pub(crate) use custom::build_presented_watermark_plan;
pub(crate) use custom::preview as render_custom_corner_preview;
pub(crate) use custom::render as render_custom_corner;

use image::ImageFormat;
use sha1::{Digest, Sha1};
use sundial::package_authoring::icon_schema::ICON_BACKGROUND_LAYER_OFFSET as ICON_RARITY_BACKGROUND_LAYER_OFFSET;
use sundial::package_authoring::{
    icon_schema::{
        ICON_DEFINITION_CLASS as ICON_CONTAINER_CLASS_HANDLE,
        ICON_DEFINITION_SIZE as ICON_CONTAINER_SIZE, ICON_LAYER_ARRAY_CLASS as LAYER_ARRAY_CLASS,
        ICON_LAYER_CLASS as WATERMARK_CLASS_HANDLE, ICON_LAYER_LANE_CLASS as LAYER_LANE_CLASS,
        ICON_LAYER_REFERENCE_OFFSETS, ICON_LAYER_TEXTURE_CLASS as LAYER_TEXTURE_CLASS,
        ICON_PRIMARY_LAYER_OFFSET, ICON_WATERMARK_LAYER_OFFSET,
        MAX_ICON_LAYER_LANES as MAX_LAYER_LANES,
        MAX_ICON_TEXTURES_PER_LANE as MAX_TEXTURES_PER_LANE,
    },
    investment_schema::{ITEM_ICON_CONTAINER_OFFSET, ITEM_ICON_ROW_SIZE},
    is_valid_package_tag,
};
use tiger_pkg::{PackageManager, TagHash};

use crate::{
    AuthoredWeaponRarity, AuthoringResult, NewTagReference, NewTagReferenceOverride, NewTagSpec,
    NewTagStorageMode, WeaponIconEdit,
    appended_tags::AppendedTagAllocator,
    error::{input as invalid, validation},
    icon_edit::build_weapon_icon_edit_plan,
    payload_guards::{
        STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE, donor_mutation_is_limited_to,
        is_stock_straight_rgba8_texture_header,
    },
    shared_tag_memory::{
        IconDefinitionCompanion, SharedTagDependencies, build_shared_tag_companion_payload,
        read_and_validate_icon_companion,
    },
    tag_payload::{
        bounded_relative_target as relative_target, read_i64, read_u32, read_u64, write_u32,
    },
};

const AUTHORED_TEXTURE_PNGS: [&[u8]; 6] = [
    include_bytes!("../../../assets/parhelion/watermark/sunrise-watermark-0-96x96.png"),
    include_bytes!("../../../assets/parhelion/watermark/sunrise-watermark-1-54x54.png"),
    include_bytes!("../../../assets/parhelion/watermark/sunrise-watermark-2-45x45.png"),
    include_bytes!("../../../assets/parhelion/watermark/sunrise-watermark-3-45x45.png"),
    include_bytes!("../../../assets/parhelion/watermark/sunrise-watermark-4-96x96.png"),
    include_bytes!("../../../assets/parhelion/watermark/sunrise-watermark-5-54x54.png"),
];
const AUTHORED_TEXTURE_PNG_SHA1: [[u8; 20]; 6] = [
    [
        0x6B, 0x64, 0x3D, 0xD6, 0xDE, 0xF4, 0xE9, 0x20, 0xAE, 0x98, 0xB2, 0x98, 0x46, 0x45, 0x8B,
        0xE8, 0x53, 0xC6, 0xCC, 0x31,
    ],
    [
        0xF1, 0x79, 0xA4, 0xEC, 0xCF, 0x41, 0xA4, 0x0F, 0x84, 0xFC, 0xE1, 0x57, 0x97, 0x34, 0xC4,
        0x3F, 0xA6, 0x6A, 0xDA, 0xFA,
    ],
    [
        0x90, 0x63, 0xFF, 0x18, 0x66, 0x4F, 0x2F, 0xF0, 0x8F, 0x9A, 0xF0, 0xBF, 0x32, 0x58, 0x1C,
        0xEA, 0x53, 0x23, 0xD4, 0xC0,
    ],
    [
        0x9E, 0x62, 0xDC, 0x3D, 0x69, 0x55, 0x7D, 0x66, 0x6C, 0x23, 0xE8, 0x59, 0x34, 0xD4, 0xF2,
        0xFF, 0xC0, 0x1A, 0x93, 0x05,
    ],
    [
        0x6D, 0x50, 0x8D, 0x4E, 0xB2, 0x79, 0x3F, 0x79, 0x8A, 0x86, 0xF8, 0x39, 0xD3, 0x4C, 0x89,
        0xF1, 0x95, 0x21, 0xCF, 0xA4,
    ],
    [
        0x29, 0xAC, 0x69, 0xBD, 0x4F, 0xCE, 0x33, 0xC4, 0x2D, 0xDD, 0xD5, 0x25, 0xDC, 0xD4, 0x4C,
        0x99, 0x5C, 0x39, 0xED, 0x06,
    ],
];
const DONOR_STANDALONE_ALPHA_SHA1: [u8; 20] = [
    0xE7, 0xF9, 0x74, 0xBA, 0xF3, 0x5C, 0x09, 0x32, 0xAC, 0xE3, 0xFD, 0x05, 0x6C, 0xF4, 0x71, 0x6F,
    0xE2, 0x26, 0x31, 0x98,
];
const AUTHORED_STANDALONE_ALPHA_SHA1: [u8; 20] = [
    0x23, 0xE7, 0x52, 0x95, 0x3B, 0xFE, 0xC3, 0xB3, 0x97, 0xA5, 0x3D, 0x19, 0x6E, 0xE5, 0x51, 0xA2,
    0x42, 0x4D, 0x4F, 0xAF,
];
const AUTHORED_STANDALONE_ALPHA_BOUNDS: (u32, u32, u32, u32) = (7, 4, 37, 41);

// These tags provide the audited serialized layer shape only. Each authored weapon still clones
// the complete icon container selected by its appearance donor.
const DONOR_TEXTURE_DATA: [TagHash; 6] = [
    TagHash(0x8131_85C5),
    TagHash(0x8131_85C8),
    TagHash(0x8131_85CA),
    TagHash(0x8131_85CB),
    TagHash(0x81A2_7512),
    TagHash(0x81A2_7515),
];
const DONOR_TEXTURE_HEADERS: [TagHash; 6] = [
    TagHash(0x8131_85C6),
    TagHash(0x8131_85C7),
    TagHash(0x8131_85C9),
    TagHash(0x8131_85CC),
    TagHash(0x81A2_7513),
    TagHash(0x81A2_7514),
];
const DONOR_WATERMARK_LAYER: TagHash = TagHash(0x8131_85CD);

const TEXTURE_DIMENSIONS: [(u32, u32); 6] =
    [(96, 96), (54, 54), (45, 45), (45, 45), (96, 96), (54, 54)];
// Preserve the approved small-scale artwork and the layer's logical layout. Only the
// private texture surfaces gain resolution; stock donors and layer geometry stay intact.
const OUTPUT_TEXTURE_SCALE: u32 = 4;
const WATERMARK_LAYER_SIZE: usize = 152;
const ITEM_ICON_IDENTITY_OFFSET: usize = 0x00;
// A private resource must not retain the donor container's content fingerprint.
// Otherwise native image reuse can select the donor's previously rendered composition.
const ICON_CONTENT_FINGERPRINT_OFFSET: usize = 0x10;
const WATERMARK_TEXTURE_REFERENCE_START: usize = 0x80;
const WATERMARK_TEXTURE_REFERENCE_COUNT: usize = 6;
const TAGS_PER_TEXTURE: usize = 2;
const SHARED_TAG_COUNT: usize = TEXTURE_DIMENSIONS.len() * TAGS_PER_TEXTURE + 1;

/// The absolute appended ordinals and dynamically assigned tags for one texture pair.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WatermarkTexturePair {
    pub width: u32,
    pub height: u32,
    pub data_ordinal: usize,
    pub header_ordinal: usize,
    pub data_tag: TagHash,
    pub header_tag: TagHash,
}

/// One authored weapon's appearance donor and optional primary-image treatment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponIconRequest {
    pub donor_container_tag: TagHash,
    pub icon_edit: WeaponIconEdit,
    pub rarity: AuthoredWeaponRarity,
}

/// One cloned base-art container that points at the shared authored watermark layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatermarkedIconContainer {
    pub donor_container_tag: TagHash,
    pub donor_companion_tag: TagHash,
    pub icon_edit: WeaponIconEdit,
    pub rarity: AuthoredWeaponRarity,
    pub authored_primary_layer_tag: Option<TagHash>,
    pub ordinal: usize,
    pub tag: TagHash,
    pub companion_ordinal: usize,
    pub companion_tag: TagHash,
}

/// An append plan containing one shared watermark resource and one container per unique donor art.
///
/// `new_tags` is ordered as six data/header pairs, the shared watermark layer, then adjacent
/// icon-definition/companion pairs in first-seen donor order. Reference overrides use absolute
/// ordinals in the caller's complete appended-tag slice.
#[derive(Clone, Debug)]
pub struct WatermarkPlan {
    pub new_tags: Vec<NewTagSpec>,
    pub reference_overrides: Vec<NewTagReferenceOverride>,
    #[cfg(test)]
    pub texture_pairs: [WatermarkTexturePair; 6],
    #[cfg(test)]
    pub watermark_layer_ordinal: usize,
    pub watermark_layer_tag: TagHash,
    #[cfg(test)]
    pub icon_containers: Vec<WatermarkedIconContainer>,
    pub icon_container_tags: Vec<TagHash>,
    /// Authored container tags aligned one-to-one with the input requests.
    pub request_container_tags: Vec<TagHash>,
}

impl WatermarkPlan {
    /// Returns the authored container that preserves `donor_container_tag`'s base art.
    #[cfg(test)]
    pub fn container_for_donor(&self, donor_container_tag: TagHash) -> Option<TagHash> {
        self.icon_containers
            .iter()
            .find(|container| container.donor_container_tag == donor_container_tag)
            .map(|container| container.tag)
    }

    /// Returns the authored container for the input request at `request_index`.
    pub fn container_for_request(&self, request_index: usize) -> Option<TagHash> {
        self.request_container_tags.get(request_index).copied()
    }
}

/// Authors one reusable Sunrise watermark resource and watermarked clones of the requested base
/// icon containers. Its six pre-rendered textures preserve the stock lane layouts and native
/// presentation polarities while replacing each logo-bearing variant with the Sunrise mark.
///
/// `current_entry_count` is the destination package's entry count before the append operation.
/// `appended_ordinal_base` is the number of tags placed before this plan in the same `new_tags`
/// slice. Containers are deduplicated only when donor, image edit, and authored rarity are identical.
#[cfg(test)]
pub fn build_watermark_plan(
    manager: &PackageManager,
    destination_package_id: u16,
    current_entry_count: usize,
    appended_ordinal_base: usize,
    icon_requests: &[WeaponIconRequest],
) -> AuthoringResult<WatermarkPlan> {
    build_watermark_plan_with_context(
        manager,
        destination_package_id,
        current_entry_count,
        appended_ordinal_base,
        icon_requests,
        &|index| format!("Icon Donor: {}", icon_requests[index].donor_container_tag),
    )
}

fn build_watermark_plan_with_context(
    manager: &PackageManager,
    destination_package_id: u16,
    current_entry_count: usize,
    appended_ordinal_base: usize,
    icon_requests: &[WeaponIconRequest],
    request_context: &dyn Fn(usize) -> String,
) -> AuthoringResult<WatermarkPlan> {
    if icon_requests.is_empty() {
        return Err(invalid(
            "A Sunrise weapon watermark plan needs at least one donor icon container",
        ));
    }
    let donor_layer = read_and_validate_watermark_donor(manager)?;

    let mut new_tags = Vec::with_capacity(SHARED_TAG_COUNT + icon_requests.len() * 5);
    let mut reference_overrides = Vec::with_capacity(TEXTURE_DIMENSIONS.len() * 2);
    let mut pairs = Vec::with_capacity(TEXTURE_DIMENSIONS.len());
    for (texture_index, (width, height)) in TEXTURE_DIMENSIONS.into_iter().enumerate() {
        let data_ordinal = AppendedTagAllocator::checked_ordinal(
            appended_ordinal_base,
            texture_index * TAGS_PER_TEXTURE,
            "watermark texture data",
        )?;
        let header_ordinal = AppendedTagAllocator::checked_ordinal(
            appended_ordinal_base,
            texture_index * TAGS_PER_TEXTURE + 1,
            "watermark texture header",
        )?;
        let data_tag = assigned_tag(destination_package_id, current_entry_count, data_ordinal)?;
        let header_tag = assigned_tag(destination_package_id, current_entry_count, header_ordinal)?;
        let (mut header, donor_pixels) = read_and_validate_texture_header(
            manager,
            DONOR_TEXTURE_HEADERS[texture_index],
            DONOR_TEXTURE_DATA[texture_index],
            width,
            height,
        )?;
        let pixels = decode_authored_texture(texture_index, width, height)?;
        validate_authored_texture(texture_index, width, height, &donor_pixels, &pixels)?;
        let output = render_output_texture(texture_index)?;
        let (width, height) = output.dimensions();
        let pixels = output.into_raw();
        resize_texture_header(&mut header, width, height, pixels.len())?;
        new_tags.push(NewTagSpec {
            template_tag: DONOR_TEXTURE_DATA[texture_index],
            payload: pixels,
            storage: NewTagStorageMode::InheritTemplate,
        });
        new_tags.push(NewTagSpec {
            template_tag: DONOR_TEXTURE_HEADERS[texture_index],
            payload: header,
            storage: NewTagStorageMode::InheritTemplate,
        });
        reference_overrides.push(NewTagReferenceOverride {
            new_tag_ordinal: data_ordinal,
            reference: NewTagReference::Appended(header_ordinal),
        });
        reference_overrides.push(NewTagReferenceOverride {
            new_tag_ordinal: header_ordinal,
            reference: NewTagReference::Appended(data_ordinal),
        });
        pairs.push(WatermarkTexturePair {
            width,
            height,
            data_ordinal,
            header_ordinal,
            data_tag,
            header_tag,
        });
    }
    let texture_pairs: [WatermarkTexturePair; 6] = pairs
        .try_into()
        .map_err(|_| validation("Watermark texture-pair count did not converge"))?;

    let watermark_layer_ordinal = AppendedTagAllocator::checked_ordinal(
        appended_ordinal_base,
        TEXTURE_DIMENSIONS.len() * TAGS_PER_TEXTURE,
        "watermark layer",
    )?;
    let watermark_layer_tag = assigned_tag(
        destination_package_id,
        current_entry_count,
        watermark_layer_ordinal,
    )?;
    let mut watermark_layer = donor_layer.clone();
    for (index, pair) in texture_pairs.iter().enumerate() {
        write_tag(
            &mut watermark_layer,
            WATERMARK_TEXTURE_REFERENCE_START + index * 4,
            pair.header_tag,
        )?;
    }
    validate_only_patched_fields(
        &donor_layer,
        &watermark_layer,
        &(0..WATERMARK_TEXTURE_REFERENCE_COUNT)
            .map(|index| WATERMARK_TEXTURE_REFERENCE_START + index * 4)
            .collect::<Vec<_>>(),
        "watermark layer",
    )?;
    new_tags.push(NewTagSpec {
        template_tag: DONOR_WATERMARK_LAYER,
        payload: watermark_layer,
        storage: NewTagStorageMode::InheritTemplate,
    });

    let mut edit_graphs = Vec::new();
    for (request_index, request) in icon_requests.iter().cloned().enumerate() {
        request
            .icon_edit
            .validate()
            .map_err(|error| error.context(request_context(request_index)))?;
        if edit_graphs
            .iter()
            .any(|(existing, _): &(WeaponIconRequest, _)| {
                existing.donor_container_tag == request.donor_container_tag
                    && existing.icon_edit == request.icon_edit
            })
        {
            continue;
        }
        (|| -> AuthoringResult<()> {
            let edit_ordinal_base = AppendedTagAllocator::checked_ordinal(
                appended_ordinal_base,
                new_tags.len(),
                "edited primary-image graph",
            )?;
            let graph = build_weapon_icon_edit_plan(
                manager,
                destination_package_id,
                current_entry_count,
                edit_ordinal_base,
                request.donor_container_tag,
                &request.icon_edit,
            )?;
            let resolved = graph.map(|graph| {
                let primary_layer_tag = graph.primary_layer_tag;
                let dependencies = graph.dependencies;
                reference_overrides.extend(graph.reference_overrides);
                new_tags.extend(graph.new_tags);
                (primary_layer_tag, dependencies)
            });
            edit_graphs.push((request, resolved));
            Ok(())
        })()
        .map_err(|error| error.context(request_context(request_index)))?;
    }

    let mut visual_revision = Sha1::new();
    for resource in &new_tags {
        visual_revision.update(&resource.payload);
    }
    let visual_revision = visual_revision.finalize();
    let mut icon_containers: Vec<WatermarkedIconContainer> = Vec::new();
    let mut request_container_tags = Vec::with_capacity(icon_requests.len());
    for (request_index, request) in icon_requests.iter().cloned().enumerate() {
        if let Some(existing) = icon_containers.iter().find(|container| {
            container.donor_container_tag == request.donor_container_tag
                && container.icon_edit == request.icon_edit
                && container.rarity == request.rarity
        }) {
            request_container_tags.push(existing.tag);
            continue;
        }
        (|| -> AuthoringResult<()> {
            let donor_container_tag = request.donor_container_tag;
            let edit_graph = edit_graphs
                .iter()
                .find(|(candidate, _)| {
                    candidate.donor_container_tag == request.donor_container_tag
                        && candidate.icon_edit == request.icon_edit
                })
                .and_then(|(_, graph)| graph.as_ref());
            let local_ordinal = new_tags.len();
            let ordinal = AppendedTagAllocator::checked_ordinal(
                appended_ordinal_base,
                local_ordinal,
                "watermarked icon container",
            )?;
            let tag = assigned_tag(destination_package_id, current_entry_count, ordinal)?;
            let companion_ordinal = AppendedTagAllocator::checked_ordinal(
                appended_ordinal_base,
                local_ordinal + 1,
                "watermarked icon companion",
            )?;
            let companion_tag = assigned_tag(
                destination_package_id,
                current_entry_count,
                companion_ordinal,
            )?;
            let (donor_container, donor_companion) =
                read_and_validate_icon_container(manager, donor_container_tag)?;
            let mut container = donor_container.clone();
            let mut patched_offsets = vec![
                ICON_CONTENT_FINGERPRINT_OFFSET,
                ICON_WATERMARK_LAYER_OFFSET,
                ICON_RARITY_BACKGROUND_LAYER_OFFSET,
            ];
            write_tag(
                &mut container,
                ICON_RARITY_BACKGROUND_LAYER_OFFSET,
                request.rarity.icon_background_layer(),
            )?;
            if let Some((primary_layer_tag, _)) = edit_graph {
                write_tag(
                    &mut container,
                    ICON_PRIMARY_LAYER_OFFSET,
                    *primary_layer_tag,
                )?;
                patched_offsets.push(ICON_PRIMARY_LAYER_OFFSET);
            }
            write_tag(
                &mut container,
                ICON_WATERMARK_LAYER_OFFSET,
                watermark_layer_tag,
            )?;
            let fingerprint = private_icon_fingerprint(&container, visual_revision.as_slice());
            write_u32(&mut container, ICON_CONTENT_FINGERPRINT_OFFSET, fingerprint)?;
            validate_only_patched_fields(
                &donor_container,
                &container,
                &patched_offsets,
                "icon container",
            )?;
            let mut dependencies = collect_unchanged_container_dependencies(
                manager,
                donor_container_tag,
                &container,
                edit_graph.is_some(),
            )?;
            if let Some((_, primary_dependencies)) = edit_graph {
                dependencies.extend(primary_dependencies.iter().copied());
            }
            for pair in &texture_pairs {
                dependencies.insert(u32::from(pair.data_tag));
                dependencies.insert(u32::from(pair.header_tag));
            }
            dependencies.insert(u32::from(watermark_layer_tag));
            dependencies.insert(u32::from(tag));
            dependencies.insert(u32::from(companion_tag));
            let companion = build_shared_tag_companion_payload(
                &donor_companion.template_payload,
                companion_tag,
                tag,
                &dependencies,
            )?;
            new_tags.push(NewTagSpec {
                template_tag: donor_container_tag,
                payload: container,
                storage: NewTagStorageMode::InheritTemplate,
            });
            new_tags.push(NewTagSpec {
                template_tag: donor_companion.tag,
                payload: companion,
                storage: NewTagStorageMode::InheritTemplate,
            });
            icon_containers.push(WatermarkedIconContainer {
                donor_container_tag,
                donor_companion_tag: donor_companion.tag,
                icon_edit: request.icon_edit,
                rarity: request.rarity,
                authored_primary_layer_tag: edit_graph.map(|(tag, _)| *tag),
                ordinal,
                tag,
                companion_ordinal,
                companion_tag,
            });
            request_container_tags.push(tag);
            Ok(())
        })()
        .map_err(|error| error.context(request_context(request_index)))?;
    }

    validate_plan(WatermarkPlanValidation {
        new_tags: &new_tags,
        overrides: &reference_overrides,
        pairs: &texture_pairs,
        layer_ordinal: watermark_layer_ordinal,
        layer_tag: watermark_layer_tag,
        containers: &icon_containers,
        appended_ordinal_base,
        requests: icon_requests,
        request_container_tags: &request_container_tags,
    })?;
    let icon_container_tags = icon_containers
        .iter()
        .map(|container| container.tag)
        .collect();
    Ok(WatermarkPlan {
        new_tags,
        reference_overrides,
        #[cfg(test)]
        texture_pairs,
        #[cfg(test)]
        watermark_layer_ordinal,
        watermark_layer_tag,
        #[cfg(test)]
        icon_containers,
        icon_container_tags,
        request_container_tags,
    })
}

/// Returns an item-icon table row that selects an authored watermarked container.
///
/// Shadowkeep's 24-byte item-icon row stores the item identity at `+0x00` and the complete icon
/// container tag at `+0x10`; all other row bytes are preserved.
pub fn item_icon_row_with_container(
    donor_item_icon_row: &[u8],
    authored_item_hash: u32,
    authored_container_tag: TagHash,
) -> AuthoringResult<Vec<u8>> {
    if donor_item_icon_row.len() != ITEM_ICON_ROW_SIZE
        || authored_item_hash == 0
        || !is_valid_package_tag(authored_container_tag)
    {
        return Err(invalid(
            "A watermarked item-icon row needs exactly 24 donor bytes and valid item/container tags",
        ));
    }
    let mut row = donor_item_icon_row.to_vec();
    row[ITEM_ICON_IDENTITY_OFFSET..ITEM_ICON_IDENTITY_OFFSET + 4]
        .copy_from_slice(&authored_item_hash.to_le_bytes());
    write_tag(&mut row, ITEM_ICON_CONTAINER_OFFSET, authored_container_tag)?;
    validate_only_patched_fields(
        donor_item_icon_row,
        &row,
        &[ITEM_ICON_IDENTITY_OFFSET, ITEM_ICON_CONTAINER_OFFSET],
        "item-icon row",
    )?;
    Ok(row)
}

#[cfg(test)]
pub(crate) fn validate_icon_definition_graph(
    manager: &PackageManager,
    container_tag: TagHash,
) -> AuthoringResult<()> {
    read_and_validate_icon_container(manager, container_tag).map(|_| ())
}

fn read_and_validate_watermark_donor(manager: &PackageManager) -> AuthoringResult<Vec<u8>> {
    let layer = read_typed_tag(
        manager,
        DONOR_WATERMARK_LAYER,
        WATERMARK_LAYER_SIZE,
        0x08,
        0x00,
        "stock watermark template layer",
    )?;
    validate_entry_reference(
        manager,
        DONOR_WATERMARK_LAYER,
        WATERMARK_CLASS_HANDLE,
        "stock watermark template layer",
    )?;
    validate_layer_texture_graph(
        manager,
        DONOR_WATERMARK_LAYER,
        &layer,
        "stock watermark template",
    )?;
    if read_u64(&layer, 0x20)? != 1
        || read_i64(&layer, 0x28)? != 0x18
        || read_u32(&layer, 0x3C)? != 0x8080_9FBD
        || read_u64(&layer, 0x40)? != 1
        || read_u32(&layer, 0x48)? != 0x8080_4A6C
        || read_u64(&layer, 0x50)? != WATERMARK_TEXTURE_REFERENCE_COUNT as u64
        || read_i64(&layer, 0x58)? != 0x18
        || read_u32(&layer, 0x6C)? != 0x8080_9FBD
        || read_u64(&layer, 0x70)? != WATERMARK_TEXTURE_REFERENCE_COUNT as u64
        || read_u32(&layer, 0x78)? != 0x8080_4A6F
    {
        return Err(invalid(
            "Stock watermark template no longer has one lane with six texture variants",
        ));
    }
    for (index, expected) in DONOR_TEXTURE_HEADERS.iter().copied().enumerate() {
        if read_tag(&layer, WATERMARK_TEXTURE_REFERENCE_START + index * 4)? != expected {
            return Err(invalid(format!(
                "Stock watermark template slot {index} no longer references {expected}"
            )));
        }
    }
    Ok(layer)
}

fn read_and_validate_texture_header(
    manager: &PackageManager,
    header_tag: TagHash,
    data_tag: TagHash,
    width: u32,
    height: u32,
) -> AuthoringResult<(Vec<u8>, Vec<u8>)> {
    let data_size = width as usize * height as usize * 4;
    let data = read_typed_tag(
        manager,
        data_tag,
        data_size,
        0x28,
        0x01,
        "stock watermark template texture data",
    )?;
    let header = read_typed_tag(
        manager,
        header_tag,
        STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE,
        0x20,
        0x01,
        "stock watermark template texture header",
    )?;
    validate_entry_reference(
        manager,
        data_tag,
        u32::from(header_tag),
        "stock watermark template texture data",
    )?;
    validate_entry_reference(
        manager,
        header_tag,
        u32::from(data_tag),
        "stock watermark template texture header",
    )?;
    if data.len() != data_size
        || !is_stock_straight_rgba8_texture_header(&header, width, height, data_size)
    {
        return Err(invalid(format!(
            "Stock watermark template header {header_tag} no longer describes {width}x{height} straight RGBA8"
        )));
    }
    Ok((header, data))
}

fn read_and_validate_icon_container(
    manager: &PackageManager,
    container_tag: TagHash,
) -> AuthoringResult<(Vec<u8>, IconDefinitionCompanion)> {
    let container = read_typed_tag(
        manager,
        container_tag,
        ICON_CONTAINER_SIZE,
        0x10,
        0x00,
        "donor item-icon container",
    )?;
    validate_entry_reference(
        manager,
        container_tag,
        ICON_CONTAINER_CLASS_HANDLE,
        "donor item-icon container",
    )?;
    if read_u32(&container, 0)? != ICON_CONTAINER_SIZE as u32 {
        return Err(invalid(format!(
            "Donor item-icon container {container_tag} is not a complete Shadowkeep weapon icon"
        )));
    }
    let mut dependencies = SharedTagDependencies::new();
    for offset in ICON_LAYER_REFERENCE_OFFSETS {
        let raw = read_u32(&container, offset)?;
        let required = offset == ICON_PRIMARY_LAYER_OFFSET;
        if let Some(layer_dependencies) = validate_optional_resource_reference(
            raw,
            required,
            &format!("donor icon-container {container_tag} layer at +0x{offset:02X}"),
            |layer_tag| validate_icon_layer(manager, layer_tag, container_tag, offset),
        )? {
            dependencies.extend(layer_dependencies);
        }
    }
    let companion = read_and_validate_icon_companion(manager, container_tag)?;
    dependencies.insert(u32::from(container_tag));
    dependencies.insert(u32::from(companion.tag));
    if dependencies != companion.dependencies {
        let missing = dependencies
            .difference(&companion.dependencies)
            .map(|raw| format!("0x{raw:08X}"))
            .collect::<Vec<_>>();
        let unexpected = companion
            .dependencies
            .difference(&dependencies)
            .map(|raw| format!("0x{raw:08X}"))
            .collect::<Vec<_>>();
        return Err(invalid(format!(
            "Donor icon definition {container_tag} companion {} does not exactly describe its reachable graph (missing: {}; unexpected: {})",
            companion.tag,
            display_set_difference(&missing),
            display_set_difference(&unexpected)
        )));
    }
    Ok((container, companion))
}

fn collect_unchanged_container_dependencies(
    manager: &PackageManager,
    donor_container_tag: TagHash,
    donor_container: &[u8],
    primary_is_authored: bool,
) -> AuthoringResult<SharedTagDependencies> {
    let mut dependencies = SharedTagDependencies::new();
    for offset in ICON_LAYER_REFERENCE_OFFSETS {
        if offset == ICON_WATERMARK_LAYER_OFFSET
            || (primary_is_authored && offset == ICON_PRIMARY_LAYER_OFFSET)
        {
            continue;
        }
        let raw = read_u32(donor_container, offset)?;
        let required = offset == ICON_PRIMARY_LAYER_OFFSET;
        if let Some(layer_dependencies) = validate_optional_resource_reference(
            raw,
            required,
            &format!(
                "authored clone of icon-container {donor_container_tag} layer at +0x{offset:02X}"
            ),
            |layer_tag| validate_icon_layer(manager, layer_tag, donor_container_tag, offset),
        )? {
            dependencies.extend(layer_dependencies);
        }
    }
    Ok(dependencies)
}

fn display_set_difference(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_owned()
    } else {
        values.join(", ")
    }
}

fn validate_icon_layer(
    manager: &PackageManager,
    layer_tag: TagHash,
    container_tag: TagHash,
    container_offset: usize,
) -> AuthoringResult<SharedTagDependencies> {
    let description = format!(
        "donor icon-container {container_tag} layer {layer_tag} at +0x{container_offset:02X}"
    );
    let entry = manager
        .get_entry(layer_tag)
        .ok_or_else(|| invalid(format!("{description} has no package entry")))?;
    if entry.file_type != 0x08
        || entry.file_subtype != 0x00
        || entry.reference != WATERMARK_CLASS_HANDLE
    {
        return Err(invalid(format!(
            "{description} has type/reference {:02X}/{:02X}/0x{:08X}; expected 08/00/0x{WATERMARK_CLASS_HANDLE:08X}",
            entry.file_type, entry.file_subtype, entry.reference
        )));
    }
    let layer = manager
        .read_tag(layer_tag)
        .map_err(|error| invalid(format!("Could not read {description}: {error}")))?;
    if layer.len() != entry.file_size as usize || read_u32(&layer, 0)? as usize != layer.len() {
        return Err(invalid(format!(
            "{description} decoded to {} bytes while its entry/payload declares {}/{}",
            layer.len(),
            entry.file_size,
            read_u32(&layer, 0)?
        )));
    }
    validate_layer_texture_graph(manager, layer_tag, &layer, &description)
}

fn validate_layer_texture_graph(
    manager: &PackageManager,
    layer_tag: TagHash,
    layer: &[u8],
    description: &str,
) -> AuthoringResult<SharedTagDependencies> {
    let mut dependencies = SharedTagDependencies::new();
    dependencies.insert(u32::from(layer_tag));
    validate_layer_texture_references(layer, description, |header_tag| {
        let data_tag = validate_texture_resource_pair(manager, layer_tag, header_tag, description)?;
        dependencies.insert(u32::from(header_tag));
        dependencies.insert(u32::from(data_tag));
        Ok(())
    })?;
    Ok(dependencies)
}

fn validate_layer_texture_references(
    layer: &[u8],
    description: &str,
    mut validate_header: impl FnMut(TagHash) -> AuthoringResult<()>,
) -> AuthoringResult<()> {
    let lane_count = usize::try_from(read_u64(layer, 0x20)?)
        .map_err(|_| invalid(format!("{description} lane count exceeds this platform")))?;
    if lane_count == 0 || lane_count > MAX_LAYER_LANES {
        return Err(invalid(format!(
            "{description} has invalid icon-layer lane count {lane_count}"
        )));
    }
    let lanes = relative_target(layer, 0x28, description)?;
    if lanes < 4
        || read_u32(layer, lanes - 4)? != LAYER_ARRAY_CLASS
        || usize::try_from(read_u64(layer, lanes)?).ok() != Some(lane_count)
        || read_u32(layer, lanes + 8)? != LAYER_LANE_CLASS
    {
        return Err(invalid(format!(
            "{description} has an invalid icon-layer lane array"
        )));
    }
    let descriptors = lanes
        .checked_add(0x10)
        .ok_or_else(|| invalid(format!("{description} lane descriptors overflowed")))?;
    for lane_index in 0..lane_count {
        let descriptor = descriptors
            .checked_add(lane_index * 0x10)
            .ok_or_else(|| invalid(format!("{description} lane descriptor overflowed")))?;
        let texture_count = usize::try_from(read_u64(layer, descriptor)?)
            .map_err(|_| invalid(format!("{description} texture count exceeds this platform")))?;
        if texture_count == 0 || texture_count > MAX_TEXTURES_PER_LANE {
            return Err(invalid(format!(
                "{description} lane {lane_index} has invalid texture count {texture_count}"
            )));
        }
        let textures = relative_target(layer, descriptor + 8, description)?;
        if textures < 4
            || read_u32(layer, textures - 4)? != LAYER_ARRAY_CLASS
            || usize::try_from(read_u64(layer, textures)?).ok() != Some(texture_count)
            || read_u32(layer, textures + 8)? != LAYER_TEXTURE_CLASS
        {
            return Err(invalid(format!(
                "{description} lane {lane_index} has an invalid texture array"
            )));
        }
        let tags = textures
            .checked_add(0x10)
            .ok_or_else(|| invalid(format!("{description} texture tags overflowed")))?;
        for texture_index in 0..texture_count {
            let raw = read_u32(layer, tags + texture_index * 4)?;
            validate_optional_resource_reference(
                raw,
                true,
                &format!("{description} lane {lane_index} texture {texture_index} header"),
                &mut validate_header,
            )?;
        }
    }
    Ok(())
}

fn validate_texture_resource_pair(
    manager: &PackageManager,
    layer_tag: TagHash,
    header_tag: TagHash,
    description: &str,
) -> AuthoringResult<TagHash> {
    let header_description = format!("{description} texture header {header_tag} from {layer_tag}");
    let header = read_typed_tag(
        manager,
        header_tag,
        STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE,
        0x20,
        0x01,
        &header_description,
    )?;
    let header_entry = manager
        .get_entry(header_tag)
        .ok_or_else(|| invalid(format!("{header_description} has no package entry")))?;
    let data_tag = TagHash(header_entry.reference);
    let data_size = read_u32(&header, 0)? as usize;
    if data_size == 0 {
        return Err(invalid(format!(
            "{header_description} declares an empty texture payload"
        )));
    }
    validate_optional_resource_reference(
        u32::from(data_tag),
        true,
        &format!("{header_description} data"),
        |resolved_data_tag| {
            read_typed_tag(
                manager,
                resolved_data_tag,
                data_size,
                0x28,
                0x01,
                &format!("{header_description} data"),
            )?;
            validate_entry_reference(
                manager,
                resolved_data_tag,
                u32::from(header_tag),
                &format!("{header_description} data"),
            )
        },
    )?;
    Ok(data_tag)
}

fn validate_optional_resource_reference<T>(
    raw: u32,
    required: bool,
    description: &str,
    resolve: impl FnOnce(TagHash) -> AuthoringResult<T>,
) -> AuthoringResult<Option<T>> {
    if raw == u32::MAX {
        if required {
            return Err(invalid(format!("{description} is required but absent")));
        }
        return Ok(None);
    }
    let tag = TagHash(raw);
    if !is_valid_package_tag(tag) {
        return Err(invalid(format!(
            "{description} contains malformed reference 0x{raw:08X}; absent references must be 0xFFFFFFFF"
        )));
    }
    resolve(tag).map(Some).map_err(|error| {
        invalid(format!(
            "{description} references unresolved or incompatible tag {tag}: {error}"
        ))
    })
}

fn read_typed_tag(
    manager: &PackageManager,
    tag: TagHash,
    expected_size: usize,
    expected_type: u8,
    expected_subtype: u8,
    description: &str,
) -> AuthoringResult<Vec<u8>> {
    let entry = manager
        .get_entry(tag)
        .ok_or_else(|| invalid(format!("{description} {tag} has no package entry")))?;
    if entry.file_size as usize != expected_size
        || entry.file_type != expected_type
        || entry.file_subtype != expected_subtype
    {
        return Err(invalid(format!(
            "{description} {tag} has size/type {}/{:02X}/{:02X}; expected {expected_size}/{expected_type:02X}/{expected_subtype:02X}",
            entry.file_size, entry.file_type, entry.file_subtype
        )));
    }
    let payload = manager
        .read_tag(tag)
        .map_err(|error| invalid(format!("Could not read {description} {tag}: {error}")))?;
    if payload.len() != expected_size {
        return Err(invalid(format!(
            "{description} {tag} decoded to {} bytes; expected {expected_size}",
            payload.len()
        )));
    }
    Ok(payload)
}

fn validate_entry_reference(
    manager: &PackageManager,
    tag: TagHash,
    expected: u32,
    description: &str,
) -> AuthoringResult<()> {
    let entry = manager
        .get_entry(tag)
        .ok_or_else(|| invalid(format!("{description} {tag} has no package entry")))?;
    if entry.reference != expected {
        return Err(invalid(format!(
            "{description} {tag} references 0x{:08X}; expected 0x{expected:08X}",
            entry.reference
        )));
    }
    Ok(())
}

pub(crate) fn decode_authored_texture(
    texture_index: usize,
    width: u32,
    height: u32,
) -> AuthoringResult<Vec<u8>> {
    let pixels = decode_source_texture(texture_index, width, height)?;
    placement::adjust_corner_glyph(texture_index, width, height, pixels)
}

fn decode_source_texture(
    texture_index: usize,
    width: u32,
    height: u32,
) -> AuthoringResult<Vec<u8>> {
    let png = AUTHORED_TEXTURE_PNGS
        .get(texture_index)
        .ok_or_else(|| invalid(format!("Unknown Sunrise watermark texture {texture_index}")))?;
    let expected_hash = AUTHORED_TEXTURE_PNG_SHA1
        .get(texture_index)
        .ok_or_else(|| {
            invalid(format!(
                "Missing hash for watermark texture {texture_index}"
            ))
        })?;
    if Sha1::digest(png).as_slice() != expected_hash {
        return Err(invalid(format!(
            "Pre-rendered Sunrise watermark texture {texture_index} no longer matches its audited asset"
        )));
    }
    let image = image::load_from_memory_with_format(png, ImageFormat::Png)
        .map_err(|error| {
            invalid(format!(
                "Could not decode pre-rendered Sunrise watermark texture {texture_index}: {error}"
            ))
        })?
        .into_rgba8();
    if image.dimensions() != (width, height) {
        return Err(invalid(format!(
            "Pre-rendered Sunrise watermark texture {texture_index} is {}x{}; expected {width}x{height}",
            image.width(),
            image.height()
        )));
    }
    Ok(image.into_raw())
}

/// Shared high-resolution output for package textures and editor previews. This resamples
/// the approved small-scale design, not the original full-size Sunrise logo.
pub(crate) fn render_output_texture(texture_index: usize) -> AuthoringResult<image::RgbaImage> {
    let &(width, height) = TEXTURE_DIMENSIONS
        .get(texture_index)
        .ok_or_else(|| invalid("Unknown Sunrise watermark texture"))?;
    let pixels = placement::render_output(
        texture_index,
        width,
        height,
        decode_source_texture(texture_index, width, height)?,
    )?;
    image::RgbaImage::from_raw(
        width * OUTPUT_TEXTURE_SCALE,
        height * OUTPUT_TEXTURE_SCALE,
        pixels,
    )
    .ok_or_else(|| validation("Rendered watermark has invalid dimensions"))
}

fn upscale_texture(width: u32, height: u32, pixels: Vec<u8>) -> AuthoringResult<image::RgbaImage> {
    let source = image::RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| validation("Watermark RGBA dimensions do not match its payload"))?;
    Ok(crate::icon_edit::fit_rgba_image(
        &source,
        width * OUTPUT_TEXTURE_SCALE,
        height * OUTPUT_TEXTURE_SCALE,
    ))
}

fn resize_texture_header(
    header: &mut [u8],
    width: u32,
    height: u32,
    data_size: usize,
) -> AuthoringResult<()> {
    let width_field = u16::try_from(width)
        .map_err(|_| validation("Watermark width exceeds its native header field"))?;
    let height_field = u16::try_from(height)
        .map_err(|_| validation("Watermark height exceeds its native header field"))?;
    let size_field = u32::try_from(data_size)
        .map_err(|_| validation("Watermark payload exceeds its native header field"))?;
    if header.len() != STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE {
        return Err(validation("Watermark texture header has an invalid size"));
    }
    write_u32(header, 0, size_field)?;
    header[14..16].copy_from_slice(&width_field.to_le_bytes());
    header[16..18].copy_from_slice(&height_field.to_le_bytes());
    if !is_stock_straight_rgba8_texture_header(header, width, height, data_size) {
        return Err(validation(
            "High-resolution watermark texture header is invalid",
        ));
    }
    Ok(())
}

fn validate_authored_texture(
    texture_index: usize,
    width: u32,
    height: u32,
    donor: &[u8],
    authored: &[u8],
) -> AuthoringResult<()> {
    let expected_size = width as usize * height as usize * 4;
    if donor.len() != expected_size || authored.len() != expected_size {
        return Err(validation(format!(
            "Sunrise watermark texture {texture_index} has malformed donor/authored RGBA dimensions"
        )));
    }

    if matches!(texture_index, 2 | 3) {
        return validate_standalone_texture(texture_index, width, donor, authored);
    }

    let (min_x, min_y, max_x, max_y) = match texture_index {
        0 | 4 => (1, 2, 31, 29),
        1 | 5 => (19, 1, 53, 32),
        _ => {
            return Err(validation(format!(
                "Sunrise watermark texture {texture_index} has no audited lane semantics"
            )));
        }
    };
    let mut changed = 0usize;
    for (pixel_index, (before, after)) in donor
        .chunks_exact(4)
        .zip(authored.chunks_exact(4))
        .enumerate()
    {
        if before == after {
            continue;
        }
        changed += 1;
        let x = pixel_index as u32 % width;
        let y = pixel_index as u32 / width;
        if x < min_x || x > max_x || y < min_y || y > max_y {
            return Err(validation(format!(
                "Sunrise watermark texture {texture_index} changed native earmark pixel ({x}, {y}) outside the audited glyph bounds"
            )));
        }
    }
    let expected_polarity = authored.chunks_exact(4).any(|pixel| {
        pixel[3] >= 200
            && if texture_index < 2 {
                pixel[0..3].iter().all(|channel| *channel >= 220)
            } else {
                pixel[0..3].iter().all(|channel| *channel <= 32)
            }
    });
    if changed == 0 || !expected_polarity {
        return Err(validation(format!(
            "Sunrise watermark texture {texture_index} does not contain its expected {} glyph",
            if texture_index < 2 { "white" } else { "black" }
        )));
    }
    Ok(())
}

fn validate_standalone_texture(
    texture_index: usize,
    width: u32,
    donor: &[u8],
    authored: &[u8],
) -> AuthoringResult<()> {
    let donor_alpha = donor
        .chunks_exact(4)
        .map(|pixel| pixel[3])
        .collect::<Vec<_>>();
    let authored_alpha = authored
        .chunks_exact(4)
        .map(|pixel| pixel[3])
        .collect::<Vec<_>>();
    if donor
        .chunks_exact(4)
        .zip(authored.chunks_exact(4))
        .any(|(before, after)| before[..3] != after[..3])
    {
        return Err(validation(format!(
            "Sunrise standalone watermark {texture_index} changed its native RGB polarity"
        )));
    }
    if Sha1::digest(&donor_alpha).as_slice() != DONOR_STANDALONE_ALPHA_SHA1 {
        return Err(invalid(format!(
            "Standalone watermark donor {texture_index} no longer has its audited alpha mask"
        )));
    }
    if donor_alpha == authored_alpha
        || Sha1::digest(&authored_alpha).as_slice() != AUTHORED_STANDALONE_ALPHA_SHA1
        || alpha_bounds(&authored_alpha, width) != Some(AUTHORED_STANDALONE_ALPHA_BOUNDS)
    {
        return Err(validation(format!(
            "Sunrise standalone watermark {texture_index} does not contain its audited Sunrise alpha mask"
        )));
    }
    Ok(())
}

fn alpha_bounds(alpha: &[u8], width: u32) -> Option<(u32, u32, u32, u32)> {
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for (index, value) in alpha.iter().copied().enumerate() {
        if value == 0 {
            continue;
        }
        let x = index as u32 % width;
        let y = index as u32 / width;
        bounds = Some(match bounds {
            None => (x, y, x, y),
            Some((min_x, min_y, max_x, max_y)) => {
                (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
            }
        });
    }
    bounds
}

pub(crate) fn private_icon_fingerprint(container: &[u8], visual_revision: &[u8]) -> u32 {
    let mut digest = Sha1::new();
    digest.update(b"parhelion.icon-composition.v1");
    digest.update(&container[ICON_CONTENT_FINGERPRINT_OFFSET + 4..]);
    digest.update(visual_revision);
    let bytes = digest.finalize();
    u32::from_le_bytes(bytes[..4].try_into().expect("SHA-1 prefix"))
}

fn assigned_tag(
    package_id: u16,
    current_entry_count: usize,
    ordinal: usize,
) -> AuthoringResult<TagHash> {
    AppendedTagAllocator::new(package_id, current_entry_count).assigned_tag(
        ordinal,
        "Watermark destination entry",
        "watermark tag",
    )
}

struct WatermarkPlanValidation<'a> {
    new_tags: &'a [NewTagSpec],
    overrides: &'a [NewTagReferenceOverride],
    pairs: &'a [WatermarkTexturePair; 6],
    layer_ordinal: usize,
    layer_tag: TagHash,
    containers: &'a [WatermarkedIconContainer],
    appended_ordinal_base: usize,
    requests: &'a [WeaponIconRequest],
    request_container_tags: &'a [TagHash],
}

fn validate_plan(context: WatermarkPlanValidation<'_>) -> AuthoringResult<()> {
    let WatermarkPlanValidation {
        new_tags,
        overrides,
        pairs,
        layer_ordinal,
        layer_tag,
        containers,
        appended_ordinal_base,
        requests,
        request_container_tags,
    } = context;
    if new_tags.len() < SHARED_TAG_COUNT + containers.len() * 2
        || overrides.len() < TEXTURE_DIMENSIONS.len() * 2
        || pairs[0].data_ordinal != appended_ordinal_base
        || pairs.iter().enumerate().any(|(index, pair)| {
            pair.data_ordinal + 1 != pair.header_ordinal
                || new_tags
                    .get(pair.data_ordinal - pairs[0].data_ordinal)
                    .is_none_or(|tag| {
                        tag.template_tag != DONOR_TEXTURE_DATA[index]
                            || tag.storage != NewTagStorageMode::InheritTemplate
                            || tag.payload.len() != pair.width as usize * pair.height as usize * 4
                    })
                || new_tags
                    .get(pair.header_ordinal - pairs[0].data_ordinal)
                    .is_none_or(|tag| {
                        tag.template_tag != DONOR_TEXTURE_HEADERS[index]
                            || tag.storage != NewTagStorageMode::InheritTemplate
                            || !is_stock_straight_rgba8_texture_header(
                                &tag.payload,
                                pair.width,
                                pair.height,
                                pair.width as usize * pair.height as usize * 4,
                            )
                    })
        })
        || layer_ordinal != pairs[5].header_ordinal + 1
        || new_tags[TEXTURE_DIMENSIONS.len() * 2].template_tag != DONOR_WATERMARK_LAYER
        || new_tags[TEXTURE_DIMENSIONS.len() * 2].storage != NewTagStorageMode::InheritTemplate
        || new_tags[TEXTURE_DIMENSIONS.len() * 2].payload.len() != WATERMARK_LAYER_SIZE
        || requests.len() != request_container_tags.len()
        || containers.last().is_none_or(|container| {
            container
                .companion_ordinal
                .checked_sub(appended_ordinal_base)
                .and_then(|index| index.checked_add(1))
                != Some(new_tags.len())
        })
        || containers.iter().any(|container| {
            let Some(definition_index) = container.ordinal.checked_sub(appended_ordinal_base)
            else {
                return true;
            };
            let companion_index = definition_index + 1;
            companion_index >= new_tags.len()
                || container.companion_ordinal != container.ordinal + 1
                || container.companion_tag.pkg_id() != container.tag.pkg_id()
                || container.companion_tag.entry_index()
                    != container.tag.entry_index().saturating_add(1)
                || new_tags[definition_index].template_tag != container.donor_container_tag
                || new_tags[definition_index].storage != NewTagStorageMode::InheritTemplate
                || new_tags[definition_index].payload.len() != ICON_CONTAINER_SIZE
                || read_tag(
                    &new_tags[definition_index].payload,
                    ICON_WATERMARK_LAYER_OFFSET,
                )
                .ok()
                    != Some(layer_tag)
                || read_tag(
                    &new_tags[definition_index].payload,
                    ICON_RARITY_BACKGROUND_LAYER_OFFSET,
                )
                .ok()
                    != Some(container.rarity.icon_background_layer())
                || container.icon_edit.is_identity()
                    != container.authored_primary_layer_tag.is_none()
                || container.authored_primary_layer_tag.is_some_and(|primary| {
                    read_tag(
                        &new_tags[definition_index].payload,
                        ICON_PRIMARY_LAYER_OFFSET,
                    )
                    .ok()
                        != Some(primary)
                })
                || new_tags[companion_index].template_tag != container.donor_companion_tag
                || new_tags[companion_index].storage != NewTagStorageMode::InheritTemplate
                || read_u64(&new_tags[companion_index].payload, 0).ok()
                    != Some(new_tags[companion_index].payload.len() as u64)
                || read_tag(&new_tags[companion_index].payload, 0x08).ok()
                    != Some(container.companion_tag)
                || read_tag(&new_tags[companion_index].payload, 0x0C).ok() != Some(container.tag)
        })
        || requests
            .iter()
            .zip(request_container_tags)
            .any(|(request, tag)| {
                !containers.iter().any(|container| {
                    container.tag == *tag
                        && container.donor_container_tag == request.donor_container_tag
                        && container.icon_edit == request.icon_edit
                        && container.rarity == request.rarity
                })
            })
    {
        return Err(validation(
            "Sunrise weapon watermark append plan failed its structural validation",
        ));
    }
    Ok(())
}

fn validate_only_patched_fields(
    donor: &[u8],
    authored: &[u8],
    field_offsets: &[usize],
    description: &str,
) -> AuthoringResult<()> {
    let allowed = field_offsets
        .iter()
        .map(|field| *field..*field + 4)
        .collect::<Vec<_>>();
    if !donor_mutation_is_limited_to(donor, authored, &allowed) {
        return Err(validation(format!(
            "Authored {description} changed donor bytes outside its audited tag field(s)"
        )));
    }
    Ok(())
}

fn read_tag(data: &[u8], offset: usize) -> AuthoringResult<TagHash> {
    Ok(TagHash(read_u32(data, offset)?))
}

fn write_tag(data: &mut [u8], offset: usize, tag: TagHash) -> AuthoringResult<()> {
    write_u32(data, offset, u32::from(tag))
}

#[cfg(test)]
mod tests;

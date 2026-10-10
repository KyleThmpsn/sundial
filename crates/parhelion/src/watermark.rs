//! Shared authoring plan for Parhelion's runtime-specific weapon-icon watermark.
//!
//! Shadowkeep item icon rows select a complete icon container. The container keeps the donor's
//! primary weapon art and rarity background in separate fields, while offset `0x20` selects the
//! season/expansion overlay layer. This module authors that layer once and then clones only the
//! requested donor containers, selecting the authored rarity background and runtime watermark.

mod custom;
mod dawn;
mod placement;
mod validation;
pub(crate) use validation::validate_icon_layer;
use validation::*;
mod textures;
pub(crate) use custom::preview as render_custom_corner_preview;
pub(crate) use custom::render as render_custom_corner;
pub(crate) use custom::{Presentation, build_presented_watermark_plan};
pub(crate) use dawn::render as render_dawn_texture;
pub(crate) use textures::decode_authored_texture;
pub(crate) use textures::private_icon_fingerprint;
pub(crate) use textures::render_output_texture;
use textures::*;

use image::ImageFormat;
use sha1::{Digest, Sha1};
use sundial::package_authoring::PackageManager;
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
use tiger_pkg::TagHash;

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
    /// Keeps the base container's own background and watermark layers. A subclass icon has
    /// neither a rarity plate nor a watermark.
    pub plain: bool,
}

/// One cloned base-art container that points at the shared authored watermark layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatermarkedIconContainer {
    pub donor_container_tag: TagHash,
    pub donor_companion_tag: TagHash,
    pub icon_edit: WeaponIconEdit,
    pub rarity: AuthoredWeaponRarity,
    pub plain: bool,
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
#[cfg(any(test, feature = "d2-model-importer"))]
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
        crate::branding::Branding::Sunrise,
        &|index| format!("Icon Donor: {}", icon_requests[index].donor_container_tag),
    )
}

fn build_watermark_plan_with_context(
    manager: &PackageManager,
    destination_package_id: u16,
    current_entry_count: usize,
    appended_ordinal_base: usize,
    icon_requests: &[WeaponIconRequest],
    branding: crate::branding::Branding,
    request_context: &dyn Fn(usize) -> String,
) -> AuthoringResult<WatermarkPlan> {
    if icon_requests.is_empty() {
        return Err(invalid(
            "A weapon watermark plan needs at least one donor icon container",
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
        let output = authored_texture(branding, texture_index, width, height, &donor_pixels)?;
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
                let primary_layer_tag = graph.layer_tag;
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
                && container.plain == request.plain
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
            let mut patched_offsets = vec![ICON_CONTENT_FINGERPRINT_OFFSET];
            if !request.plain {
                patched_offsets.extend([
                    ICON_WATERMARK_LAYER_OFFSET,
                    ICON_RARITY_BACKGROUND_LAYER_OFFSET,
                ]);
                write_tag(
                    &mut container,
                    ICON_RARITY_BACKGROUND_LAYER_OFFSET,
                    request.rarity.icon_background_layer(),
                )?;
                write_tag(
                    &mut container,
                    ICON_WATERMARK_LAYER_OFFSET,
                    watermark_layer_tag,
                )?;
            }
            if let Some((primary_layer_tag, _)) = edit_graph {
                write_tag(
                    &mut container,
                    ICON_PRIMARY_LAYER_OFFSET,
                    *primary_layer_tag,
                )?;
                patched_offsets.push(ICON_PRIMARY_LAYER_OFFSET);
            }
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
            if !request.plain {
                for pair in &texture_pairs {
                    dependencies.insert(u32::from(pair.data_tag));
                    dependencies.insert(u32::from(pair.header_tag));
                }
                dependencies.insert(u32::from(watermark_layer_tag));
            }
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
                plain: request.plain,
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

fn authored_texture(
    branding: crate::branding::Branding,
    index: usize,
    width: u32,
    height: u32,
    donor: &[u8],
) -> AuthoringResult<image::RgbaImage> {
    // The serialized texture graph is shared by both runtimes. Only Sunrise's
    // authored glyph is a pixel-limited edit of the audited stock alpha mask.
    if branding == crate::branding::Branding::Sunrise {
        let pixels = decode_authored_texture(index, width, height)?;
        validate_authored_texture(index, width, height, donor, &pixels)?;
    }
    branding.texture(index)
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
                || (!container.plain
                    && (read_tag(
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
                            != Some(container.rarity.icon_background_layer())))
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
                        && container.plain == request.plain
                })
            })
    {
        return Err(validation(
            "Weapon watermark append plan failed its structural validation",
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

//! An emblem's nameplate container for a recipe: the base's container with each image's layer
//! taken from its source, a picture painted over a copy of the base's layer for that image.
use super::{Nameplate, NameplateImage, NameplatePart};
use crate::image_import::EmbeddedImage;
use crate::shared_tag_memory::SharedTagDependencies;
use crate::tag_payload::{read_u32, write_u32};
use crate::{
    AuthoringResult, NewTagReferenceOverride, NewTagSpec, NewTagStorageMode,
    error::{invalid, validation},
};
use sundial::package_authoring::icon_schema::ICON_LAYER_REFERENCE_OFFSETS;
use sundial::package_authoring::{PackageManager, is_valid_package_tag};
use tiger_pkg::TagHash;

/// The container's second reference to the banner, which stock nameplates keep equal to the first.
const BANNER_COPY_OFFSET: usize = 0x28;
/// The container's fingerprint, which an authored container sets from what it shows.
const FINGERPRINT_OFFSET: usize = 0x10;
/// The two colors that follow the layers. They go with the banner, so another emblem's banner
/// brings its own.
const COLORS: std::ops::Range<usize> = 0x30..0x50;

/// One nameplate image as the build takes it.
#[derive(Clone)]
enum Source {
    /// The base container's own layer, or none where it has none.
    Base,
    /// Another emblem's layer, kept as it is.
    Layer(TagHash),
    /// A picture of the recipe's own, painted over a copy of the base's layer for the image.
    Image {
        image: EmbeddedImage,
        layout: TagHash,
    },
}

/// A recipe's nameplate read against the packages: the base's container and its icon row, which
/// the authored ones copy, where each image comes from, and whose colors go with the banner.
#[derive(Clone)]
pub(crate) struct ResolvedNameplate {
    pub(crate) template_row: u16,
    template: TagHash,
    sources: Vec<(NameplatePart, Source)>,
    colors: Option<TagHash>,
    custom_colors: Option<[[f32; 4]; 2]>,
}

/// Reads a recipe's nameplate against the packages. `base` is the base emblem's nameplate row and
/// container. `container_of` gives another emblem's nameplate container and refuses an item that is
/// not an emblem.
pub(crate) fn resolve(
    manager: &PackageManager,
    base: (u16, TagHash),
    nameplate: &Nameplate,
    container_of: &dyn Fn(u32) -> AuthoringResult<TagHash>,
) -> AuthoringResult<ResolvedNameplate> {
    let (template_row, template) = base;
    let template_payload = crate::icon_edit::read_icon_container(manager, template)?;
    let layer_in = |payload: &[u8], part: NameplatePart| -> AuthoringResult<Option<TagHash>> {
        let tag = TagHash(read_u32(payload, part.layer_offset())?);
        Ok(is_valid_package_tag(tag).then_some(tag))
    };
    let mut colors = None;
    let mut sources = Vec::with_capacity(NameplatePart::ALL.len());
    for part in NameplatePart::ALL {
        let source = match nameplate.part(part) {
            None => Source::Base,
            Some(NameplateImage::Emblem { item_hash }) => {
                let hash = item_hash.parse_u32().map_err(|error| invalid(error.to_string()))?;
                let container = container_of(hash)?;
                let payload = crate::icon_edit::read_icon_container(manager, container)?;
                let layer = layer_in(&payload, part)?.ok_or_else(|| {
                    invalid(format!(
                        "Emblem 0x{hash:08X} has no {} image",
                        part.label().to_lowercase()
                    ))
                })?;
                if part == NameplatePart::Banner {
                    colors = Some(container);
                }
                Source::Layer(layer)
            }
            Some(NameplateImage::Image { image }) => Source::Image {
                image: image.clone(),
                layout: layer_in(&template_payload, part)?.ok_or_else(|| {
                    invalid(format!(
                        "The base emblem has no {} for a picture to take the place of. Take it from another emblem.",
                        part.label().to_lowercase()
                    ))
                })?,
            },
        };
        sources.push((part, source));
    }
    Ok(ResolvedNameplate {
        template_row,
        template,
        sources,
        colors,
        custom_colors: nameplate
            .colors
            .map(|colors| colors.map(|color| color.map(|value| value.get()))),
    })
}

/// Builds the authored nameplate container and its companion into `nodes`, with any repainted
/// layers before them, and returns the container.
pub(crate) fn author(
    manager: &PackageManager,
    nameplate: &ResolvedNameplate,
    nodes: &mut Vec<NewTagSpec>,
    references: &mut Vec<NewTagReferenceOverride>,
) -> AuthoringResult<TagHash> {
    let package = crate::package_profile::PARHELION_ASSET_PACKAGE_ID;
    let allocator = crate::appended_tags::AppendedTagAllocator::new(package, 0);
    let mut container = crate::icon_edit::read_icon_container(manager, nameplate.template)?;
    let banner_offset = NameplatePart::Banner.layer_offset();
    let banner_copied =
        read_u32(&container, BANNER_COPY_OFFSET)? == read_u32(&container, banner_offset)?;
    let mut dependencies = SharedTagDependencies::new();
    let mut painted = Vec::new();
    let mut revision = Vec::new();
    for (part, source) in &nameplate.sources {
        let layer = match source {
            Source::Base => continue,
            Source::Layer(layer) => *layer,
            Source::Image { image, layout } => {
                let plan = crate::icon_edit::build_layer_repaint_plan(
                    manager,
                    package,
                    0,
                    nodes.len(),
                    (*layout, nameplate.template),
                    &|pixels, width, height| paint(image, pixels, width, height),
                )?;
                // The painted pixels, so a change in how a picture is sized refreshes the icon.
                revision.extend(
                    plan.new_tags
                        .iter()
                        .flat_map(|node| node.payload.iter().copied()),
                );
                dependencies.extend(plan.dependencies);
                nodes.extend(plan.new_tags);
                references.extend(plan.reference_overrides);
                painted.push(plan.layer_tag);
                plan.layer_tag
            }
        };
        write_u32(&mut container, part.layer_offset(), layer.0)?;
        if *part == NameplatePart::Banner && banner_copied {
            write_u32(&mut container, BANNER_COPY_OFFSET, layer.0)?;
        }
    }
    // Stock layers the container keeps, its own or other emblems', stay resident through it.
    for offset in ICON_LAYER_REFERENCE_OFFSETS {
        let layer = TagHash(read_u32(&container, offset)?);
        if is_valid_package_tag(layer) && !painted.contains(&layer) {
            dependencies.extend(crate::watermark::validate_icon_layer(
                manager,
                layer,
                nameplate.template,
                offset,
            )?);
        }
    }
    if let Some(source) = nameplate.colors {
        let colors = crate::icon_edit::read_icon_container(manager, source)?;
        container[COLORS].copy_from_slice(&colors[COLORS]);
    }
    if let Some(colors) = nameplate.custom_colors {
        for (lane, value) in colors.into_iter().flatten().enumerate() {
            container[COLORS.start + lane * 4..COLORS.start + lane * 4 + 4]
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    let fingerprint = crate::watermark::private_icon_fingerprint(&container, &revision);
    write_u32(&mut container, FINGERPRINT_OFFSET, fingerprint)?;
    let private_container =
        allocator.assigned_tag(nodes.len(), "Nameplate definition", "emblem nameplate")?;
    nodes.push(NewTagSpec {
        template_tag: nameplate.template,
        payload: container,
        storage: NewTagStorageMode::InheritTemplate,
    });
    let companion_tag =
        allocator.assigned_tag(nodes.len(), "Nameplate companion", "emblem nameplate")?;
    dependencies.extend([private_container.0, companion_tag.0]);
    let donor =
        crate::shared_tag_memory::read_and_validate_icon_companion(manager, nameplate.template)?;
    let companion = crate::shared_tag_memory::build_shared_tag_companion_payload(
        &donor.template_payload,
        companion_tag,
        private_container,
        &dependencies,
    )?;
    nodes.push(NewTagSpec {
        template_tag: donor.tag,
        payload: companion,
        storage: NewTagStorageMode::InheritTemplate,
    });
    Ok(private_container)
}

/// One image of a nameplate container at its own size, as an export writes it.
pub(crate) fn layer_pixels(
    manager: &PackageManager,
    container: TagHash,
    part: NameplatePart,
) -> AuthoringResult<image::RgbaImage> {
    let payload = crate::icon_edit::read_icon_container(manager, container)?;
    let layer = TagHash(read_u32(&payload, part.layer_offset())?);
    if !is_valid_package_tag(layer) {
        return Err(invalid(format!(
            "Nameplate {container} has no {} image",
            part.label().to_lowercase()
        )));
    }
    let layer_payload = manager
        .read_tag(layer)
        .map_err(|error| invalid(format!("Could not read nameplate layer {layer}: {error}")))?;
    let header = crate::icon_edit::texture_reference_offsets(&layer_payload, layer)?
        .first()
        .map(|(_, header)| *header)
        .ok_or_else(|| invalid(format!("Nameplate layer {layer} has no texture")))?;
    crate::icon_edit::decode_texture(manager, header).map_err(invalid)
}

/// Paints `image` into one texture's RGBA8 pixels, scaled to cover the texture.
fn paint(
    image: &EmbeddedImage,
    pixels: &mut [u8],
    width: usize,
    height: usize,
) -> AuthoringResult<()> {
    let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
        return Err(invalid("Nameplate texture size does not fit 32 bits"));
    };
    let covered = crate::image_import::cover(image.pixels(), width, height);
    if covered.as_raw().len() != pixels.len() {
        return Err(validation(
            "A nameplate picture does not match its texture's size",
        ));
    }
    pixels.copy_from_slice(covered.as_raw());
    Ok(())
}

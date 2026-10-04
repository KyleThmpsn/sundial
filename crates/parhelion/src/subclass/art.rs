//! A subclass's screen art. Its item strings name a second icon container at +0x82, whose one
//! layer lane names three 1080 by 1080 character pictures, one per attunement in the order of
//! `AttunementPath::index`: top, bottom, middle. Every stock subclass names one picture twice,
//! for the top and bottom attunements that share a Super, and another for the middle one
//! (Arcstrider's staff, then Whirlwind Guard), so the subclass screen most likely picks the
//! selected attunement's. That reading comes from the stock layers and is untested in game.
//!
//! A recipe keeps its base's pictures, takes either one from another subclass, or brings a
//! picture of its own. The build then gives the subclass a container of its own with a private
//! copy of the layer.
use crate::HexHash;
use crate::image_import::EmbeddedImage;
use crate::shared_tag_memory::SharedTagDependencies;
use crate::tag_payload::{read_u32, write_u32};
use crate::{
    AuthoringResult, NewTagReferenceOverride, NewTagSpec, NewTagStorageMode,
    error::{invalid, validation},
};
use serde::{Deserialize, Serialize};
use sundial::package_authoring::PackageManager;
use sundial::package_authoring::icon_schema::ICON_PRIMARY_LAYER_OFFSET;
use tiger_pkg::TagHash;

/// The container's fingerprint, which an authored container sets from what it shows.
const FINGERPRINT_OFFSET: usize = 0x10;
/// The layer's pictures among its texture references, one per attunement.
const PICTURES: usize = 3;

/// One of a subclass's screen pictures, by the attunement it shows for.
#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ArtPart {
    #[default]
    Top,
    Bottom,
    Middle,
}

impl ArtPart {
    pub const ALL: [Self; 3] = [Self::Top, Self::Bottom, Self::Middle];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Top => "Top",
            Self::Bottom => "Bottom",
            Self::Middle => "Middle",
        }
    }

    /// The picture's place among the layer's texture references, as the attunement's index.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Top => 0,
            Self::Bottom => 1,
            Self::Middle => 2,
        }
    }

    /// The size the stock pictures have. A picture of the recipe's own is built at whatever size
    /// the base's own has.
    pub const SIZE: (u32, u32) = (1080, 1080);
}

/// Where one screen picture comes from, when it is not the base subclass's.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArtImage {
    /// One picture of another subclass.
    Subclass { item_hash: HexHash, part: ArtPart },
    /// A picture of the recipe's own, scaled to cover the picture and cropped at its edges.
    Image { image: EmbeddedImage },
}

/// A subclass's screen pictures. Each one left out keeps the base subclass's.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScreenArt {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<ArtImage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom: Option<ArtImage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub middle: Option<ArtImage>,
}

impl ScreenArt {
    #[must_use]
    pub const fn part(&self, part: ArtPart) -> Option<&ArtImage> {
        match part {
            ArtPart::Top => self.top.as_ref(),
            ArtPart::Bottom => self.bottom.as_ref(),
            ArtPart::Middle => self.middle.as_ref(),
        }
    }

    pub fn set(&mut self, part: ArtPart, image: Option<ArtImage>) {
        *match part {
            ArtPart::Top => &mut self.top,
            ArtPart::Bottom => &mut self.bottom,
            ArtPart::Middle => &mut self.middle,
        } = image;
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        ArtPart::ALL
            .into_iter()
            .all(|part| self.part(part).is_none())
    }

    /// Every subclass a picture names must be a hash.
    pub(crate) fn validate(&self) -> Result<(), String> {
        for part in ArtPart::ALL {
            if let Some(ArtImage::Subclass { item_hash, .. }) = self.part(part) {
                item_hash
                    .parse_u32()
                    .map_err(|error| format!("{} subclass: {error}", part.label()))?;
            }
        }
        Ok(())
    }
}

/// One picture as the build takes it.
#[derive(Clone)]
enum Source {
    Base,
    /// Another subclass's texture, kept as it is.
    Texture(TagHash),
    /// A picture of the recipe's own, painted over a copy of the base's texture.
    Image(EmbeddedImage),
}

/// A recipe's screen art read against the packages: the base's container and its icon row,
/// which the authored ones copy, its layer, and where each picture comes from.
#[derive(Clone)]
pub(crate) struct ResolvedArt {
    pub(crate) template_row: u16,
    template: TagHash,
    layer: TagHash,
    sources: Vec<Source>,
}

/// The layer of a subclass's art container and its pictures' texture headers, by attunement.
fn pictures(
    manager: &PackageManager,
    container: TagHash,
) -> AuthoringResult<(TagHash, Vec<TagHash>)> {
    let payload = crate::icon_edit::read_icon_container(manager, container)?;
    let layer = TagHash(read_u32(&payload, ICON_PRIMARY_LAYER_OFFSET)?);
    let layer_payload = manager
        .read_tag(layer)
        .map_err(|error| invalid(format!("Could not read screen art layer {layer}: {error}")))?;
    let headers = crate::icon_edit::texture_reference_offsets(&layer_payload, layer)?
        .into_iter()
        .map(|(_, header)| header)
        .collect::<Vec<_>>();
    if headers.len() != PICTURES {
        return Err(invalid(format!(
            "Screen art {container} holds {} pictures, not {PICTURES}",
            headers.len()
        )));
    }
    Ok((layer, headers))
}

/// Reads a recipe's screen art against the packages. `base` is the base subclass's art row and
/// container. `container_of` gives another subclass's art container and refuses an item that is
/// not a subclass.
pub(crate) fn resolve(
    manager: &PackageManager,
    base: (u16, TagHash),
    art: &ScreenArt,
    container_of: &dyn Fn(u32) -> AuthoringResult<TagHash>,
) -> AuthoringResult<ResolvedArt> {
    let (template_row, template) = base;
    let (layer, _) = pictures(manager, template)?;
    let sources = ArtPart::ALL
        .into_iter()
        .map(|part| {
            Ok(match art.part(part) {
                None => Source::Base,
                Some(ArtImage::Subclass {
                    item_hash,
                    part: taken,
                }) => {
                    let hash = item_hash
                        .parse_u32()
                        .map_err(|error| invalid(error.to_string()))?;
                    let (_, headers) = pictures(manager, container_of(hash)?)?;
                    Source::Texture(headers[taken.index()])
                }
                Some(ArtImage::Image { image }) => Source::Image(image.clone()),
            })
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    Ok(ResolvedArt {
        template_row,
        template,
        layer,
        sources,
    })
}

/// Builds the authored art container, its private layer and repainted pictures, and its
/// companion into `nodes`, and returns the container.
pub(crate) fn author(
    manager: &PackageManager,
    art: &ResolvedArt,
    nodes: &mut Vec<NewTagSpec>,
    references: &mut Vec<NewTagReferenceOverride>,
) -> AuthoringResult<TagHash> {
    let package = crate::package_profile::PARHELION_ASSET_PACKAGE_ID;
    let allocator = crate::appended_tags::AppendedTagAllocator::new(package, 0);
    let painters = art
        .sources
        .iter()
        .map(|source| match source {
            Source::Image(image) => Some(
                move |pixels: &mut [u8], width: usize, height: usize| -> AuthoringResult<()> {
                    paint(image, pixels, width, height)
                },
            ),
            _ => None,
        })
        .collect::<Vec<_>>();
    let (plan, stock) = crate::icon_edit::build_layer_texture_plan(
        manager,
        package,
        0,
        nodes.len(),
        (art.layer, art.template),
        &|index| match (art.sources.get(index), painters.get(index)) {
            (Some(Source::Texture(header)), _) => crate::icon_edit::TextureChange::Use(*header),
            (Some(Source::Image(_)), Some(Some(painter))) => {
                crate::icon_edit::TextureChange::Paint {
                    group: index,
                    paint: painter,
                }
            }
            _ => crate::icon_edit::TextureChange::Keep,
        },
    )?;
    // The painted pixels and the textures taken, so any change refreshes the container.
    let mut revision = plan
        .new_tags
        .iter()
        .flat_map(|node| node.payload.iter().copied())
        .collect::<Vec<_>>();
    revision.extend(stock.iter().flat_map(|tag| tag.to_le_bytes()));
    let mut dependencies = SharedTagDependencies::new();
    dependencies.extend(plan.dependencies);
    dependencies.extend(stock);
    nodes.extend(plan.new_tags);
    references.extend(plan.reference_overrides);
    let mut container = crate::icon_edit::read_icon_container(manager, art.template)?;
    write_u32(&mut container, ICON_PRIMARY_LAYER_OFFSET, plan.layer_tag.0)?;
    let fingerprint = crate::watermark::private_icon_fingerprint(&container, &revision);
    write_u32(&mut container, FINGERPRINT_OFFSET, fingerprint)?;
    let private_container =
        allocator.assigned_tag(nodes.len(), "Screen art definition", "subclass screen art")?;
    nodes.push(NewTagSpec {
        template_tag: art.template,
        payload: container,
        storage: NewTagStorageMode::InheritTemplate,
    });
    let companion_tag =
        allocator.assigned_tag(nodes.len(), "Screen art companion", "subclass screen art")?;
    dependencies.extend([private_container.0, companion_tag.0]);
    let donor = crate::shared_tag_memory::read_and_validate_icon_companion(manager, art.template)?;
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

/// One picture of a subclass's art container at its own size, as the page and an export show it.
pub(crate) fn picture_pixels(
    manager: &PackageManager,
    container: TagHash,
    part: ArtPart,
) -> AuthoringResult<image::RgbaImage> {
    let (_, headers) = pictures(manager, container)?;
    crate::icon_edit::decode_texture(manager, headers[part.index()]).map_err(invalid)
}

/// Paints `image` into one texture's RGBA8 pixels, scaled to cover the texture.
fn paint(
    image: &EmbeddedImage,
    pixels: &mut [u8],
    width: usize,
    height: usize,
) -> AuthoringResult<()> {
    let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
        return Err(invalid("Screen art texture size does not fit 32 bits"));
    };
    let covered = crate::image_import::cover(image.pixels(), width, height);
    if covered.as_raw().len() != pixels.len() {
        return Err(validation(
            "A screen art picture does not match its texture's size",
        ));
    }
    pixels.copy_from_slice(covered.as_raw());
    Ok(())
}

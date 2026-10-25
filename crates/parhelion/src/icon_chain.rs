//! Copies of a stock seven-tag icon chain with new pixels. The chain is an icon container naming
//! one layer, the layer naming two straight RGBA8 texture headers, each header and its data entry
//! naming each other, and the container's companion listing all seven. A copy keeps every donor
//! byte except the links between its own tags and the pixels it is given. The Sunrise badge copies
//! the Lunar badge's chain, and a Crucible map's private match picture copies its donor map's.
use sundial::package_authoring::PackageManager;
use sundial::package_authoring::icon_schema::{ICON_DEFINITION_SIZE, ICON_PRIMARY_LAYER_OFFSET};
use tiger_pkg::TagHash;

use crate::{
    AuthoringResult, NewTagReference, NewTagReferenceOverride, NewTagSpec, NewTagStorageMode,
    appended_tags::AppendedTagAllocator,
    error::{input as invalid, validation},
    payload_guards::{
        STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE, donor_mutation_is_limited_to,
        is_stock_straight_rgba8_texture_header,
    },
    shared_tag_memory::{
        IconDefinitionCompanion, build_shared_tag_companion_payload, dependency_set,
        read_and_validate_icon_companion,
    },
    tag_payload::{read_u32, write_u32},
};

/// The tags a chain copy appends, in this order.
pub(crate) const TAGS: usize = 7;
const LAYER_CLASS: u32 = 0x8080_4A69;
const CONTAINER_CLASS: u32 = 0x8080_4A53;

/// One of a donor chain's textures: its data and header entries, its size, and where the layer
/// names its header.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Texture {
    pub data: TagHash,
    pub header: TagHash,
    pub width: u32,
    pub height: u32,
    pub layer_field: usize,
}

impl Texture {
    pub(crate) fn data_size(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }
}

/// A stock icon chain to copy: its low and high resolution textures, its layer with the words
/// its layout must hold besides the header tags, and its container and companion. `label` names
/// it in errors.
#[derive(Clone, Debug)]
pub(crate) struct Donor {
    pub label: String,
    pub textures: [Texture; 2],
    pub layer: TagHash,
    pub layer_size: usize,
    pub layer_words: Vec<(usize, u32)>,
    pub container: TagHash,
    pub companion: TagHash,
}

/// A donor chain's payloads, read and checked.
pub(crate) struct Chain {
    pub data: [Vec<u8>; 2],
    headers: [Vec<u8>; 2],
    layer: Vec<u8>,
    container: Vec<u8>,
    companion: IconDefinitionCompanion,
}

/// Absolute ordinals in the complete appended-tag slice passed to the extended-overlay writer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ordinals {
    pub low_data: usize,
    pub low_header: usize,
    pub high_data: usize,
    pub high_header: usize,
    pub layer: usize,
    pub container: usize,
    pub companion: usize,
}

/// The destination-package tags a copy's seven entries take.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tags {
    pub low_data: TagHash,
    pub low_header: TagHash,
    pub high_data: TagHash,
    pub high_header: TagHash,
    pub layer: TagHash,
    pub container: TagHash,
    pub companion: TagHash,
}

/// A complete append plan for one copied chain.
///
/// `new_tags` is always ordered low data and header, high data and header, layer, then an adjacent
/// container and companion pair. The reference overrides use absolute appended ordinals, so the
/// two texture data and header pairs remain reciprocal after this plan is inserted after other
/// authored tags.
#[derive(Clone, Debug)]
pub struct Plan {
    pub new_tags: Vec<NewTagSpec>,
    pub reference_overrides: Vec<NewTagReferenceOverride>,
    #[cfg(test)]
    pub ordinals: Ordinals,
    #[cfg(test)]
    pub tags: Tags,
    pub container_tag: TagHash,
}

/// Index of the container in a plan's `new_tags`, and of the high resolution data.
const CONTAINER_INDEX: usize = 5;
const HIGH_DATA_INDEX: usize = 2;
/// The container's content fingerprint, which every private icon container carries.
const FINGERPRINT: usize = 0x10;

impl Plan {
    /// Marks the copy's container with the fingerprint of its container and high resolution
    /// pixels, as every private icon container is marked.
    pub(crate) fn fingerprint(&mut self) -> AuthoringResult<()> {
        let fingerprint = crate::watermark::private_icon_fingerprint(
            &self.new_tags[CONTAINER_INDEX].payload,
            &self.new_tags[HIGH_DATA_INDEX].payload,
        );
        write_u32(
            &mut self.new_tags[CONTAINER_INDEX].payload,
            FINGERPRINT,
            fingerprint,
        )
    }
}

impl Donor {
    /// Reads and checks the donor chain.
    pub(crate) fn read(&self, manager: &PackageManager) -> AuthoringResult<Chain> {
        let mut data = Vec::with_capacity(2);
        let mut headers = Vec::with_capacity(2);
        for texture in &self.textures {
            data.push(self.read_tag(manager, texture.data, texture.data_size(), 0x28, 0x01)?);
            let header = self.read_tag(
                manager,
                texture.header,
                STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE,
                0x20,
                0x01,
            )?;
            self.reference(manager, texture.data, texture.header)?;
            self.reference(manager, texture.header, texture.data)?;
            if !is_stock_straight_rgba8_texture_header(
                &header,
                texture.width,
                texture.height,
                texture.data_size(),
            ) {
                return Err(invalid(format!(
                    "{} texture header {} no longer describes a {} by {} straight-RGBA texture",
                    self.label, texture.header, texture.width, texture.height
                )));
            }
            headers.push(header);
        }
        let layer = self.read_tag(manager, self.layer, self.layer_size, 0x08, 0x00)?;
        let container = self.read_tag(manager, self.container, ICON_DEFINITION_SIZE, 0x10, 0x00)?;
        self.reference(manager, self.layer, TagHash(LAYER_CLASS))?;
        self.reference(manager, self.container, TagHash(CONTAINER_CLASS))?;
        let companion = read_and_validate_icon_companion(manager, self.container)?;
        let mut layout = self.layer_words.iter().copied().chain(
            self.textures
                .iter()
                .map(|texture| (texture.layer_field, u32::from(texture.header))),
        );
        if layer.len() != self.layer_size
            || layout.any(|(offset, value)| read_u32(&layer, offset).ok() != Some(value))
        {
            return Err(invalid(format!(
                "{} layer {} no longer has its expected texture layout",
                self.label, self.layer
            )));
        }
        if read_u32(&container, 0)? != ICON_DEFINITION_SIZE as u32
            || read_u32(&container, ICON_PRIMARY_LAYER_OFFSET)? != u32::from(self.layer)
            || (0x18..=0x28)
                .step_by(4)
                .any(|offset| read_u32(&container, offset).ok() != Some(u32::MAX))
        {
            return Err(invalid(format!(
                "{} container {} no longer has one required layer and no optional layers",
                self.label, self.container
            )));
        }
        let expected = dependency_set(self.tags());
        if companion.tag != self.companion || companion.dependencies != expected {
            return Err(invalid(format!(
                "{} companion {} no longer lists exactly its seven-tag chain",
                self.label, companion.tag
            )));
        }
        let two = |_| validation("An icon chain has two textures");
        Ok(Chain {
            data: data.try_into().map_err(two)?,
            headers: headers.try_into().map_err(two)?,
            layer,
            container,
            companion,
        })
    }

    /// The donor's seven tags in a plan's order.
    fn tags(&self) -> [TagHash; TAGS] {
        let [low, high] = self.textures;
        [
            low.data,
            low.header,
            high.data,
            high.header,
            self.layer,
            self.container,
            self.companion,
        ]
    }

    /// A copy of `chain`, the chain read from this donor, holding `pixels` for its low and high
    /// textures, as entries of package `package_id` holding `entry_count` entries before the
    /// append, from appended ordinal `ordinal_base`.
    pub(crate) fn plan(
        &self,
        chain: &Chain,
        pixels: [Vec<u8>; 2],
        package_id: u16,
        entry_count: usize,
        ordinal_base: usize,
    ) -> AuthoringResult<Plan> {
        for (texture, data) in self.textures.iter().zip(&pixels) {
            if data.len() != texture.data_size() {
                return Err(validation(format!(
                    "{} copy has {} bytes for a {} by {} texture",
                    self.label,
                    data.len(),
                    texture.width,
                    texture.height
                )));
            }
        }
        let ordinals = self.ordinals(ordinal_base)?;
        let tags = self.assigned_tags(package_id, entry_count, ordinals)?;
        let [low, high] = self.textures;
        let mut layer = chain.layer.clone();
        write_u32(&mut layer, low.layer_field, u32::from(tags.low_header))?;
        write_u32(&mut layer, high.layer_field, u32::from(tags.high_header))?;
        let mut container = chain.container.clone();
        write_u32(
            &mut container,
            ICON_PRIMARY_LAYER_OFFSET,
            u32::from(tags.layer),
        )?;
        let companion = build_shared_tag_companion_payload(
            &chain.companion.template_payload,
            tags.companion,
            tags.container,
            &dependency_set([
                tags.low_data,
                tags.low_header,
                tags.high_data,
                tags.high_header,
                tags.layer,
                tags.container,
                tags.companion,
            ]),
        )?;
        self.validate_links(chain, &layer, &container, tags)?;
        let [low_pixels, high_pixels] = pixels;
        let spec = |template: TagHash, payload: Vec<u8>| NewTagSpec {
            template_tag: template,
            payload,
            storage: NewTagStorageMode::InheritTemplate,
        };
        let new_tags = vec![
            spec(low.data, low_pixels),
            spec(low.header, chain.headers[0].clone()),
            spec(high.data, high_pixels),
            spec(high.header, chain.headers[1].clone()),
            spec(self.layer, layer),
            spec(self.container, container),
            spec(self.companion, companion),
        ];
        let reference_overrides = reciprocal_reference_overrides(ordinals);
        self.validate_plan(&new_tags, &reference_overrides, ordinals, tags)?;
        Ok(Plan {
            new_tags,
            reference_overrides,
            #[cfg(test)]
            ordinals,
            #[cfg(test)]
            tags,
            container_tag: tags.container,
        })
    }

    fn read_tag(
        &self,
        manager: &PackageManager,
        tag: TagHash,
        expected_size: usize,
        expected_type: u8,
        expected_subtype: u8,
    ) -> AuthoringResult<Vec<u8>> {
        let entry = manager.get_entry(tag).ok_or_else(|| {
            invalid(format!(
                "{} donor tag {tag} has no package entry",
                self.label
            ))
        })?;
        if entry.file_type != expected_type || entry.file_subtype != expected_subtype {
            return Err(invalid(format!(
                "{} donor tag {tag} has type {:02X}/{:02X}; expected {expected_type:02X}/{expected_subtype:02X}",
                self.label, entry.file_type, entry.file_subtype
            )));
        }
        let payload = manager.read_tag(tag).map_err(|error| {
            invalid(format!(
                "Could not read {} donor tag {tag}: {error}",
                self.label
            ))
        })?;
        if payload.len() != expected_size || entry.file_size as usize != expected_size {
            return Err(invalid(format!(
                "{} donor tag {tag} is {} bytes (entry declares {}). Expected {expected_size}",
                self.label,
                payload.len(),
                entry.file_size
            )));
        }
        Ok(payload)
    }

    fn reference(
        &self,
        manager: &PackageManager,
        tag: TagHash,
        expected: TagHash,
    ) -> AuthoringResult<()> {
        let entry = manager.get_entry(tag).ok_or_else(|| {
            invalid(format!(
                "{} donor tag {tag} has no package entry",
                self.label
            ))
        })?;
        if entry.reference != u32::from(expected) {
            return Err(invalid(format!(
                "{} donor tag {tag} references {:08X}; expected {expected}",
                self.label, entry.reference
            )));
        }
        Ok(())
    }

    pub(crate) fn ordinals(&self, base: usize) -> AuthoringResult<Ordinals> {
        let ordinal =
            |local: usize| AppendedTagAllocator::checked_ordinal(base, local, &self.label);
        Ok(Ordinals {
            low_data: ordinal(0)?,
            low_header: ordinal(1)?,
            high_data: ordinal(2)?,
            high_header: ordinal(3)?,
            layer: ordinal(4)?,
            container: ordinal(CONTAINER_INDEX)?,
            companion: ordinal(6)?,
        })
    }

    pub(crate) fn assigned_tags(
        &self,
        package_id: u16,
        entry_count: usize,
        ordinals: Ordinals,
    ) -> AuthoringResult<Tags> {
        let allocator = AppendedTagAllocator::new(package_id, entry_count);
        let destination = format!("{} destination entry", self.label);
        let tag = format!("{} tag", self.label);
        let assigned =
            |ordinal: usize| allocator.assigned_tag(ordinal, destination.as_str(), tag.as_str());
        Ok(Tags {
            low_data: assigned(ordinals.low_data)?,
            low_header: assigned(ordinals.low_header)?,
            high_data: assigned(ordinals.high_data)?,
            high_header: assigned(ordinals.high_header)?,
            layer: assigned(ordinals.layer)?,
            container: assigned(ordinals.container)?,
            companion: assigned(ordinals.companion)?,
        })
    }

    /// Checks the copy's layer and container name its own tags and change no other donor byte.
    fn validate_links(
        &self,
        chain: &Chain,
        layer: &[u8],
        container: &[u8],
        tags: Tags,
    ) -> AuthoringResult<()> {
        let [low, high] = self.textures;
        if layer.len() != self.layer_size
            || container.len() != ICON_DEFINITION_SIZE
            || read_u32(layer, low.layer_field)? != u32::from(tags.low_header)
            || read_u32(layer, high.layer_field)? != u32::from(tags.high_header)
            || read_u32(container, ICON_PRIMARY_LAYER_OFFSET)? != u32::from(tags.layer)
        {
            return Err(validation(format!(
                "Emitted {} texture chain failed its payload-link validation",
                self.label
            )));
        }
        let unchanged = |donor: &[u8], emitted: &[u8], allowed: &[std::ops::Range<usize>], what| {
            if donor_mutation_is_limited_to(donor, emitted, allowed) {
                Ok(())
            } else {
                Err(validation(format!(
                    "Emitted {} {what} changed donor bytes outside its tag-link fields",
                    self.label
                )))
            }
        };
        unchanged(
            &chain.layer,
            layer,
            &[
                low.layer_field..low.layer_field + 4,
                high.layer_field..high.layer_field + 4,
            ],
            "layer",
        )?;
        let primary = ICON_PRIMARY_LAYER_OFFSET..ICON_PRIMARY_LAYER_OFFSET + 4;
        unchanged(&chain.container, container, &[primary], "container")
    }

    fn validate_plan(
        &self,
        new_tags: &[NewTagSpec],
        overrides: &[NewTagReferenceOverride],
        ordinals: Ordinals,
        tags: Tags,
    ) -> AuthoringResult<()> {
        let [low, high] = self.textures;
        let sizes = [
            low.data_size(),
            STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE,
            high.data_size(),
            STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE,
            self.layer_size,
            ICON_DEFINITION_SIZE,
        ];
        let templates = self.tags();
        let companion = new_tags.get(TAGS - 1);
        if new_tags.len() != TAGS
            || new_tags[..TAGS - 1].iter().zip(sizes).zip(templates).any(
                |((spec, size), template)| {
                    spec.payload.len() != size
                        || spec.template_tag != template
                        || spec.storage != NewTagStorageMode::InheritTemplate
                },
            )
            || companion.is_none_or(|companion| {
                companion.template_tag != self.companion
                    || companion.storage != NewTagStorageMode::InheritTemplate
                    || read_u32(&companion.payload, 0).ok().map(|v| v as usize)
                        != Some(companion.payload.len())
                    || read_u32(&companion.payload, 0x08).ok() != Some(u32::from(tags.companion))
                    || read_u32(&companion.payload, 0x0C).ok() != Some(u32::from(tags.container))
            })
        {
            return Err(validation(format!(
                "{} append plan changed tag order, templates, or payload sizes",
                self.label
            )));
        }
        if overrides != reciprocal_reference_overrides(ordinals)
            || ordinals.companion != ordinals.container + 1
            || tags.companion.pkg_id() != tags.container.pkg_id()
            || tags.companion.entry_index() != tags.container.entry_index().saturating_add(1)
            || (tags.container.entry_index() as usize) < ordinals.container
        {
            return Err(validation(format!(
                "{} append plan has an invalid reciprocal-reference layout",
                self.label
            )));
        }
        Ok(())
    }
}

fn reciprocal_reference_overrides(ordinals: Ordinals) -> Vec<NewTagReferenceOverride> {
    let pair = |from: usize, to: usize| NewTagReferenceOverride {
        new_tag_ordinal: from,
        reference: NewTagReference::Appended(to),
    };
    vec![
        pair(ordinals.low_data, ordinals.low_header),
        pair(ordinals.low_header, ordinals.low_data),
        pair(ordinals.high_data, ordinals.high_header),
        pair(ordinals.high_header, ordinals.high_data),
    ]
}

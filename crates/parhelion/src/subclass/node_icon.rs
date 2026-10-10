//! Private node icon containers preserve donor artwork and carry the authored CUI color.
use sundial::package_authoring::PackageManager;
use tiger_pkg::TagHash;

use super::authoring::NodeIcon;
use crate::{AuthoringResult, NewTagReferenceOverride, NewTagSpec, NewTagStorageMode};

pub(crate) fn author(
    manager: &PackageManager,
    icon: &NodeIcon,
    nodes: &mut Vec<NewTagSpec>,
    references: &mut Vec<NewTagReferenceOverride>,
) -> AuthoringResult<TagHash> {
    let tag = if let Some(artwork) = &icon.artwork {
        crate::icon_edit::package_icons::author(
            manager,
            artwork,
            icon.source_container,
            nodes,
            references,
        )?
    } else {
        let allocator = crate::appended_tags::AppendedTagAllocator::new(
            crate::package_profile::PARHELION_ASSET_PACKAGE_ID,
            0,
        );
        let tag = allocator.assigned_tag(nodes.len(), "Node icon", "ability color")?;
        let container = crate::icon_edit::read_icon_container(manager, icon.source_container)?;
        let donor = crate::shared_tag_memory::read_and_validate_icon_companion(
            manager,
            icon.source_container,
        )?;
        let companion = crate::shared_tag_memory::adjacent_companion_tag(tag)?;
        let mut dependencies = donor.dependencies;
        dependencies.remove(&icon.source_container.0);
        dependencies.remove(&donor.tag.0);
        dependencies.extend([tag.0, companion.0]);
        let payload = crate::shared_tag_memory::build_shared_tag_companion_payload(
            &donor.template_payload,
            companion,
            tag,
            &dependencies,
        )?;
        nodes.push(NewTagSpec {
            template_tag: icon.source_container,
            payload: container,
            storage: NewTagStorageMode::InheritTemplate,
        });
        nodes.push(NewTagSpec {
            template_tag: donor.tag,
            payload,
            storage: NewTagStorageMode::InheritTemplate,
        });
        tag
    };
    if let Some(rgb) = icon.color {
        let container = &mut nodes[usize::from(tag.entry_index())].payload;
        for (channel, value) in rgb
            .map(|v| (f32::from(v) / 255.0).powf(2.2))
            .into_iter()
            .chain([1.0])
            .enumerate()
        {
            sundial::package_authoring::native_payload::write_bytes(
                container,
                0x30 + channel * 4,
                &value.to_le_bytes(),
            )
            .map_err(crate::error::invalid)?;
        }
        // The native adapter exposes field 7 as the color-presence mask, independent of alpha.
        container[0x70] |= 1;
        let fingerprint = crate::watermark::private_icon_fingerprint(container, &rgb);
        crate::tag_payload::write_u32(container, 0x10, fingerprint)?;
    }
    Ok(tag)
}

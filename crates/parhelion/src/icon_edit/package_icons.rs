//! Installed 2D artwork and private, size-correct glyph layers.
use crate::artwork_browser::Purpose;
use crate::tag_payload::{read_u16 as checked_u16, read_u32 as checked_u32};
use crate::{AuthoringResult, NewTagSpec, NewTagStorageMode, error::invalid};
use tiger_pkg::{PackageManager, TagHash};
fn read_u16(data: &[u8], offset: usize) -> Result<u16, String> {
    checked_u16(data, offset).map_err(|e| e.to_string())
}
fn read_u32(data: &[u8], offset: usize) -> Result<u32, String> {
    checked_u32(data, offset).map_err(|e| e.to_string())
}

pub(crate) struct Entry {
    pub tag: TagHash,
    pub package: String,
    pub name: String,
    pub white: bool,
    pub size: [usize; 2],
    pub thumbnail: egui::ColorImage,
}

fn dimensions(header: &[u8], purpose: Purpose) -> Result<[usize; 2], String> {
    let width = usize::from(read_u16(header, 14)?);
    let height = usize::from(read_u16(header, 16)?);
    if !purpose.accepts_size(width as u32, height as u32)
        || width > 1024
        || height > 1024
        || (purpose == Purpose::Perk
            && [width, height] != [crate::artwork_browser::perk_quality::EDGE as usize; 2])
        || read_u16(header, 18)? != 1
        || read_u16(header, 20)? != 1
        || !matches!(read_u32(header, 4)?, 28 | 29 | 71 | 72)
        || read_u32(header, 36)? != u32::MAX
    {
        return Err(if purpose == Purpose::Perk {
            "Choose a native 96 × 96 perk icon."
        } else {
            "Choose a supported single 2D artwork texture between 16 and 1024 pixels per side."
        }
        .into());
    }
    Ok([width, height])
}

fn visible_transparency(image: &egui::ColorImage) -> bool {
    image.pixels.iter().any(|p| p.a() < 255) && image.pixels.iter().any(|p| p.a() > 0)
}

pub(crate) fn load(manager: &PackageManager, tag: TagHash) -> Result<egui::ColorImage, String> {
    load_for(manager, tag, Purpose::Perk)
}

pub(crate) fn load_for(
    manager: &PackageManager,
    tag: TagHash,
    purpose: Purpose,
) -> Result<egui::ColorImage, String> {
    if (crate::package_profile::MIN_AUTHORED_STANDALONE_PACKAGE_ID
        ..=crate::package_profile::MAX_AUTHORED_STANDALONE_PACKAGE_ID)
        .contains(&tag.pkg_id())
        || !crate::package_profile::is_stock_item_definition(tag.0)
    {
        return Err("Choose an icon from an installed stock package.".into());
    }
    let entry = manager.get_entry(tag).ok_or("Icon texture is missing")?;
    if entry.file_type != 32 || entry.file_subtype != 1 || entry.file_size != 40 {
        return Err("Icon reference is not a supported 2D texture header".into());
    }
    let header = manager.read_tag(tag).map_err(|e| e.to_string())?;
    dimensions(&header, purpose)?;
    let data = manager
        .get_entry(TagHash(entry.reference))
        .ok_or("Icon pixel resource is missing")?;
    if data.file_type != 40 || data.file_subtype != 1 || data.file_size > 8 * 1024 * 1024 {
        return Err("Icon pixel resource has an unsupported type or size".into());
    }
    let image = super::render_texture_preview(manager, tag)?;
    if (purpose.transparent() && !visible_transparency(&image))
        || !image.pixels.iter().any(|p| p.a() > 0)
    {
        return Err("Choose an icon with visible artwork and transparency.".into());
    }
    Ok(image)
}

/// Returning false cancels the scan and releases the package manager promptly.
#[cfg(test)]
pub(crate) fn scan(
    manager: &PackageManager,
    emit: impl FnMut(usize, usize, Option<Entry>) -> bool,
) {
    scan_for(manager, Purpose::Perk, emit);
}

pub(crate) fn scan_for(
    manager: &PackageManager,
    purpose: Purpose,
    mut emit: impl FnMut(usize, usize, Option<Entry>) -> bool,
) {
    // A texture can have several native names, including names on its pixel resource.
    let mut names = std::collections::BTreeMap::<u32, Vec<&str>>::new();
    for entry in &manager.lookup.named_tags {
        names.entry(entry.hash.0).or_default().push(&entry.name);
    }
    let mut tags = manager.get_all_by_type(32, Some(1));
    tags.sort_by_key(|(tag, _)| {
        let name = manager
            .package_paths
            .get(&tag.pkg_id())
            .map(|p| p.name.as_str())
            .unwrap_or("");
        (!name.contains("investment") && !name.contains("nux"), tag.0)
    });
    let total = tags.len();
    for (index, (tag, header)) in tags.into_iter().enumerate() {
        let entry = load_for(manager, tag, purpose)
            .ok()
            .and_then(|image| {
                let white = crate::artwork_browser::perk_quality::inspect(
                    image.pixels.iter().map(|p| p.to_srgba_unmultiplied()),
                );
                if purpose == Purpose::Perk && white.is_none() {
                    return None;
                }
                Some((image, white == Some(true)))
            })
            .map(|(image, white)| {
                let [w, h] = image.size;
                let size = [64 * w / w.max(h), 64 * h / w.max(h)];
                let mut thumbnail = egui::ColorImage::new(size, egui::Color32::TRANSPARENT);
                for y in 0..size[1] {
                    for x in 0..size[0] {
                        thumbnail[(x, y)] = image[(x * w / size[0], y * h / size[1])];
                    }
                }
                let mut aliases = names.get(&tag.0).cloned().unwrap_or_default();
                aliases.extend(names.get(&header.reference).into_iter().flatten().copied());
                aliases.sort_unstable();
                aliases.dedup();
                Entry {
                    tag,
                    package: manager
                        .package_paths
                        .get(&tag.pkg_id())
                        .map(|p| p.filename.clone())
                        .unwrap_or_default(),
                    name: aliases.join("\n"),
                    white,
                    size: image.size,
                    thumbnail,
                }
            });
        if !emit(index + 1, total, entry) {
            break;
        }
    }
}

pub(crate) fn primary_texture(
    manager: &PackageManager,
    container: TagHash,
) -> Result<TagHash, String> {
    let layer =
        super::authoring::read_primary_layer_tag(manager, container).map_err(|e| e.to_string())?;
    let payload =
        super::authoring::read_icon_layer(manager, layer, container).map_err(|e| e.to_string())?;
    super::texture_reference_offsets(&payload, layer)
        .map_err(|e| e.to_string())?
        .first()
        .map(|(_, tag)| *tag)
        .ok_or_else(|| "This perk has no icon texture".into())
}

pub(crate) fn author(
    manager: &PackageManager,
    icon: &crate::perk::Icon,
    template: TagHash,
    nodes: &mut Vec<NewTagSpec>,
    references: &mut Vec<crate::NewTagReferenceOverride>,
) -> AuthoringResult<TagHash> {
    let allocator = crate::appended_tags::AppendedTagAllocator::new(
        crate::package_profile::PARHELION_ASSET_PACKAGE_ID,
        0,
    );
    let pixels = match icon {
        crate::perk::Icon::Texture { tag } => {
            let texture = TagHash(
                tag.parse_u32()
                    .map_err(|error| invalid(error.to_string()))?,
            );
            let image =
                load(manager, texture).map_err(|e| invalid(format!("Perk icon {texture}: {e}")))?;
            // Stock glyphs already include their authored margins. Decode without rescaling
            // them, then reproduce each destination lane at its own native dimensions.
            image::RgbaImage::from_fn(image.size[0] as u32, image.size[1] as u32, |x, y| {
                image::Rgba(image[(x as usize, y as usize)].to_srgba_unmultiplied())
            })
        }
        crate::perk::Icon::Image { image, .. } => super::glyph::fit(&image.fit_to(96, 96)),
    };
    let edit = super::WeaponIconEdit {
        imported_image: Some(super::ImportedIcon::from_normalized(pixels).map_err(invalid)?),
        ..Default::default()
    };
    // A Trait glyph has separate 96x96 and 40x40 lanes. Replacing all lane references
    // with one 96x96 native texture loses the small version. The shared planner authors
    // an independent texture/header pair for every donor size, for either icon source.
    let plan = super::build_weapon_icon_edit_plan(
        manager,
        crate::package_profile::PARHELION_ASSET_PACKAGE_ID,
        0,
        nodes.len(),
        template,
        &edit,
    )?
    .ok_or_else(|| invalid("Perk icon produced no artwork"))?;
    let private_layer = plan.primary_layer_tag;
    // Fingerprint the emitted pixels as well as the references. Padding or resampling
    // fixes must invalidate a cached icon even when the recipe and tag positions match.
    let revision = plan
        .new_tags
        .iter()
        .flat_map(|node| node.payload.iter().copied())
        .collect::<Vec<_>>();
    let mut dependencies = plan.dependencies;
    nodes.extend(plan.new_tags);
    references.extend(plan.reference_overrides);
    let private_container =
        allocator.assigned_tag(nodes.len(), "Perk icon definition", "private perk")?;
    let mut container = manager
        .read_tag(template)
        .map_err(|e| invalid(e.to_string()))?;
    for offset in sundial::package_authoring::icon_schema::ICON_LAYER_REFERENCE_OFFSETS {
        if offset == sundial::package_authoring::icon_schema::ICON_PRIMARY_LAYER_OFFSET {
            crate::tag_payload::write_u32(&mut container, offset, private_layer.0)?;
        } else {
            // Only the glyph changes. Preserve any donor background, watermark, or
            // foreground and retain its complete resource closure in the companion.
            let layer = checked_u32(&container, offset)?;
            if layer != u32::MAX {
                dependencies.extend(crate::watermark::validate_icon_layer(
                    manager,
                    TagHash(layer),
                    template,
                    offset,
                )?);
            }
        }
    }
    let fingerprint = crate::watermark::private_icon_fingerprint(&container, &revision);
    crate::tag_payload::write_u32(&mut container, 0x10, fingerprint)?;
    nodes.push(NewTagSpec {
        template_tag: template,
        payload: container,
        storage: NewTagStorageMode::InheritTemplate,
    });
    let companion_tag =
        allocator.assigned_tag(nodes.len(), "Perk icon companion", "private perk")?;
    dependencies.extend([private_layer.0, private_container.0, companion_tag.0]);
    let donor = crate::shared_tag_memory::read_and_validate_icon_companion(manager, template)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn excludes_blank_opaque_and_non_icon_textures() {
        let mut header = vec![0; 40];
        header[4..8].copy_from_slice(&28u32.to_le_bytes());
        header[14..22].copy_from_slice(&[96, 0, 96, 0, 1, 0, 1, 0]);
        header[36..40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(dimensions(&header, Purpose::Perk).unwrap(), [96, 96]);
        for edge in [32u16, 64, 128, 256, 512] {
            let mut other = header.clone();
            other[14..16].copy_from_slice(&edge.to_le_bytes());
            other[16..18].copy_from_slice(&edge.to_le_bytes());
            assert!(dimensions(&other, Purpose::Perk).is_err());
            assert!(dimensions(&other, Purpose::Badge).is_ok());
        }
        header[16..18].copy_from_slice(&16u16.to_le_bytes());
        assert!(dimensions(&header, Purpose::Perk).is_err());
        assert!(!visible_transparency(&egui::ColorImage::new(
            [2, 2],
            egui::Color32::TRANSPARENT
        )));
        assert!(!visible_transparency(&egui::ColorImage::new(
            [2, 2],
            egui::Color32::WHITE
        )));
        let mut icon = egui::ColorImage::new([2, 2], egui::Color32::TRANSPARENT);
        icon[(1, 1)] = egui::Color32::WHITE;
        assert!(visible_transparency(&icon));
    }

    #[test]
    #[ignore = "requires PARHELION_SOCKET_TEST_PACKAGES, writes a temporary artwork contact sheet"]
    fn native_perk_gallery() {
        let packages = std::env::var_os("PARHELION_SOCKET_TEST_PACKAGES").unwrap();
        let manager = sundial::package_authoring::open_shadowkeep_package_manager(
            std::path::Path::new(&packages),
        )
        .unwrap();
        let mut sheet = image::RgbaImage::from_pixel(800, 640, image::Rgba([35, 35, 35, 255]));
        let mut count = 0;
        scan(&manager, |_, _, entry| {
            if let Some(entry) = entry.filter(|entry| entry.white) {
                assert_eq!(entry.size, [96, 96]);
                for y in 0..64 {
                    for x in 0..64 {
                        let [r, g, b, a] = entry.thumbnail[(x, y)].to_srgba_unmultiplied();
                        let blend = |v| {
                            ((u32::from(v) * u32::from(a) + 35 * (255 - u32::from(a))) / 255) as u8
                        };
                        sheet.put_pixel(
                            (count % 10) * 80 + x as u32 + 8,
                            (count / 10) * 80 + y as u32 + 8,
                            image::Rgba([blend(r), blend(g), blend(b), 255]),
                        );
                    }
                }
                count += 1;
            }
            count < 80
        });
        assert_eq!(count, 80);
        let path = std::env::temp_dir().join("parhelion-perk-quality.png");
        sheet.save(&path).unwrap();
        eprintln!("Inspected {count} perk candidates: {}", path.display());
    }
}

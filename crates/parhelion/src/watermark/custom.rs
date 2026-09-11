use super::*;
use crate::presentation::Artwork;

/// Keep each distinct corner treatment in its own validated native icon graph.
pub(crate) fn build_presented_watermark_plan(
    manager: &PackageManager,
    destination_package_id: u16,
    current_entry_count: usize,
    appended_ordinal_base: usize,
    requests: &[WeaponIconRequest],
    artwork: &[Option<Artwork>],
    request_context: &dyn Fn(usize) -> String,
) -> AuthoringResult<WatermarkPlan> {
    if requests.len() != artwork.len() || requests.is_empty() {
        return Err(invalid(
            "Release watermark artwork must match the weapon icon requests",
        ));
    }
    let mut groups: Vec<(Option<Artwork>, Vec<usize>)> = vec![];
    for (index, image) in artwork.iter().enumerate() {
        if let Some((_, members)) = groups.iter_mut().find(|(candidate, _)| candidate == image) {
            members.push(index);
        } else {
            groups.push((image.clone(), vec![index]));
        }
    }
    let mut result: Option<WatermarkPlan> = None;
    let mut mapped = vec![TagHash(0); requests.len()];
    for (artwork, members) in groups {
        let base = AppendedTagAllocator::checked_ordinal(
            appended_ordinal_base,
            result.as_ref().map_or(0, |plan| plan.new_tags.len()),
            "custom corner artwork",
        )?;
        let selected = members
            .iter()
            .map(|&i| requests[i].clone())
            .collect::<Vec<_>>();
        let mut plan = build_watermark_plan_with_context(
            manager,
            destination_package_id,
            current_entry_count,
            base,
            &selected,
            &|index| request_context(members[index]),
        )?;
        if let Some(artwork) = artwork {
            apply_artwork(&mut plan, &artwork, current_entry_count, base).map_err(|error| {
                let owners = members
                    .iter()
                    .map(|&index| request_context(index))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                error.context(format!("Corner Icon Used By:\n{owners}"))
            })?;
        }
        for (&member, &tag) in members.iter().zip(&plan.request_container_tags) {
            mapped[member] = tag;
        }
        if let Some(result) = &mut result {
            result.new_tags.extend(plan.new_tags);
            result.reference_overrides.extend(plan.reference_overrides);
            result.icon_container_tags.extend(plan.icon_container_tags);
            #[cfg(test)]
            result.icon_containers.extend(plan.icon_containers);
        } else {
            result = Some(plan);
        }
    }
    let mut result = result.ok_or_else(|| invalid("No release watermark artwork requests"))?;
    result.request_container_tags = mapped;
    Ok(result)
}

fn apply_artwork(
    plan: &mut WatermarkPlan,
    artwork: &Artwork,
    current_entry_count: usize,
    appended_ordinal_base: usize,
) -> AuthoringResult<()> {
    for index in 0..TEXTURE_DIMENSIONS.len() {
        let pixels = render(artwork, index)?;
        let texture = &mut plan.new_tags[index * TAGS_PER_TEXTURE].payload;
        if pixels.len() != texture.len() {
            return Err(validation(
                "Custom corner artwork changed its native texture dimensions",
            ));
        }
        *texture = pixels;
    }
    let mut revision = Sha1::new();
    for tag in &plan.new_tags[..TEXTURE_DIMENSIONS.len() * TAGS_PER_TEXTURE] {
        revision.update(&tag.payload);
    }
    let revision = revision.finalize();
    for tag in &plan.icon_container_tags {
        let index = usize::from(tag.entry_index()) - current_entry_count - appended_ordinal_base;
        let payload = &mut plan.new_tags[index].payload;
        let fingerprint = private_icon_fingerprint(payload, revision.as_slice());
        write_u32(payload, ICON_CONTENT_FINGERPRINT_OFFSET, fingerprint)?;
    }
    Ok(())
}

pub(crate) fn preview(artwork: &Artwork) -> AuthoringResult<image::RgbaImage> {
    let (width, height) = TEXTURE_DIMENSIONS[0];
    image::RgbaImage::from_raw(
        width * OUTPUT_TEXTURE_SCALE,
        height * OUTPUT_TEXTURE_SCALE,
        render(artwork, 0)?,
    )
    .ok_or_else(|| invalid("Watermark preview has unexpected dimensions"))
}

pub(crate) fn render(artwork: &Artwork, index: usize) -> AuthoringResult<Vec<u8>> {
    let scale = OUTPUT_TEXTURE_SCALE;
    let source = image::load_from_memory(AUTHORED_TEXTURE_PNGS[index])
        .map_err(|error| invalid(error.to_string()))?
        .into_rgba8();
    let (width, height) = source.dimensions();
    let (mut plate, bounds, polarity) = if matches!(index, 2 | 3) {
        let color = source.get_pixel(0, 0).0;
        (
            image::RgbaImage::from_pixel(
                width,
                height,
                image::Rgba([color[0], color[1], color[2], 0]),
            ),
            (7, 4, 37, 41),
            [color[0], color[1], color[2]],
        )
    } else {
        let (guide_index, bounds) = if matches!(index, 0 | 4) {
            (0, (5, 4, 31, 26))
        } else {
            (1, (23, 3, 49, 25))
        };
        let guide = image::load_from_memory(AUTHORED_TEXTURE_PNGS[guide_index])
            .map_err(|error| invalid(error.to_string()))?
            .into_rgba8();
        let mut plate = source;
        let background = if index < 2 {
            [0, 0, 0, 102]
        } else {
            [185, 185, 185, 255]
        };
        for y in bounds.1..=bounds.3 {
            for x in bounds.0..=bounds.2 {
                let mask = guide.get_pixel(x, y);
                if mask[3] != 0 && mask[0] != 0 {
                    plate.put_pixel(x, y, image::Rgba(background));
                }
            }
        }
        (plate, bounds, if index < 2 { [255; 3] } else { [0; 3] })
    };
    plate = upscale_texture(width, height, plate.into_raw())?;
    let mut glyph = artwork.render(
        (bounds.2 - bounds.0 + 1) * scale,
        (bounds.3 - bounds.1 + 1) * scale,
    );
    for (x, y, pixel) in glyph.enumerate_pixels_mut() {
        pixel.0[..3].copy_from_slice(&polarity);
        if !matches!(index, 2 | 3) {
            // Clip arbitrary silhouettes to the native corner plate, including its edge.
            let alpha = u32::from(plate.get_pixel(bounds.0 * scale + x, bounds.1 * scale + y)[3]);
            let background_alpha = if index < 2 { 102 } else { 255 };
            pixel[3] = (u32::from(pixel[3]) * alpha.min(background_alpha) / background_alpha) as u8;
        }
    }
    image::imageops::overlay(
        &mut plate,
        &glyph,
        i64::from(bounds.0 * scale),
        i64::from(bounds.1 * scale),
    );
    Ok(plate.into_raw())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cropped_and_positioned_watermarks_change_every_native_lane() {
        let source = image::RgbaImage::from_fn(80, 40, |x, y| {
            image::Rgba([200, 40, 90, if x > 20 && y < 30 { 255 } else { 0 }])
        });
        let original = Artwork::from_source(source).unwrap();
        let edited = original
            .with_composition(crate::presentation::composition::Composition {
                crop: [2000, 0, 8000, 8000],
                scale: 70,
                offset: [15, -10],
                ..Default::default()
            })
            .unwrap();
        for index in 0..TEXTURE_DIMENSIONS.len() {
            assert_ne!(
                render(&original, index).unwrap(),
                render(&edited, index).unwrap()
            );
        }
        assert_eq!(
            preview(&edited).unwrap().into_raw(),
            render(&edited, 0).unwrap()
        );
    }

    #[test]
    fn custom_corner_silhouettes_preserve_the_native_palette_and_plate_edges() {
        let mut png = std::io::Cursor::new(vec![]);
        image::RgbaImage::from_pixel(96, 96, image::Rgba([30, 190, 40, 255]))
            .write_to(&mut png, ImageFormat::Png)
            .unwrap();
        let artwork = Artwork::from_png(&png.into_inner()).unwrap();
        for (index, png) in AUTHORED_TEXTURE_PNGS.iter().enumerate() {
            let source = image::load_from_memory(png).unwrap().into_rgba8();
            let rendered = render(&artwork, index).unwrap();
            assert_eq!(
                rendered.len(),
                (source.width() * source.height() * OUTPUT_TEXTURE_SCALE * OUTPUT_TEXTURE_SCALE * 4)
                    as usize
            );
            if matches!(index, 2 | 3) {
                let color = &source.get_pixel(0, 0).0[..3];
                assert!(
                    rendered
                        .chunks_exact(4)
                        .filter(|pixel| pixel[3] != 0)
                        .all(|pixel| &pixel[..3] == color)
                );
            } else {
                let plate =
                    upscale_texture(source.width(), source.height(), source.into_raw()).unwrap();
                for (before, after) in plate.pixels().zip(rendered.chunks_exact(4)) {
                    if before[3] == 0 {
                        assert_eq!(after[3], 0, "Custom glyph escapes corner plate {index}");
                    }
                }
            }
        }
    }
}

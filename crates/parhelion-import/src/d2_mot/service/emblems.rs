//! Emblem artwork is native editable recipe data, with no external asset folder dependency.
use super::*;
use crate::d2_mot::{assets::item, icon, localization};
use base64::{Engine as _, engine::general_purpose::STANDARD};

fn png(width: u16, height: u16, rgba: &[u8]) -> Result<String> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, u32::from(width), u32::from(height));
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(rgba)?;
    }
    Ok(STANDARD.encode(bytes))
}

fn layer(r: &mut Reader, tag: u32) -> Result<Option<u32>> {
    if matches!(tag, 0 | u32::MAX | 0x811C9DC5) {
        return Ok(None);
    }
    let data = r.tag(tag, None)?;
    let resource = data.pointer(16)?;
    let lanes = data.array(resource, 16, None)?;
    if lanes.is_empty() {
        return Ok(None);
    }
    ensure!(
        lanes.len() == 1,
        "Nameplate layer has multiple animation lanes"
    );
    let frames = data.array(lanes[0], 4, None)?;
    if frames.is_empty() {
        return Ok(None);
    }
    ensure!(
        frames.len() == 1,
        "Animated nameplate images require a native animation converter"
    );
    Ok(Some(data.u32(frames[0])?))
}

fn template(r: &mut Reader, hash: u32) -> Result<()> {
    let tag = item::find_native(r, hash)?;
    let definition = r.tag(tag, Some(0x80807BEA))?;
    ensure!(
        definition.u8(0xB8)? == 27,
        "Native template is not an emblem"
    );
    let globals = r
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|t| t.name == "investment_globals")
        .context("Native globals")?
        .hash
        .0;
    let globals = r.tag(globals, None)?;
    let table = r.tag(globals.u32(16 + 33 * 16)?, None)?;
    let row = table
        .array(8, 24, Some(0x80805CDF))?
        .into_iter()
        .find(|&row| table.u32(row).ok() == Some(hash))
        .context("Native emblem strings")?;
    let strings = r.tag(table.u32(row + 16)?, None)?;
    let table = r.tag(globals.u32(16 + 75 * 16)?, None)?;
    let rows = table.array(8, 24, Some(0x80802957))?;
    let row = *rows
        .get(usize::from(strings.u16(0x82)?))
        .context("Native nameplate index")?;
    let container = r.tag(table.u32(row + 16)?, None)?;
    for (offset, size) in [(0x14, [474, 96]), (0x20, [96, 96]), (0x24, [1958, 146])] {
        let texture =
            layer(r, container.u32(offset)?)?.context("Native nameplate layer missing")?;
        let header = r.tag(texture, None)?;
        ensure!(
            [header.u16(14)?, header.u16(16)?] == size,
            "Native nameplate dimensions would crop source artwork"
        );
        ensure!(
            header.0.len() == 40 && header.u16(12)? == 0xCAFE && matches!(header.u32(4)?, 28 | 29),
            "Native nameplate texture cannot be edited as RGBA8"
        );
        let data_tag = r.reference(texture)?;
        let data = r.tag(data_tag, None)?;
        let bytes = usize::from(size[0]) * usize::from(size[1]) * 4;
        ensure!(
            r.reference(data_tag)? == texture
                && data.0.len() == bytes
                && header.u32(0)? as usize == bytes,
            "Native nameplate texture does not contain one editable surface"
        );
    }
    Ok(())
}

pub(super) fn prepare(
    source: &Weapon,
    modern: &Path,
    native: &Path,
    donors: &Value,
    output: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<PathBuf> {
    let output = crate::d2_mot::reader::outside(output, modern.parent().context("Modern root")?)?;
    let output = crate::d2_mot::reader::outside(&output, native.parent().context("Native root")?)?;
    let folder = reserve_assets(&output, "emblem")?;
    progress("Reading source emblem artwork...".into());
    let mut r = Reader::new(modern, &folder.join("source"), true)?;
    let (index, tag) = item::find(&mut r, source.hash)?;
    let definition = r.tag(tag, Some(0x8080799D))?;
    let strings_tag = localization::item_strings(&mut r, source.hash, index)?;
    let strings = r.tag(strings_tag, Some(0x8080549F))?;
    ensure!(
        Family::source(&definition, &strings)?.is_some_and(|(kind, _, _)| kind == Family::Emblem),
        "Source item is not an emblem"
    );
    let mut labels = localization::Resolver::default();
    let name = labels.label(&mut r, &strings, 0x80)?;
    ensure!(
        name == source.name,
        "Source emblem name changed during import"
    );
    let flavor = labels.label(&mut r, &strings, 0xA4)?;
    let inventory_container = icon::container(&mut r, strings.u32(0x78)? as usize)?;
    layer(&mut r, inventory_container.u32(0x14)?)?.context("Source inventory artwork missing")?;
    let inventory = icon::read_index(&mut r, strings.u32(0x78)? as usize)?;
    let container = icon::container(&mut r, strings.u32(0x7C)? as usize)?;
    let mut nameplate = json!({});
    for (name, offset, size) in [
        ("banner", 0x14, [474, 96]),
        ("overlay", 0x24, [96, 96]),
        ("background", 0x28, [1958, 146]),
    ] {
        let image = if let Some(texture) = layer(&mut r, container.u32(offset)?)? {
            let image = icon::read_texture(&mut r, texture)?;
            ensure!(
                [image.width, image.height] == size,
                "Source {name} dimensions are not supported natively"
            );
            png(image.width, image.height, &icon::decode(&image)?)?
        } else {
            ensure!(name != "banner", "Source emblem has no banner");
            // Explicit transparent artwork prevents a missing source layer inheriting donor art.
            png(
                size[0],
                size[1],
                &vec![0; usize::from(size[0]) * usize::from(size[1]) * 4],
            )?
        };
        nameplate[name] = json!({"source":"image","image":{"png_base64":image}});
    }
    let colors = (0..2)
        .map(|color| {
            (0..4)
                .map(|lane| container.f32(0x30 + color * 16 + lane * 4))
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    nameplate["colors"] = json!(colors);
    ensure!(
        colors.iter().flatten().all(|value| value.is_finite()),
        "Source nameplate colors are not finite"
    );
    let rarity = match definition.u8(0xA0)? {
        1 => "common",
        2 => "uncommon",
        3 => "rare",
        4 => "legendary",
        5 => "exotic",
        _ => anyhow::bail!("Unsupported source emblem rarity"),
    };
    r.finish()?;
    let mut candidates = donors["emblems"]
        .as_array()
        .context("No native emblem templates loaded")?
        .iter()
        .collect::<Vec<_>>();
    candidates.sort_by_key(|d| {
        (
            d["hash"].as_u64() != Some(u64::from(source.hash)),
            d["hash"].as_u64(),
        )
    });
    let mut r = Reader::new(native, &folder.join("native"), false)?;
    let mut selected = None;
    let mut failures = Vec::new();
    for donor in candidates {
        let hash = profile::hash(donor, "hash")?;
        match template(&mut r, hash) {
            Ok(()) => {
                selected = Some((hash, donor));
                break;
            }
            Err(error) => failures.push(json!({"donor":hash,"reason":format!("{error:#}")})),
        }
    }
    write_json(&folder.join("template-checks.json"), &json!(failures))?;
    let (hash, donor) =
        selected.context("No native emblem template has all three matching image layouts")?;
    r.finish()?;
    let namespace = crate::d2_mot::compatibility::namespace(source.hash);
    let recipe = json!({"schema":1,"kind":"emblem","collection_placement":"sunrise_badge",
        "namespace":namespace,"identity":batch::identity(&namespace)?,"name":name,"type_name":source.weapon_type,
        "donor":{"item_hash":format!("0x{hash:08X}"),"expected_name":donor["name"]},
        "flavor":flavor,"source":"Source: Imported emblem",
        "overrides":{"rarity":rarity,"nameplate":nameplate,"stat_trackers":{"mode":"all"},"icon_edit":{"imported_image":{
            "png_base64":png(inventory.width, inventory.height, &inventory.rgba)?}}}});
    let path = folder.join("emblem.parhelion.json");
    write_json(&path, &recipe)?;
    write_json(
        &output.join("result.json"),
        &json!({"recipe":path,"source_item":source.hash,
        "donor_hash":hash,"kind":"emblem","embedded_artwork":true,"gameplay_verified":false}),
    )?;
    progress("Emblem recipe prepared with embedded artwork.".into());
    Ok(path)
}

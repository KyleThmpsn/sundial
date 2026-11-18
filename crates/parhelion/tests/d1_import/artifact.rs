//! Artifact acceptance follows the source slot and the staged item's equipment and Collections.
use super::*;
mod presentation;

fn collect(value: &Value, refusals: &mut Vec<Value>, omitted: &mut Vec<Value>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                match (key.as_str(), value.as_array()) {
                    ("audio_refusals", Some(rows)) => refusals.extend(rows.iter().cloned()),
                    ("omitted", Some(rows)) => omitted.extend(rows.iter().cloned()),
                    _ => collect(value, refusals, omitted),
                }
            }
        }
        Value::Array(rows) => {
            for row in rows {
                collect(row, refusals, omitted);
            }
        }
        _ => {}
    }
}

/// A lens talent's attached graph must carry the source screen color record: the source
/// parameter vectors in the stock record's rows, and the source lookup texture when it has one.
/// The source record is read here from its tag, which the private record's symbol ends with.
fn screen_color(source: &Source, folder: &Path) -> Result<Vec<Value>> {
    let manifest: Value = serde_json::from_slice(&fs::read(folder.join("asset-graph.json"))?)?;
    let nodes = manifest["attachments"]["nodes"]
        .as_array()
        .context("Attachment nodes")?;
    let node = |symbol: &str| {
        nodes
            .iter()
            .find(|node| node["symbol"] == symbol)
            .with_context(|| format!("Attachment node {symbol}"))
    };
    let file = |node: &Value| -> Result<Vec<u8>> {
        Ok(fs::read(
            folder.join(node["file"].as_str().context("Node file")?),
        )?)
    };
    let patch = |node: &Value, offset: u64| {
        node["patches"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|patch| patch["offset"] == offset)
            .and_then(|patch| patch["symbol"].as_str())
            .map(str::to_owned)
    };
    let rows = |bytes: &[u8], count: usize| -> Result<Vec<[u8; 16]>> {
        // A parameter program's initial vectors: a count at `+60` and a relative offset at
        // `+68` to an array header of sixteen bytes.
        ensure!(
            u64::from_le_bytes(bytes[0x60..0x68].try_into()?) == count as u64,
            "Screen parameter vector count"
        );
        let start = 0x68 + usize::try_from(i64::from_le_bytes(bytes[0x68..0x70].try_into()?))? + 16;
        (0..count)
            .map(|i| Ok(bytes[start + i * 16..start + i * 16 + 16].try_into()?))
            .collect()
    };
    let mut found = Vec::new();
    for record in nodes
        .iter()
        .filter(|node| node["template"] == 0x80C0_E2D4u32)
    {
        let symbol = record["symbol"].as_str().context("Record symbol")?;
        let tag = u32::from_str_radix(&symbol[symbol.len() - 8..], 16)?;
        let original = source.read(tag)?;
        let emitted = file(record)?;
        ensure!(
            emitted.len() == 0x50 && emitted[8..0x28] == original.0[8..0x28],
            "Screen record mode and flags differ from the source"
        );
        let parameters = file(node(&patch(record, 0x40).context("Screen parameters")?)?)?;
        let theirs = rows(&source.read(original.u32(0x40)?)?.0, 12)?;
        let ours = rows(&parameters, 13)?;
        let one = 1f32.to_le_bytes();
        let inserted = [one, one, [0; 4], [0; 4]].concat();
        ensure!(
            ours[..11] == theirs[..11] && ours[11][..] == inserted[..] && ours[12] == theirs[11],
            "Screen parameter vectors differ from the source"
        );
        let lookup = original.u32(0x28)?;
        let texture = patch(record, 0x28);
        ensure!(
            texture.is_some() == (lookup != u32::MAX),
            "Screen lookup texture differs from the source"
        );
        if let Some(texture) = &texture {
            let header = node(texture)?;
            let data = node(header["reference"].as_str().context("Lookup data")?)?;
            ensure!(
                file(data)?.len() == 1024 * 32 * 4,
                "Screen lookup texture is not 1024 by 32 texels of four bytes"
            );
        }
        found.push(json!({"source":tag,"lookup":(lookup != u32::MAX).then_some(lookup)}));
    }
    Ok(found)
}

/// What each attached graph's translation kept as silent timing: cues the bank converter
/// refused and event kinds without a converter. A talent can be listed as translated while its
/// attached graph plays neither, so the receipt states both, and a case can require its sound.
fn feedback(source: &Source, recipe: &WeaponRecipe, receipt: &Value) -> Result<Value> {
    let directory = &recipe
        .overrides
        .imported_graph
        .as_ref()
        .context("Imported graph")?
        .directory;
    let mut rows = Vec::new();
    let attachments = directory.join("attachments");
    if attachments.is_dir() {
        for entry in fs::read_dir(attachments)? {
            let folder = entry?.path();
            let path = folder.join("provenance.json");
            if !path.is_file() {
                continue;
            }
            let provenance: Value = serde_json::from_slice(&fs::read(&path)?)?;
            let (mut refusals, mut omitted) = (Vec::new(), Vec::new());
            collect(&provenance, &mut refusals, &mut omitted);
            rows.push(
                json!({"attachment":folder.file_name().and_then(|name| name.to_str()),
                "audio_refusals":refusals,"omitted_events":omitted,
                "screen_color":screen_color(source, &folder)?}),
            );
        }
    }
    if receipt["case"]["require_attached_sound"] == true {
        ensure!(!rows.is_empty(), "No attached graph to carry the sound");
        for row in &rows {
            ensure!(
                row["audio_refusals"].as_array().is_some_and(Vec::is_empty)
                    && row["omitted_events"].as_array().is_some_and(|events| events
                        .iter()
                        .all(|event| event["class"] != 0x80801F00u32)),
                "Attached graph kept its sound as silent timing: {row}"
            );
        }
    }
    if receipt["case"]["require_screen_color"] == true {
        ensure!(
            rows.iter().any(|row| row["screen_color"]
                .as_array()
                .is_some_and(|records| !records.is_empty()))
                && rows.iter().all(
                    |row| row["omitted_events"].as_array().is_some_and(|events| events
                        .iter()
                        .all(|event| event["class"] != 0x8080052Fu32))
                ),
            "Lens talent did not carry its screen color record: {rows:?}"
        );
    }
    Ok(json!(rows))
}

/// The mean absolute channel difference of two images of one size, weighting color by alpha so
/// that transparent pixels compare by their coverage alone.
fn difference(a: &image::RgbaImage, b: &image::RgbaImage) -> f64 {
    let mut total = 0u64;
    for (p, q) in a.pixels().zip(b.pixels()) {
        for channel in 0..3 {
            let x = u32::from(p[channel]) * u32::from(p[3]) / 255;
            let y = u32::from(q[channel]) * u32::from(q[3]) / 255;
            total += u64::from(x.abs_diff(y));
        }
        total += u64::from(p[3].abs_diff(q[3]));
    }
    total as f64 / (f64::from(a.width()) * f64::from(a.height()) * 4.0)
}

/// A Rise of Iron artifact's strings name a second image at `+70`, which the source inspection
/// screen draws behind the item. Shadowkeep draws an item's large icon texture there: 104 stock
/// icons hold a 96-pixel tile, a 60-pixel copy and a 460-pixel picture in one lane. The recipe
/// must carry the source picture, and the staged icon's large texture must show it.
///
/// Failure modes: the picture dropped from the recipe or changed on the way, a staged icon
/// that keeps the template's single tile, a large texture painted with the enlarged tile, and
/// a tile replaced by the picture. An artifact without a second image must gain none.
fn backdrop(
    source: &Source,
    native: &mut Reader,
    globals: &Payload,
    source_strings: u32,
    recipe: &WeaponRecipe,
) -> Result<Value> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let saved = serde_json::to_value(recipe)?;
    let saved = &saved["overrides"]["icon_edit"]["backdrop_image"]["png_base64"];
    let original = source.icon_png(source.read(source_strings)?.u32(0x70)?, None)?;
    let Some(original) = original else {
        ensure!(
            saved.is_null(),
            "Recipe carries a backdrop its source lacks"
        );
        return Ok(json!({"source":false}));
    };
    let original = image::load_from_memory(&original)?.to_rgba8();
    let carried = image::load_from_memory(
        &STANDARD.decode(saved.as_str().context("Recipe lost the source backdrop")?)?,
    )?
    .to_rgba8();
    ensure!(
        carried == original,
        "Recipe backdrop differs from the source picture"
    );
    let item_hash = recipe.identity.item_hash.parse_u32()?;
    let table = native.tag(globals.u32(16 + 33 * 16)?, None)?;
    let row = table
        .array(8, 24, Some(0x80805CDF))?
        .into_iter()
        .find(|&row| table.u32(row).ok() == Some(item_hash))
        .context("Artifact strings entry")?;
    let strings = native.tag(table.u32(row + 16)?, None)?;
    let icons = native.tag(globals.u32(16 + 75 * 16)?, None)?;
    let rows = icons.array(8, 0x18, Some(0x80802957))?;
    let container = native.tag(
        icons.u32(
            *rows
                .get(usize::from(strings.u16(0x80)?))
                .context("Icon row")?
                + 0x10,
        )?,
        Some(0x80804A53),
    )?;
    let layer = native.tag(container.u32(0x14)?, Some(0x80804A69))?;
    let lanes = layer.array(0x20, 16, Some(0x80804A6C))?;
    ensure!(lanes.len() == 1, "Artifact icon layer lanes");
    let mut images = Vec::new();
    for frame in layer.array(lanes[0], 4, Some(0x80804A6F))? {
        let tag = layer.u32(frame)?;
        let header = native.tag(tag, None)?;
        let (width, height) = (u32::from(header.u16(14)?), u32::from(header.u16(16)?));
        ensure!(
            matches!(header.u32(4)?, 28 | 29),
            "Artifact icon texture format"
        );
        let pixels = native
            .manager
            .get_entry(tiger_pkg::TagHash(tag))
            .context("Artifact icon texture entry")?
            .reference;
        let pixels = native.tag(pixels, None)?;
        images.push(
            image::RgbaImage::from_raw(width, height, pixels.0.clone())
                .context("Artifact icon texture size differs from its pixels")?,
        );
    }
    let sizes = images
        .iter()
        .map(image::RgbaImage::dimensions)
        .collect::<Vec<_>>();
    ensure!(
        sizes == [(96, 96), (60, 60), (460, 460)],
        "Artifact icon textures {sizes:?} are not a tile, a small copy and a large picture"
    );
    // The source picture is square and smaller than the large texture, so it fills it scaled
    // by one factor. Compare at the source's size, where only the resampling differs.
    let resize = |image: &image::RgbaImage| {
        image::imageops::resize(
            image,
            original.width(),
            original.height(),
            image::imageops::FilterType::Triangle,
        )
    };
    ensure!(
        original.width() == original.height(),
        "Source backdrop is not square"
    );
    let (large, tile) = (
        difference(&resize(&images[2]), &original),
        difference(&resize(&images[0]), &original),
    );
    ensure!(
        large < 6.0 && large * 2.0 < tile,
        "Artifact large icon texture differs from the source backdrop by {large:.2} (the tile by {tile:.2})"
    );
    // The tile keeps the inventory icon, which the picture must not replace.
    let icon = image::load_from_memory(
        &STANDARD.decode(
            serde_json::to_value(recipe)?["overrides"]["icon_edit"]["imported_image"]["png_base64"]
                .as_str()
                .context("Recipe icon")?,
        )?,
    )?
    .to_rgba8();
    let kept = difference(&images[0], &icon);
    ensure!(
        kept < 1.0,
        "Artifact icon tile differs from the recipe icon by {kept:.2}"
    );
    Ok(
        json!({"source":true,"size":[original.width(),original.height()],
        "large_difference":large,"tile_difference":tile}),
    )
}

pub(super) fn check(
    source: &Source,
    native: &mut Reader,
    item_tag: u32,
    recipe: &WeaponRecipe,
    receipt: &Value,
) -> Result<Value> {
    let feedback = feedback(source, recipe, receipt)?;
    let source_item = source
        .artifacts()?
        .into_iter()
        .find(|item| Some(u64::from(item.hash)) == receipt["source"].as_u64())
        .context("Original artifact")?;
    let original = source.read(source_item.definition)?;
    ensure!(
        original.u8(original.pointer(0x18)? + 0x18)? == 16,
        "Source artifact slot"
    );
    ensure!(
        recipe.kind == ItemKind::Artifact,
        "Portable recipe lost its kind"
    );
    // The class must survive the portable recipe, where the editor and Collections read it.
    ensure!(
        recipe.overrides.armor_class
            == [
                parhelion::ArmorClass::Titan,
                parhelion::ArmorClass::Hunter,
                parhelion::ArmorClass::Warlock
            ]
            .get(usize::from(source_item.class_type))
            .copied(),
        "Portable recipe class differs from the source artifact"
    );
    // Stock helmets name these class unlock-flag rows in the same equipment block, and their
    // tooltips name the class after Requires.
    let class_flag = [0x10C, 0x0F3, 0x113]
        .get(usize::from(source_item.class_type))
        .copied();
    ensure!(
        class_flag.is_some() || source_item.class_type == 3,
        "Source artifact class"
    );
    let emitted = native.tag(item_tag, Some(0x80807BEA))?;
    ensure!(
        emitted.u8(0xB8)? == 49,
        "Artifact entered another inventory bucket"
    );
    let equipment = emitted.pointer(0x10)?;
    ensure!(
        emitted.u32(equipment - 4)? == 0x80807C02,
        "Equipment block class"
    );
    ensure!(
        emitted.u16(equipment + 0x18)? == 0x0112,
        "Artifact equipment slot"
    );
    ensure!(
        emitted.u16(equipment + 0x1A)? == u16::MAX,
        "Artifact equipment sentinel"
    );
    let mut conditions = Vec::new();
    if emitted.u64(equipment)? != 0 {
        for group in emitted.array(equipment, 16, Some(0x80807D2F))? {
            conditions.push(
                emitted
                    .array(group, 8, Some(0x80807D31))?
                    .into_iter()
                    .map(|token| Ok([emitted.u32(token)?, emitted.u32(token + 4)?]))
                    .collect::<Result<Vec<_>>>()?,
            );
        }
    }
    // The source class must arrive as exactly one native class-flag requirement. An
    // unrestricted source must stay free of equip conditions.
    ensure!(
        conditions
            == class_flag
                .map(|flag| vec![vec![[1, flag]]])
                .unwrap_or_default(),
        "Artifact equip conditions {conditions:X?} differ from source class {}",
        source_item.class_type
    );
    ensure!(
        emitted.u64(0x20)? == 0 && emitted.u64(0x90)? == 0,
        "Artifact retained seasonal requirements or objectives"
    );
    ensure!(emitted.u64(0x88)? == 0, "Artifact retained a model");
    // A Seasonal Artifact names its season and that season's progression, which the seasonal
    // tooltip shows in place of the item's own perks. The import must carry neither.
    ensure!(
        emitted.u8(0xA8)? == u8::MAX && emitted.u16(0xDA)? == u16::MAX,
        "Artifact retained the template's season or progression"
    );
    // The template stays on its character. The import must be free to move to the Vault.
    ensure!(
        emitted.u8(0xBC)? == 0,
        "Artifact retained the template's character-bound flag"
    );
    // Stock artifacts hold no item categories, from which the client derives an item's type.
    // The import must hold the pair every stock helmet starts and ends with.
    let categories = emitted
        .array(0xE0, 2, Some(0x80802C50))?
        .into_iter()
        .map(|row| emitted.u16(row))
        .collect::<Result<Vec<_>, _>>()?;
    ensure!(
        categories == [0, 0x1B],
        "Artifact item categories {categories:X?} are not the helmet pair"
    );
    // The inspection screen finds stats, Power and sockets on gear that declares stats as
    // armor does. A stock Legendary helmet declares stats 0, 1 and 2 with no amount and holds
    // a quality block of five arrays: its class's infusion category, three level rows and a
    // version. Failure modes: the template's seasonal stat 60 kept, a row missing or carrying
    // an amount or a program, and a quality block that is absent, another class's, or placed
    // so that an array header loses its 16-byte alignment. The item size word must cover them.
    let mut stats = emitted
        .array(emitted.pointer(0x70)?, 0x28, Some(0x80803033))?
        .into_iter()
        .map(|row| {
            Ok((
                emitted.u8(row)?,
                emitted.u32(row + 4)?,
                emitted.u64(row + 8)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    stats.sort_unstable();
    ensure!(
        stats == [(0, 0, 0), (1, 0, 0), (2, 0, 0)],
        "Artifact stat rows {stats:?} are not a helmet's"
    );
    let quality = emitted.pointer(0x48)?;
    ensure!(
        emitted.u32(quality - 4)? == 0x80807BF5 && emitted.u64(quality + 8)? == 3,
        "Artifact quality block differs from a helmet's"
    );
    let infusion = [0xD382DE85u32, 0xC4C4D0D3, 0x3E6403E6]
        .get(usize::from(source_item.class_type))
        .copied()
        .unwrap_or(0xC4C4D0D3);
    let mut levels = Vec::new();
    for (field, class, stride) in [
        (0x10, 0x80800070u32, 4usize),
        (0x28, 0x80807BFB, 2),
        (0x38, 0x80807BFB, 2),
        (0x48, 0x80807BFB, 2),
        (0x60, 0x80805921, 2),
    ] {
        let rows = emitted.array(quality + field, stride, Some(class))?;
        ensure!(
            rows.first().is_some_and(|row| row % 16 == 0),
            "Artifact quality array {field:X} is empty or misaligned"
        );
        levels.push(
            rows.into_iter()
                .map(|row| {
                    Ok(if stride == 4 {
                        emitted.u32(row)?
                    } else {
                        u32::from(emitted.u16(row)?)
                    })
                })
                .collect::<Result<Vec<_>>>()?,
        );
    }
    ensure!(
        levels[0] == [infusion]
            && levels[1] == [0x75]
            && levels[2].len() == 2
            && levels[2][0] == 0x77
            && levels[3] == [0x73]
            && levels[4].len() == 1,
        "Artifact quality rows {levels:X?} are not a Legendary helmet's of its class"
    );
    ensure!(
        usize::try_from(emitted.u64(0)?)? == emitted.0.len(),
        "Artifact definition size word differs from its payload"
    );
    ensure!(
        emitted
            .array(emitted.pointer(0x70)? + 16, 24, Some(0x808077BC))?
            .is_empty(),
        "Artifact retained direct donor perks"
    );
    let sockets = emitted.array(emitted.pointer(0x68)?, 80, Some(0x808077C4))?;
    ensure!(!sockets.is_empty(), "Artifact has no selectable talents");
    let globals = native
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|n| n.name == "investment_globals")
        .context("Globals")?
        .hash
        .0;
    let globals = native.tag(globals, None)?;
    let root = native.tag(globals.u32(16)?, None)?;
    let items = native.tag(root.u32(8 + 48 * 16)?, None)?;
    let item_hash = recipe.identity.item_hash.parse_u32()?;
    let item_index = items
        .array(8, 24, Some(0x80807BE8))?
        .iter()
        .position(|&row| items.u32(row).ok() == Some(item_hash))
        .context("Authored item row")?;
    let collectibles = native.tag(root.u32(8 + 19 * 16)?, None)?;
    let found = collectibles
        .array(8, 0xB8, Some(0x80803475))?
        .into_iter()
        .enumerate()
        .filter(|&(_, row)| collectibles.u16(row + 0x2C).ok() == Some(item_index as u16))
        .collect::<Vec<_>>();
    ensure!(
        found.len() == 1,
        "Artifact must have exactly one collectible"
    );
    let (collectible, row) = found[0];
    ensure!(
        collectibles.u32(row + 0x28)? == recipe.identity.collectible_hash.parse_u32()?,
        "Artifact collectible identity changed"
    );
    let parents = collectibles.array(row + 0x18, 2, Some(0x80803962))?;
    ensure!(!parents.is_empty(), "Artifact is absent from Collections");
    let nodes = native.tag(root.u32(8 + 63 * 16)?, None)?;
    let rows = nodes.array(8, 0xA8, Some(0x80803056))?;
    let mut pages = Vec::new();
    for parent in parents {
        let index = collectibles.u16(parent)?;
        let row = rows[usize::from(index)];
        let children = nodes.array(row + 0x78, 4, Some(0x80803068))?;
        ensure!(
            children
                .iter()
                .filter(|&&at| nodes.u16(at).ok() == Some(collectible as u16))
                .count()
                == 1,
            "Artifact's Collections page must list it exactly once"
        );
        pages.push(index);
    }
    let presentation = presentation::check(
        native,
        &globals,
        &nodes,
        (item_hash, recipe.donor.item_hash.parse_u32()?),
        &pages,
        source_item.class_type,
    )?;
    let backdrop = backdrop(source, native, &globals, source_item.strings, recipe)?;
    Ok(
        json!({"source_slot":16,"native_bucket":49,"equipment_slot":0x0112,
        "source_class":source_item.class_type,"equip_conditions":conditions,
        "attached_feedback":feedback,"backdrop":backdrop,
        "sockets":sockets.len(),"collectible":collectible,"pages":pages,
        "presentation":presentation,"gameplay_verified":false}),
    )
}

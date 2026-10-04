//! Preserve each source custom, default and locked dye layer with its material program.
use super::*;
use crate::d2_mot::{dye_bundle, dyes, tfx};

fn templates(r: &mut Reader, donor: u32) -> Result<Vec<Value>> {
    let tag = crate::d2_mot::assets::item::find_native(r, donor)?;
    let rows = dyes::inspect(r, tag, false)?;
    let rows = rows.as_array().context("native dye templates")?.clone();
    if !rows.is_empty() {
        return Ok(rows);
    }
    // Undyed native gear still provides a valid runtime carrier. A stock shader supplies
    // record metadata when the source adds dyes. Renderer scopes supply the actual defaults.
    let tag = r
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|t| t.name == "investment_globals")
        .context("native globals")?
        .hash
        .0;
    let globals = r.tag(tag, None)?;
    let root = r.tag(globals.u32(16)?, Some(0x80807D84))?;
    let items = r.tag(root.u32(8 + 48 * 16)?, None)?;
    for row in items.array(8, 24, Some(0x80807BE8))? {
        let tag = items.u32(row + 16)?;
        let item = r.tag(tag, Some(0x80807BEA))?;
        if item.u8(0xB8)? != 14 {
            continue;
        }
        let rows = dyes::inspect(r, tag, false)?;
        let rows = rows.as_array().context("native shader templates")?;
        if !rows.is_empty()
            && rows.iter().all(|row| {
                row["found"]
                    .as_array()
                    .is_some_and(|found| found.len() == 1)
            })
        {
            return Ok(rows.clone());
        }
    }
    anyhow::bail!("No native dye scope template is available")
}

pub(super) fn append(
    modern: &Path,
    native: &Path,
    source: u32,
    donor: u32,
    folder: &Path,
    output: &Path,
    graph: &mut Value,
) -> Result<()> {
    let source_path = folder.join("source-dyes");
    let native_path = folder.join("native-dyes");
    let mut r = Reader::new(modern, &source_path, true)?;
    let (_, tag) = crate::d2_mot::assets::item::find(&mut r, source)?;
    let all = dyes::inspect(&mut r, tag, true)?;
    write_json(&source_path.join("dyes-all.json"), &all)?;
    if all.as_array().context("source dye layers")?.is_empty() {
        graph["dyes"] = json!([]);
        graph["dye_rows"] = json!([[], [], []]);
        graph["material_programs"] = json!([]);
        r.finish()?;
        return Ok(());
    }
    write_json(
        &source_path.join("render-context.json"),
        &tfx::context(&mut r, true)?,
    )?;
    r.finish()?;
    let mut r = Reader::new(native, &native_path, false)?;
    let templates = templates(&mut r, donor)?;
    write_json(
        &native_path.join("render-context.json"),
        &tfx::context(&mut r, false)?,
    )?;
    r.finish()?;
    let mut rows = vec![Vec::<Value>::new(); 3];
    let mut dyes = Vec::new();
    let mut programs = Vec::new();
    for (layer, descriptor) in [0x28u64, 0x38, 0x48].into_iter().enumerate() {
        let source_rows = all
            .as_array()
            .context("source dyes")?
            .iter()
            .filter(|row| row["descriptor"].as_u64() == Some(descriptor))
            .cloned()
            .collect::<Vec<_>>();
        if source_rows.is_empty() {
            continue;
        }
        let mut channels = BTreeSet::new();
        let mut native_rows = Vec::new();
        for row in &source_rows {
            let channel = row["channel"].as_u64().context("source dye channel")?;
            ensure!(
                matches!(channel,0..=2|4..=15) && channels.insert(channel),
                "Duplicate or unsupported source dye channel"
            );
            let mut template = templates
                .iter()
                .find(|t| t["channel"] == row["channel"])
                .or_else(|| templates.first())
                .context("Native gear has no dye scope template")?
                .clone();
            template["channel"] = row["channel"].clone();
            native_rows.push(template);
        }
        write_json(&source_path.join("dyes.json"), &json!(source_rows))?;
        write_json(&native_path.join("dyes.json"), &json!(native_rows))?;
        let seed = folder.join(format!("dye-seed-{layer}"));
        fs::create_dir_all(&seed)?;
        write_json(
            &seed.join("asset-graph.json"),
            &json!({"kind":"shader_preset","nodes":[],"dye_key_base":0xE2600000u32}),
        )?;
        let converted = folder.join(format!("dyes-{layer}"));
        let mut reader = Reader::new(native, &converted, false)?;
        dye_bundle::build(&mut reader, &source_path, &native_path, &seed)?;
        reader.finish()?;
        let converted = graph::append(
            &converted,
            output,
            &format!("layer-{layer}"),
            graph["nodes"].as_array_mut().context("gear nodes")?,
        )?;
        for dye in converted["dyes"].as_array().context("converted dyes")? {
            let mut dye = dye.clone();
            let channel = dye["channel"].as_u64().context("dye channel")?;
            let ordinal = dyes.len();
            dye["manifest"] = json!(private_key(
                profile::hash(graph, "item_hash")?,
                &format!("dye-{ordinal}")
            ));
            rows[layer].push(json!({"channel":channel,"dye":ordinal}));
            dyes.push(dye);
        }
        programs.push(json!({"layer":layer,"programs":converted["material_programs"],"lookups":converted["render_lookups"]}));
    }
    graph["dyes"] = json!(dyes);
    graph["dye_rows"] = json!(rows);
    graph["material_programs"] = json!(programs);
    Ok(())
}

//! Prepare complete shader recipes without entering weapon model or gameplay conversion.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};

pub(super) fn prepare(
    source: &Weapon,
    modern: &Path,
    native: &Path,
    donors: &Value,
    output: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<PathBuf> {
    let output = super::super::reader::outside(output, modern.parent().context("modern root")?)?;
    let output = super::super::reader::outside(&output, native.parent().context("native root")?)?;
    let _indexes = (
        super::super::reader::package_index(&modern.canonicalize()?, true)?,
        super::super::reader::package_index(&native.canonicalize()?, false)?,
    );
    let mut candidates = donors["shaders"]
        .as_array()
        .context("No native shader templates loaded")?
        .iter()
        .collect::<Vec<_>>();
    candidates.sort_by_key(|d| d["hash"].as_u64());
    ensure!(
        !candidates.is_empty(),
        "No installed native shader to convert with"
    );
    let namespace = super::super::compatibility::namespace(source.hash);
    let identity = batch::identity(&namespace)?;
    let item = profile::hash(&identity, "item_hash")?;
    let donor = candidates
        .first()
        .context("Native shader registration template missing")?;
    let hash = profile::hash(donor, "hash")?;
    progress("Converting source shader...".into());
    let folder = reserve_assets(&output, "shader")?;
    let p = json!({"kind":"shader","source_item":source.hash,"native_item":hash,"dye_key_base":0xE2600000u32});
    write_json(&folder.join("profile.json"), &p)?;
    let report = super::super::assets::shader::export(&p, modern, native, &folder).inspect_err(|error| {
        let _ = write_json(&folder.join("failure.json"), &json!({"source_item":source.hash,"native_item":hash,"reason":format!("{error:#}"),"complete":false,"installable":false}));
    })?;
    let graph_path = folder.join("preset");
    let mut graph: Value = serde_json::from_slice(&fs::read(graph_path.join("asset-graph.json"))?)?;
    graph["kind"] = json!("shader");
    graph["item_hash"] = json!(item);
    graph["installable"] = json!(true);
    graph["source_item"] = json!(source.hash);
    graph["source_name"] = json!(source.name);
    graph["native_item"] = json!(hash);
    graph["limitations"] = report["limits"].clone();
    graph["material_programs"] = report["material_programs"].clone();
    let mut keys = std::collections::BTreeSet::new();
    for dye in graph["dyes"]
        .as_array_mut()
        .context("Imported shader dyes")?
    {
        let channel = dye["channel"].as_u64().context("Shader channel")?;
        ensure!(
            matches!(channel, 0..=2 | 4..=15),
            "Unsupported native shader channel {channel}"
        );
        let key = format!("parhelion/{namespace}/dye/{channel}")
            .bytes()
            .fold(0x811C9DC5u32, |h, b| {
                h.wrapping_mul(16777619) ^ u32::from(b)
            });
        ensure!(
            ![0, u32::MAX, 0x811C9DC5].contains(&key) && keys.insert(key),
            "Shader dye identity collision"
        );
        dye["manifest"] = json!(key);
    }
    let presentation: Value =
        serde_json::from_slice(&fs::read(folder.join("source/presentation.json"))?)?;
    ensure!(
        presentation["name"].as_str() == Some(source.name.as_str()),
        "Source shader name changed during import"
    );
    let icon = STANDARD.encode(fs::read(folder.join("source/item-icon.png"))?);
    fs::copy(
        folder.join("source/item-icon.png"),
        graph_path.join("source-icon.png"),
    )?;
    graph["source_icon_png"] = json!("source-icon.png");
    graph["source_rarity"] = presentation["rarity"].clone();
    write_json(&graph_path.join("asset-graph.json"), &graph)?;
    let reference = GraphReference::new(&graph_path, item)?;
    let rarity = match presentation["rarity"].as_u64() {
        Some(1) => "common",
        Some(2) => "uncommon",
        Some(3) => "rare",
        Some(4) => "legendary",
        Some(5) => "exotic",
        _ => anyhow::bail!("Unsupported source shader rarity"),
    };
    let recipe = json!({
        "schema":1,"kind":"shader","collection_placement":"sunrise_badge",
        "namespace":namespace,"identity":identity,"name":source.name,"type_name":"Shader",
        "donor":{"item_hash":format!("0x{hash:08X}"),"expected_name":donor["name"]},
        "flavor":presentation["flavor"].as_str().unwrap_or(""),"source":"Source: Imported shader",
        "overrides":{"rarity":rarity,"imported_graph":reference,"icon_edit":{"imported_image":{"png_base64":icon}}}
    });
    let path = folder.join("shader.parhelion.json");
    write_json(&path, &recipe)?;
    write_json(
        &output.join("result.json"),
        &json!({"recipe":path,"source_item":source.hash,"donor_hash":hash,"limits":report["limits"],"gameplay_verified":false}),
    )?;
    progress("Shader recipe prepared. Native rendering needs an in-game check.".into());
    Ok(path)
}

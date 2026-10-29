//! A modern weapon ornament for a stock weapon whose ornament socket offers its ornaments from a
//! shared plug set. The ornament becomes a mod built on a stock ornament of that set, so it joins
//! the set and the weapon offers it beside its own. Its model converts on the stock weapon's base,
//! the modern weapon of the same hash prepared on its stock self with the native rig, so the
//! ornament wears the stock weapon's skeleton and leaves its animation, audio and gameplay alone.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::collections::BTreeSet;

fn load(path: &Path) -> Result<Value> {
    serde_json::from_slice(&fs::read(path).with_context(|| format!("Read {}", path.display()))?)
        .map_err(Into::into)
}

pub(super) fn prepare(
    source: &Weapon,
    modern: &Path,
    native: &Path,
    donors: &Value,
    output: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<PathBuf> {
    ensure!(
        source.family() == Family::WeaponOrnament,
        "Item is not a weapon ornament"
    );
    let category = source
        .plug_category
        .context("Weapon ornament has no plug category")?;
    let output = crate::d2_mot::reader::outside(output, modern.parent().context("modern root")?)?;
    let output = crate::d2_mot::reader::outside(&output, native.parent().context("native root")?)?;
    let previous = previous_model_donor(&output, source.hash);
    let mut templates = donors["weapon_ornaments"]
        .as_array()
        .context("No stock weapon ornaments loaded")?
        .iter()
        .filter(|template| template["plug_category"].as_u64() == Some(u64::from(category)))
        .collect::<Vec<_>>();
    templates.sort_by_key(|template| {
        (
            template["hash"].as_u64() != previous.map(u64::from),
            template["weapon"].as_u64(),
            template["hash"].as_u64(),
        )
    });
    ensure!(
        !templates.is_empty(),
        "No stock weapon offers this ornament's category from a shared plug set"
    );
    let root = reserve_assets(&output, "ornament")?;
    let item = source.destination()?;
    let mut failures = Vec::new();
    // The base is the weapon's, so each weapon is tried once, on its first template.
    let mut tried = BTreeSet::new();
    for template in templates {
        let weapon = profile::hash(template, "weapon")?;
        if !tried.insert(weapon) {
            continue;
        }
        crate::cancellation::check()?;
        let folder = root.join(format!("candidate-{}", tried.len() - 1));
        fs::create_dir_all(&folder)?;
        match convert(source, (modern, native), template, &folder, item, progress) {
            Ok(path) => {
                write_json(
                    &output.join("result.json"),
                    &json!({"recipe":path,"source_item":source.hash,"donor_hash":template["hash"],
                        "weapon":weapon,"kind":source.family(),"attempts":failures,"gameplay_verified":false}),
                )?;
                progress(format!("{} recipe prepared.", source.family().label()));
                return Ok(path);
            }
            Err(error) => {
                crate::cancellation::check()?;
                if crate::cancellation::is_cancelled(&error) {
                    return Err(error);
                }
                failures.push(
                    json!({"weapon":weapon,"donor":template["hash"],"reason":format!("{error:#}")}),
                );
                write_json(&root.join("failures.json"), &json!(failures))?;
            }
        }
    }
    let reasons = failures
        .iter()
        .filter_map(|row| row["reason"].as_str())
        .collect::<BTreeSet<_>>();
    anyhow::bail!(
        "No stock weapon could carry this ornament. {}",
        reasons.into_iter().take(4).collect::<Vec<_>>().join(". ")
    )
}

/// Convert the ornament on `template`'s weapon into `folder` and write its mod recipe.
fn convert(
    source: &Weapon,
    (modern, native): (&Path, &Path),
    template: &Value,
    folder: &Path,
    item: u32,
    progress: &mut dyn FnMut(String),
) -> Result<PathBuf> {
    let template_hash = profile::hash(template, "hash")?;
    let weapon = profile::hash(template, "weapon")?;
    // A weapon import takes its art, dye and kept part keys from its ordinal, as this does from the
    // mod's own identity.
    let ordinal = item & 0xFFFF;
    // Stock weapons can share an ornament category, so the ornament's model is fitted to the
    // weapon whose modern counterpart offers it, not to whichever weapon has the category.
    progress("Checking which weapon offers the ornament…".into());
    let mut reader = Reader::new(modern, &folder.join("sockets"), true)?;
    let sockets = crate::d2_mot::ornaments::gameplay_sockets(&mut reader, weapon)
        .context("The modern weapon's sockets")?;
    ensure!(
        sockets["sockets"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|socket| socket["choices"].as_array().into_iter().flatten())
            .any(|choice| choice["hash"].as_u64() == Some(u64::from(source.hash))),
        "{} does not offer this ornament",
        template["weapon_name"]
            .as_str()
            .unwrap_or("The stock weapon")
    );
    progress(format!(
        "Preparing {} as the ornament's base…",
        template["weapon_name"]
            .as_str()
            .unwrap_or("the stock weapon")
    ));
    let base = batch::prepare_one_reusing(
        &json!({"hash":weapon,"native_item":weapon,"source_owned_rig":false,"base_only":true}),
        ordinal,
        modern,
        native,
        folder,
        None,
        None,
        progress,
    )
    .context("The stock weapon's base")?;
    let base = PathBuf::from(base["folder"].as_str().context("Weapon base folder")?);
    let out = folder.join("ornament");
    let graph = crate::d2_mot::weapon_ornament::prepare(
        &crate::d2_mot::weapon_ornament::Request {
            modern,
            native,
            weapon: &base,
            ornament: source.hash,
            target_item: item,
            link: None,
            art_key: 0xE2A0_0000u32
                .checked_add(ordinal)
                .context("Art key overflow")?,
            dye_key_base: 0xE2B0_0000u32
                .checked_add(ordinal * 16)
                .context("Dye key overflow")?,
            ordinal,
            out: &out,
        },
        progress,
    )?;
    let source_report = load(&out.join("source").join("report.json"))?;
    ensure!(
        source_report["name"].as_str() == Some(source.name.as_str()),
        "Source name changed during import"
    );
    // A mod built on a stock ornament takes an imported appearance of the ornament kind. It holds
    // the ornament's icon and names, which the recipe's source card shows.
    let manifest = graph.join("asset-graph.json");
    let mut report = load(&manifest)?;
    report["kind"] = json!("ornament");
    fs::copy(
        out.join("source").join("item-icon.png"),
        graph.join("source-icon.png"),
    )?;
    report["source_icon_png"] = json!("source-icon.png");
    report["source_item"] = json!(source.hash);
    report["source_name"] = json!(source.name);
    report["source_type"] = json!(source.weapon_type);
    write_json(&manifest, &report)?;
    let icon = STANDARD.encode(fs::read(out.join("source").join("item-icon.png"))?);
    let description = source_report["flavor"].as_str().unwrap_or("").trim();
    let namespace = source.namespace();
    let reference = GraphReference::new(&graph, item)?;
    // The source artwork is the item's icon, which the build draws on the plate of the mod's
    // rarity under the runtime's watermark. The plug shows the item's icon.
    let mut recipe = json!({"schema":1,"kind":"mod","collection_placement":"sunrise_badge",
        "namespace":namespace,"identity":crate::graph::import_identity(&namespace)?,"name":source.name,
        "donor":{"item_hash":format!("0x{template_hash:08X}"),"expected_name":null},
        "flavor":description,"source":"Source: Imported gear",
        "overrides":{"imported_graph":reference,"ornament_weapon":format!("0x{weapon:08X}"),
            "icon_edit":{"imported_image":{"png_base64":icon}},
            "socket_plug_variants":[{
            "replace_effects":true,"socket_index":0,"choice_index":0,
            "source_plug_hash":format!("0x{template_hash:08X}"),"name":source.name,"description":description,
            "offer_everywhere":true,"sandbox_perks":[]}]}});
    // The plug takes the source ornament's rarity, not its stock template's.
    if let Some(rarity) = source
        .rarity
        .and_then(|rarity| crate::d2_mot::gameplay::rarity(u64::from(rarity)))
    {
        recipe["overrides"]["rarity"] = json!(rarity);
    }
    let path = folder.join("ornament.parhelion.json");
    write_json(&path, &recipe)?;
    Ok(path)
}

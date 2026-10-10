//! Keep source geometry in independently addressable attachment slots.
use super::*;
use std::collections::BTreeSet;

fn key(item: u32, selector: u64, position: usize) -> u32 {
    format!("parhelion/imported-part/{item:08X}/{selector}/{position}")
        .bytes()
        .fold(0x811C9DC5u32, |hash, byte| {
            hash.wrapping_mul(16777619) ^ u32::from(byte)
        })
}

/// Redistribute newly converted passes across the already authored attachment boundaries.
pub(super) fn refresh(g: &mut Graph, draws: &Draws) -> Result<()> {
    let mut description = g.manifest["source_parts"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if description.is_empty() {
        return Ok(());
    }
    let mut assigned = BTreeSet::new();
    let mut selected_count = 0;
    for part in &mut description {
        let models = part["source_models"]
            .as_array()
            .context("part sources")?
            .iter()
            .map(|v| v.as_str().context("source model").map(str::to_owned))
            .collect::<Result<BTreeSet<_>>>()?;
        ensure!(models.is_disjoint(&assigned), "overlapping source parts");
        assigned.extend(models.iter().cloned());
        let mut selected = draws.clone();
        for stage in 0..23 {
            selected.records[stage].retain(|(_, symbol)| {
                draws
                    .sources
                    .get(symbol)
                    .is_some_and(|m| models.contains(m))
            });
            selected.layout(stage)?;
        }
        let (bytes, patches, count) = selected.model()?;
        let symbol = part["model"].as_str().context("part model")?;
        g.write(symbol, &bytes)?;
        g.node_mut(symbol)?["patches"] = json!(patches);
        part["draw_count"] = json!(count);
        selected_count += count;
    }
    let mut main = draws.clone();
    for stage in 0..23 {
        main.records[stage].retain(|(_, symbol)| {
            !draws
                .sources
                .get(symbol)
                .is_some_and(|m| assigned.contains(m))
        });
        main.layout(stage)?;
    }
    let (bytes, patches, count) = main.model()?;
    ensure!(
        count + selected_count == draws.records.iter().map(Vec::len).sum::<usize>(),
        "source partition lost draws"
    );
    g.write("model", &bytes)?;
    g.node_mut("model")?["patches"] = json!(patches);
    g.manifest["native_draw_parts"] = json!(count);
    g.manifest["assembled_draw_parts"] = json!(count + selected_count);
    g.manifest["source_parts"] = json!(description);
    Ok(())
}

/// A source part's attachment slot: selector, position, assignment, models and
/// whether the slot is a source art region.
type Group = (u64, usize, u32, BTreeSet<String>, bool);

/// Promote source parts an older graph already split with donor slots into the
/// new source array.
fn promote(g: &mut Graph, source_parts: &[Value], regions: &BTreeSet<u64>) -> Result<()> {
    // Older graphs may already have split a subset of the same region using
    // donor slots. Promote those pieces too, so the new source array includes
    // every position rather than only the pieces separated in this pass.
    for part in g.manifest["source_parts"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        if !part["selector"]
            .as_u64()
            .is_some_and(|selector| regions.contains(&selector))
        {
            continue;
        }
        let matches = source_parts
            .iter()
            .filter(|source| {
                source["placements"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|placement| {
                        placement["selector"] == part["selector"]
                            && placement["position"] == part["position"]
                    })
            })
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "existing source part has ambiguous region provenance"
        );
        part["assignment"] = json!(tag(&matches[0]["assignment"])?);
        part["source_region"] = json!(true);
    }
    Ok(())
}

/// Select the attachment slot for one source part, or `None` to leave the part
/// assembled.
fn slot(
    part: &Value,
    source: &Value,
    draws: &Draws,
    parents: &[Value],
    regions: &BTreeSet<u64>,
    host: u64,
) -> Result<Option<Group>> {
    if part["missing"] == true {
        return Ok(None);
    }
    let models = source["models"]
        .as_array()
        .context("source models")?
        .iter()
        .filter(|model| model["entity"] == part["entity"])
        .map(|model| {
            model["model"]
                .as_str()
                .context("source model")
                .map(str::to_owned)
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let models = models
        .into_iter()
        .filter(|model| {
            draws
                .records
                .iter()
                .flatten()
                .any(|(_, symbol)| draws.sources.get(symbol) == Some(model))
        })
        .collect::<BTreeSet<_>>();
    if models.is_empty() {
        return Ok(None);
    }
    let placements = part["placements"]
        .as_array()
        .context("source part placements")?;
    let slots = placements
        .iter()
        .filter_map(|placement| {
            Some((
                placement["selector"].as_u64()?,
                placement["position"].as_u64()? as usize,
            ))
        })
        .collect::<BTreeSet<_>>();
    if slots.len() != 1 {
        return Ok(None);
    }
    let (selector, position) = *slots.first().context("source slot")?;
    // Selector zero is the weapon body. Keep it on the chosen marker host,
    // including swords whose native body and marker-bearing slots differ.
    if selector == 0 {
        return Ok(None);
    }
    let candidates = parents
        .iter()
        .filter(|parent| {
            parent["placement"]["selector"].as_u64() == Some(selector)
                && parent["placement"]["position"].as_u64() == Some(position as u64)
        })
        .collect::<Vec<_>>();
    let source_region = regions.contains(&selector);
    let assignment = if source_region {
        tag(&part["assignment"])?
    } else {
        if candidates.len() != 1 {
            return Ok(None);
        }
        tag(&candidates[0]["assignment"])?
    };
    // Marker-bearing slots still use their separately carried source markers.
    // A geometry-only attachment gets its own mutable owner and channel bank.
    if !source_region && (u64::from(assignment) == host || candidates[0]["marker_set"] == true) {
        return Ok(None);
    }
    Ok(Some((
        selector,
        position,
        assignment,
        models,
        source_region,
    )))
}

/// Copy the core nodes and the model node under the part's names, retargeting
/// their references to the copies.
fn write_nodes(
    g: &mut Graph,
    originals: &[Value],
    names: &BTreeMap<String, String>,
    bytes: &[u8],
    patches: &[Value],
) -> Result<()> {
    for name in names.keys() {
        let original = originals
            .iter()
            .find(|node| node["symbol"] == *name)
            .context("part node")?;
        let mut node = original.clone();
        let symbol = &names[name];
        node["symbol"] = json!(symbol);
        node["file"] = json!(format!("{symbol}.bin"));
        if name == "model" {
            node["patches"] = json!(patches);
        }
        for patch in node["patches"].as_array_mut().context("part patches")? {
            if let Some(target) = patch["symbol"]
                .as_str()
                .and_then(|symbol| names.get(symbol))
            {
                patch["symbol"] = json!(target);
            }
        }
        for field in ["reference", "shared_owner"] {
            if let Some(target) = node[field].as_str().and_then(|symbol| names.get(symbol)) {
                node[field] = json!(target);
            }
        }
        let payload = if name == "model" {
            bytes.to_vec()
        } else {
            g.read(name)?.0
        };
        fs::write(
            g.root.join(node["file"].as_str().context("part file")?),
            payload,
        )?;
        g.manifest["nodes"]
            .as_array_mut()
            .context("graph nodes")?
            .push(node);
    }
    Ok(())
}

pub(super) fn separate(
    g: &mut Graph,
    draws: &Draws,
    source: &Value,
    prepared: &Path,
) -> Result<()> {
    if let Some(entity) = source["independent_art_entity"].as_str() {
        ensure!(
            source["models"]
                .as_array()
                .context("independent art models")?
                .iter()
                .all(|model| model["entity"] == entity),
            "Independent art view mixes entities"
        );
        // Gear already converts each art entity independently. Its complete class/body
        // layout is registered after assembly, without weapon attachment heuristics.
        return Ok(());
    }
    // Older exports have no placement provenance. They remain assembled until
    // re-exported, rather than guessing placement from asset ordering.
    let Some(source_parts) = source["art_parts"].as_array() else {
        return Ok(());
    };
    let native = load(&prepared.join("native/template-report.json"))?;
    let parents = native["parents"].as_array().context("native art parents")?;
    let host = g.manifest["native_assignment"]
        .as_u64()
        .context("host assignment")?;
    let item = u32::try_from(g.manifest["item_hash"].as_u64().context("item identity")?)?;
    let mut assigned = BTreeSet::new();
    let mut groups = Vec::new();
    let regions = variants::regions(source)?;
    promote(g, source_parts, &regions)?;
    if let Some(kept) = g.manifest["kept_parts"].as_array_mut() {
        kept.retain(|part| {
            !part["placement"]["selector"]
                .as_u64()
                .is_some_and(|selector| regions.contains(&selector))
        });
    }
    g.manifest["source_art_regions"] = json!({"selectors":regions,"selection":"native region stat with source-ordered alternatives","gameplay_verified":false});
    ensure!(
        !parents.iter().any(|parent| {
            tag(&parent["assignment"]).ok().map(u64::from) == Some(host)
                && parent["placement"]["selector"]
                    .as_u64()
                    .is_some_and(|s| regions.contains(&s))
        }),
        "source alternatives cannot replace the assembled body's art region"
    );
    for part in source_parts {
        let Some((selector, position, assignment, models, source_region)) =
            slot(part, source, draws, parents, &regions, host)?
        else {
            continue;
        };
        ensure!(
            models.is_disjoint(&assigned),
            "source geometry occupies ambiguous attachment slots"
        );
        assigned.extend(models.iter().cloned());
        groups.push((selector, position, assignment, models, source_region));
    }
    if groups.is_empty() {
        return Ok(());
    }
    ensure!(
        groups.len() <= 64,
        "too many independent source attachments"
    );
    for (_, symbol) in draws.records.iter().flatten() {
        ensure!(
            draws.sources.contains_key(symbol),
            "source draw lacks model provenance"
        );
    }
    let originals = g.manifest["nodes"]
        .as_array()
        .context("graph nodes")?
        .clone();
    let core = [
        "parent",
        "parent-companion",
        "entity",
        "owner",
        "object-channels",
        "object-channel-allocation",
        "object-channel-input-allocation",
    ];
    let mut description = g.manifest["source_parts"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let first_index = description.len();
    let previous_count = description
        .iter()
        .map(|part| number(&part["draw_count"]))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .sum::<usize>();
    let mut keys = BTreeSet::from([
        0,
        u32::MAX,
        crate::d2_mot::artwork::EMPTY,
        u32::try_from(g.manifest["art_key"].as_u64().context("art key")?)?,
    ]);
    for part in &description {
        ensure!(
            keys.insert(u32::try_from(
                part["key"].as_u64().context("existing art key")?
            )?),
            "existing source attachment key collision"
        );
    }
    let mut selected_count = 0;
    for (index, (selector, position, assignment, models, source_region)) in
        groups.into_iter().enumerate()
    {
        let prefix = format!("part-{}", first_index + index);
        let names = core
            .iter()
            .filter(|name| originals.iter().any(|node| node["symbol"] == **name))
            .map(|name| ((*name).to_owned(), format!("{prefix}-{name}")))
            .chain(std::iter::once((
                "model".to_owned(),
                format!("{prefix}-model"),
            )))
            .collect::<BTreeMap<_, _>>();
        let mut selected = draws.clone();
        for stage in 0..23 {
            selected.records[stage].retain(|(_, symbol)| models.contains(&draws.sources[symbol]));
            selected.layout(stage)?;
        }
        let (bytes, patches, count) = selected.model()?;
        ensure!(count > 0, "independent source attachment has no draws");
        selected_count += count;
        write_nodes(g, &originals, &names, &bytes, &patches)?;
        let art_key = key(item, selector, position);
        ensure!(
            keys.insert(art_key),
            "private source attachment key collision"
        );
        description.push(
            json!({"assignment":assignment,"key":art_key,"selector":selector,
            "position":position,"source_region":source_region,"source_models":models,"parent":names["parent"],
            "entity":names["entity"],"owner":names["owner"],"model":names["model"],
            "draw_count":count,"gameplay_verified":false}),
        );
    }
    let mut main = draws.clone();
    for stage in 0..23 {
        main.records[stage].retain(|(_, symbol)| !assigned.contains(&draws.sources[symbol]));
        main.layout(stage)?;
    }
    let (bytes, patches, count) = main.model()?;
    ensure!(
        count > 0 && count + selected_count == draws.records.iter().map(Vec::len).sum::<usize>(),
        "source attachment partition loses or duplicates draws"
    );
    g.write("model", &bytes)?;
    g.node_mut("model")?["patches"] = json!(patches);
    g.manifest["assembled_draw_parts"] = json!(count + selected_count + previous_count);
    g.manifest["native_draw_parts"] = json!(count);
    g.manifest["source_parts"] = json!(description);
    if let Some(kept) = g.manifest["kept_parts"].as_array_mut() {
        kept.retain(|part| {
            !part["placement"]["selector"]
                .as_u64()
                .is_some_and(|s| regions.contains(&s))
        });
    }
    g.manifest["source_art_regions"] = json!({"selectors":regions,"selection":"native region stat with source-ordered alternatives","gameplay_verified":false});
    Ok(())
}

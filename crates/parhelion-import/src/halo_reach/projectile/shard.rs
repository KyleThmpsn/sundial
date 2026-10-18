//! Target-owned shard collections with source threshold and countdowns.
use super::*;
use assembly::Assembly;

pub(super) struct Shards {
    count: u32,
    delay: f32,
    expiry: f32,
}

impl Shards {
    pub fn read(source: &Value) -> Result<Option<Self>> {
        let count = source["super_detonation_count"]
            .as_i64()
            .context("Source shard threshold")?;
        if count <= 0 {
            return Ok(None);
        }
        let count = u32::try_from(count)?;
        let delay = source["super_detonation_seconds"]
            .as_f64()
            .context("Source shard delay")? as f32;
        let expiry = source["timer_seconds"][1]
            .as_f64()
            .context("Source shard expiry")? as f32;
        ensure!(
            (2..=32).contains(&count)
                && delay.is_finite()
                && expiry.is_finite()
                && delay > 0.
                && expiry > delay
                && source["timer_start"] == 2
                && source["flags"].as_u64().is_some_and(|flags| flags & 4 != 0),
            "Unsupported source attached-shard timing policy"
        );
        Ok(Some(Self {
            count,
            delay,
            expiry,
        }))
    }

    pub fn append(
        &self,
        reader: &mut Reader,
        assets: &mut Assets,
        owners: &mut BTreeMap<u32, Payload>,
    ) -> Result<Value> {
        let mut collector = Assembly::read(reader, 0x80ef286e)?;
        let tag = collector.one(0x80802cda)?;
        let p = collector.owners.get_mut(&tag).unwrap();
        let d = p.pointer(24)?;
        ensure!(
            p.u32(d + 0x64)? == 5 && p.u32(d + 0x1d0)? == 0x80ef27fb,
            "Native collector admission or finish contract differs"
        );
        let mut removed = BTreeSet::new();
        for offset in [0x68, 0xc8, 0x128] {
            removed.extend(program::unbind(p, d + offset)?);
        }
        put(&mut p.0, d + 0x64, &self.count.to_le_bytes())?;
        program::first(p, d + 0x68 + 0x10, self.delay)?;
        program::first(p, d + 0xc8 + 0x10, self.expiry)?;
        program::constant(p, d + 0x128 + 0x10, 0.)?;
        let impact_graph = Assembly::read(reader, 0x80ef27fb)?;
        let impact = append(reader, assets, 0x80ef27fb, impact_graph)?;
        // The finish reference begins at +0x1C8. Its tag is eight bytes in.
        put(&mut p.0, d + 0x1d0, &impact.to_le_bytes())?;
        collector.entity =
            assembly::without_inputs(&collector.entity, &collector.owners, &removed)?;
        let root = append(reader, assets, 0x80ef286e, collector)?;
        let mut hits = 0;
        let mut timers = 0;
        let mut placements = 0;
        for p in owners.values_mut() {
            if Assembly::class(p)? != 0x8080388f {
                continue;
            }
            let d = p.pointer(24)?;
            let i = p.pointer(16)?;
            let programs = p.array(i + 0x40, 48, Some(0x808089f7))?;
            for group in p.array(d + 0x218, 88, Some(0x808037cd))? {
                // CEB5D0 evaluates this optional group gate before its material
                // filter. The source shard policy does not use carrier perks.
                put(&mut p.0, group + 0x40, &u32::MAX.to_le_bytes())?;
                for response in p.array(group + 0x48, 392, Some(0x808037d0))? {
                    let mut target = false;
                    let mut timer = false;
                    let mut attachment = false;
                    for action in p.array(response + 0x178, 16, Some(0x808037f7))? {
                        let parameter = p.pointer(action)?;
                        match (p.u32(action + 8)?, p.u32(parameter - 4)?) {
                            (19, 0x80802ce7) => match p.u32(parameter + 8)? {
                                0x80ef286e => {
                                    put(&mut p.0, parameter + 8, &root.to_le_bytes())?;
                                    hits += 1;
                                    target = true;
                                }
                                0x80ef27fb => {
                                    put(&mut p.0, parameter + 8, &impact.to_le_bytes())?;
                                }
                                _ => anyhow::bail!("Unknown shard contact collection"),
                            },
                            (1, 0x808037e2) => {
                                put(&mut p.0, parameter + 8, &[1])?;
                                placements += 1;
                                attachment = true;
                            }
                            (0, 0x808037e1) => {
                                let index = usize::try_from(p.u32(parameter + 0x30)?)?;
                                let expression =
                                    *programs.get(index).context("Shard contact timer program")?;
                                let definition = usize::try_from(p.u64(expression + 8)?)?;
                                ensure!(
                                    p.u64(definition + 8)? == expression as u64,
                                    "Shard contact timer reciprocal differs"
                                );
                                program::constant(p, definition + 0x10, self.expiry)?;
                                put(&mut p.0, parameter, &[0])?;
                                timer = true;
                            }
                            _ => {}
                        }
                    }
                    let fallback = timer && attachment;
                    timers += usize::from(fallback);
                    // CEB970 rejects zero probability before the action list.
                    // Preserve material and target filters, while disabling the
                    // carrier's instant finish and special impact alternatives.
                    program::constant(
                        p,
                        response + 0xc0,
                        if target || fallback { 1. } else { 0. },
                    )?;
                    if target || fallback {
                        program::constant(p, response + 0x100, 1.)?;
                    }
                }
            }
        }
        ensure!(
            hits == 1 && timers == 1 && placements > 0,
            "Shard contact routing differs"
        );
        Ok(
            json!({"threshold":self.count,"combine_delay_seconds":self.delay,"expiry_seconds":self.expiry,
            "collector":assets.names[&root],"impact":assets.names[&impact],
            "scope":"A private collector on each struck target, namespaced again for the authored item",
            "contact_policy":"Eligible target attachment or timed surface attachment, without carrier perk gates or special finish responses",
            "retained_native_behavior":["Native host launch-speed adaptation", "Native target acquisition and conservative source minimum turn rate", "Native damage calculation and impact presentation", "Incomplete collections expire from their latest accepted hit", "Detonating shards finish one per native update"],
            "gameplay_verified":false}),
        )
    }
}

fn append(reader: &mut Reader, assets: &mut Assets, tag: u32, mut graph: Assembly) -> Result<u32> {
    let root = assets.reserve(reader, format!("reach-projectile-graph-{tag:08X}"))?;
    let mut ids = BTreeMap::new();
    let mut allocations = BTreeMap::new();
    for (&owner, p) in &graph.owners {
        ids.insert(
            owner,
            assets.reserve(reader, format!("reach-projectile-{tag:08X}-{owner:08X}"))?,
        );
        let allocation = p.u32(0x44)?;
        if let std::collections::btree_map::Entry::Vacant(entry) = allocations.entry(allocation) {
            entry.insert(assets.reserve(
                reader,
                format!("reach-projectile-{tag:08X}-allocation-{allocation:08X}"),
            )?);
        }
    }
    let mut converted = graph.owners.clone();
    for (&owner, p) in &mut converted {
        super::relocate(p, &graph.owners, &ids, &[])?;
        let allocation = p.u32(0x44)?;
        put(&mut p.0, 0x44, &allocations[&allocation].to_le_bytes())?;
        assets.push(ids[&owner], owner, p.clone(), None)?;
    }
    for (tag, id) in allocations {
        assets.push(id, tag, (*reader.tag(tag, Some(0x80809bbb))?).clone(), None)?;
    }
    let rows = graph.entity.array(16, 12, Some(0x80809c04))?;
    let components = rows
        .iter()
        .map(|&row| {
            let owner = graph.entity.u32(row)?;
            Ok(compiled::Component {
                owner: ids[&owner],
                payload: &converted[&owner],
                template: &graph.owners[&owner],
                entity: &graph.entity,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let replication_template = graph.entity.u32(0x88)?;
    if replication_template != u32::MAX {
        reader.tag(replication_template, Some(0x80809bb6))?;
        let replication =
            assets.reserve(reader, format!("reach-projectile-{tag:08X}-replication"))?;
        assets.push(
            replication,
            replication_template,
            compiled::replication::emit(root, &components)?,
            None,
        )?;
        put(&mut graph.entity.0, 0x88, &replication.to_le_bytes())?;
    }
    super::relocate(&mut graph.entity, &graph.owners, &ids, &rows)?;
    assets.push(root, tag, graph.entity, None)?;
    Ok(root)
}

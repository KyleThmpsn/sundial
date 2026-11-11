//! Target-owned shard collections with source threshold and countdowns.
use super::*;
use crate::halo_reach::blast;
use crate::tiger::sequence;
use assembly::Assembly;

pub(super) struct Shards {
    count: u32,
    delay: f32,
    expiry: f32,
}

/// The source detonation's light on the finish: the private `record` that replaces the stock light
/// record `stock` in its light event, and the `seconds` the source light lasts, zero to keep the
/// event's own duration.
pub(super) struct Light {
    pub record: u32,
    pub stock: u32,
    pub seconds: f32,
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
        (behavior, projectile): (&Value, &Value),
        explosion: (&[(u32, u8)], f32, Option<&Light>),
        contact: Option<u32>,
    ) -> Result<Value> {
        let mut collector = Assembly::read(reader, 0x80ef286e)?;
        let tag = collector.one(0x80802cda)?;
        let p = collector.owners.get_mut(&tag).unwrap();
        let d = p.pointer(24)?;
        ensure!(
            p.u32(d + 0x64)? == 5
                && p.u32(d + 0x1d0)? == 0x80ef27fb
                && p.u32(d + 0x1e0)? == u32::MAX,
            "Native collector admission or finish contract differs"
        );
        // The stock latch expression at `+0x128` stays with its six channel inputs. It is
        // `is_zero` of their sum, not a timer, and a constant zero there kept a full collection
        // from latching: in game six needles waited out the ordinary delay instead.
        let mut removed = BTreeSet::new();
        for offset in [0x68, 0xc8] {
            removed.extend(program::unbind(p, d + offset)?);
        }
        put(&mut p.0, d + 0x64, &self.count.to_le_bytes())?;
        program::first(p, d + 0x68 + 0x10, self.delay)?;
        program::first(p, d + 0xc8 + 0x10, self.expiry)?;
        let mut impact_graph = Assembly::read(reader, 0x80ef27fb)?;
        let combine = self.combine(
            reader,
            assets,
            &mut impact_graph,
            behavior,
            projectile,
            None,
        )?;
        let combine_visuals = visuals(reader, &mut impact_graph, explosion)?;
        let impact = append(reader, assets, 0x80ef27fb, impact_graph, "")?;
        // A needle that detonates alone deals the source's small per-needle amount, where a
        // supercombine shares the source's large one. Without a fixed impact amount it shares the
        // supercombine finish.
        let (timed, timed_report) =
            match super::detonation::contact(behavior, projectile, "impact_damage")? {
                Ok((amount, _)) => {
                    let mut graph = Assembly::read(reader, 0x80ef27fb)?;
                    let report = self.combine(
                        reader,
                        assets,
                        &mut graph,
                        behavior,
                        projectile,
                        Some(amount),
                    )?;
                    let mut report = report;
                    report["visuals"] = visuals(reader, &mut graph, explosion)?;
                    (append(reader, assets, 0x80ef27fb, graph, "-timed")?, report)
                }
                Err(reason) => (impact, json!({"status":"shared","reason":reason})),
            };
        // The finish references begin at +0x1C8 (latched) and +0x1D8 (ordinary). Each tag is
        // eight bytes in. Reach needles that do not supercombine still explode on their own once
        // the attached timer runs out, so an expiring collection finishes through the same graph,
        // where stock Malfeasance names none.
        put(&mut p.0, d + 0x1d0, &impact.to_le_bytes())?;
        put(&mut p.0, d + 0x1e0, &timed.to_le_bytes())?;
        // The collector keeps one weak handle per admitted shard in instance `+0xD0`, 8-byte rows
        // of class `80802CDC` sized by allocation record `F011A37B`. Stock Malfeasance holds 5
        // for its limit of 5, so a larger source threshold wrote past the array: the 6th needle
        // in one target faulted the client under the collector's admission at `exe+0x308661`.
        // The rows grow to the threshold, and the definition moves past them last, after every
        // definition write above.
        let instance = p.pointer(16)?;
        let handles = p.array(instance + 0xd0, 8, Some(0x80802cdc))?.len();
        let count = usize::try_from(self.count)?;
        if handles < count {
            crate::presentation::append(
                &mut p.0,
                instance + 0xd0,
                0x80802cdc,
                &vec![0xff; count * 8],
                8,
            )?;
            let tag = p.u32(0x44)?;
            let mut allocation = (*reader.tag(tag, Some(0x80809bbb))?).clone();
            let records = allocation
                .array(0x20, 40, Some(0x80808852))?
                .into_iter()
                .filter(|&row| {
                    allocation.u32(row).ok() == Some(0xf011a37b)
                        && allocation.u32(row + 16).ok() == Some(0x80802cdc)
                })
                .collect::<Vec<_>>();
            let [record] = records.as_slice() else {
                anyhow::bail!("Native collector handle allocation is absent or ambiguous")
            };
            ensure!(
                usize::try_from(allocation.u32(record + 20)?)? == handles,
                "Native collector handle allocation differs from its array"
            );
            put(&mut allocation.0, record + 20, &self.count.to_le_bytes())?;
            collector.allocations.insert(tag, allocation);
            let end = p.0.len();
            crate::tiger::instance::seal_span(p, &mut Vec::new(), end)?;
        }
        collector.entity =
            assembly::without_inputs(&collector.entity, &collector.owners, &removed)?;
        let root = append(reader, assets, 0x80ef286e, collector, "")?;
        let (impact_profile, mut impact_damage) =
            self.impact(reader, assets, owners, behavior, projectile)?;
        let mut contacts = 0;
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
                            // A contact damage action names its own profile. `FFFFFFFF` deals
                            // the firing weapon's damage, so each needle took the donor rifle's.
                            (9, 0x808037f3) => {
                                if let Some(profile) = impact_profile
                                    && p.u32(parameter + 8)? == u32::MAX
                                {
                                    put(&mut p.0, parameter + 8, &profile.to_le_bytes())?;
                                    contacts += 1;
                                }
                            }
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
        if impact_profile.is_some() {
            ensure!(contacts > 0, "Shard contact damage actions are absent");
            impact_damage["contact_actions"] = json!(contacts);
        }
        let presentation = Self::presentation(owners, timed, contact)?;
        // The shard movement hosts a default-material bounce only, which adds no response group.
        let ricochet = super::response::ricochet(reader, owners, &mut BTreeMap::new(), projectile)?;
        Ok(
            json!({"threshold":self.count,"combine_delay_seconds":self.delay,"expiry_seconds":self.expiry,
            "collector":assets.names[&root],"impact":assets.names[&impact],"combine":combine,"visuals":combine_visuals,"timed":timed_report,
            "impact_damage":impact_damage,"contact_presentation":presentation,"ricochet":ricochet,
            "scope":"A private collector on each struck target, namespaced again for the authored item",
            "contact_policy":"Eligible target attachment or timed surface attachment, without carrier perk gates or special finish responses",
            "retained_native_behavior":["Native host launch-speed adaptation", "Native target acquisition and conservative source minimum turn rate", "Native shard contact damage and impact sound", "Incomplete collections detonate through the same finish once the source attached timer runs from their latest accepted hit", "Detonating shards finish one per native update"],
            "gameplay_verified":false}),
        )
    }
}

impl Shards {
    /// Replaces the movement's impact table at definition `+0x1E0`. That `80808BCD` table names the
    /// graphs a contact spawns by material, and the donor's played its own explosion on every
    /// surface hit. It takes `impact`, the private table creating the source contact effect, or
    /// none, since the response worker builds no contact effects without one. A needle left on a
    /// surface finishes through the movement's final-impact graph at `+0x180` once its attached
    /// timer runs out, so it takes `finish`, the private timed finish with the converted source
    /// detonation. Collected needles finish through the collector's own graphs. Both definition
    /// views are edited.
    fn presentation(
        owners: &mut BTreeMap<u32, Payload>,
        finish: u32,
        impact: Option<u32>,
    ) -> Result<Value> {
        let mut movements = owners
            .values_mut()
            .filter(|p| Assembly::class(p).ok() == Some(0x8080388f))
            .collect::<Vec<_>>();
        let [p] = movements.as_mut_slice() else {
            anyhow::bail!("Shard movement is absent or ambiguous")
        };
        let definition = p.pointer(24)?;
        let views = BTreeSet::from([definition, usize::try_from(p.u64(p.pointer(16)? + 8)?)?]);
        let table = p.u32(definition + 0x1e0)?;
        for &view in &views {
            ensure!(
                p.u32(view + 0x1e0)? == table && p.u32(view + 0x180)? == u32::MAX,
                "Shard movement views differ or already name a final-impact graph"
            );
        }
        for view in views {
            put(
                &mut p.0,
                view + 0x1e0,
                &impact.unwrap_or(u32::MAX).to_le_bytes(),
            )?;
            put(&mut p.0, view + 0x180, &finish.to_le_bytes())?;
        }
        Ok(
            json!({"native_impact_table":format!("{table:08X}"),"impact_table":impact,
            "final_impact":"timed finish",
            "limits":["Without a converted source contact effect a contact shows nothing until the needle detonates.",
                "The movement's native impact sound at +0x1F0 stays."],
            "gameplay_verified":false}),
        )
    }

    /// Translates the source's default-material bounce into the shard movement's free contact
    /// response.
    ///
    /// Gives each shard's direct hit the source impact amount through a private absolute copy of
    /// the movement's native direct-hit profile at definition `+0x118`, so a needle deals the
    /// source's small hit and the supercombine carries the large share. A sealed movement keeps
    /// its original definition view, so both views name the copy.
    fn impact(
        &self,
        reader: &mut Reader,
        assets: &mut Assets,
        owners: &mut BTreeMap<u32, Payload>,
        behavior: &Value,
        projectile: &Value,
    ) -> Result<(Option<u32>, Value)> {
        let (amount, source) =
            match super::detonation::contact(behavior, projectile, "impact_damage")? {
                Ok(found) => found,
                Err(reason) => return Ok((None, json!({"status":"native","reason":reason}))),
            };
        let mut movements = owners
            .values_mut()
            .filter(|p| Assembly::class(p).ok() == Some(0x8080388f))
            .collect::<Vec<_>>();
        let [p] = movements.as_mut_slice() else {
            anyhow::bail!("Shard movement is absent or ambiguous")
        };
        let definition = p.pointer(24)?;
        let native = p.u32(definition + 0x118)?;
        if native == u32::MAX {
            return Ok((
                None,
                json!({"status":"native","reason":"The movement names no direct-hit profile"}),
            ));
        }
        let views = BTreeSet::from([definition, usize::try_from(p.u64(p.pointer(16)? + 8)?)?]);
        for &view in &views {
            ensure!(
                p.u32(view + 0x118)? == native,
                "Shard movement definition views name other direct-hit profiles"
            );
        }
        let snapshot = assets.clone();
        let (profile, report) = match super::detonation::profile(
            reader,
            assets,
            "reach-projectile-impact-damage",
            native,
            amount,
            0.,
        ) {
            Ok(created) => created,
            Err(error) => {
                *assets = snapshot;
                return Ok((
                    None,
                    json!({"status":"native","native":format!("{native:08X}"),
                    "reason":format!("The native direct-hit profile has another structure: {error:#}")}),
                ));
            }
        };
        for view in views {
            put(&mut p.0, view + 0x118, &profile.to_le_bytes())?;
        }
        Ok((
            Some(profile),
            json!({"status":"translated","source":source,"amount":amount,"native":format!("{native:08X}"),
            "profile":report,"gameplay_verified":false,
            "limits":["The source impact acceleration is not carried. Target resistance, power and difficulty scaling stay native."]}),
        ))
    }

    /// Points the latched finish's damage query at the source supercombine. Every collected
    /// shard finishes through that query once, so each carries an equal share of the source
    /// amount and push. A native finish laid out another way keeps its native damage.
    fn combine(
        &self,
        reader: &mut Reader,
        assets: &mut Assets,
        graph: &mut Assembly,
        behavior: &Value,
        projectile: &Value,
        timed: Option<f32>,
    ) -> Result<Value> {
        // The collection belongs to the struck target, so its finish is the source damage to
        // the object the shards are attached to. A timed finish carries `timed` per shard with no
        // push, for needles that detonate alone.
        let mut found = None;
        for role in ["attached_super_damage", "super_damage"] {
            if let Ok(radial) = super::detonation::radial(behavior, projectile, role)? {
                found = Some((role, radial));
                break;
            }
        }
        let Some((role, radial)) = found else {
            return Ok(
                json!({"status":"native","reason":"The source has no spherical supercombine damage"}),
            );
        };
        let metres = rig::METRES_PER_UNIT;
        let share = self.count as f32;
        let radius = radial.radius[1] * metres;
        let start = radial.radius[0] / radial.radius[1];
        let snapshot = (assets.clone(), graph.owners.clone());
        let mut translate = || -> Result<Value> {
            let mut queries = Vec::new();
            for (&tag, p) in &graph.owners {
                if Assembly::class(p)? != 0x808084e9 {
                    continue;
                }
                let d = p.pointer(24)?;
                for event in p.array(d + 0x168, 24, Some(0x808093e6))? {
                    let q = p.pointer(event + 16)?;
                    if p.u32(q - 4)? == 0x80804ab9 {
                        queries.push((tag, q));
                    }
                }
            }
            let &[(tag, q)] = queries.as_slice() else {
                anyhow::bail!("Native supercombine needs one damage query")
            };
            let p = graph
                .owners
                .get_mut(&tag)
                .context("Native supercombine flow")?;
            let template = p.u32(blast::action(p, q)? + 8)?;
            let (symbol, amount, push) = match timed {
                Some(amount) => ("reach-projectile-timed-damage", amount, 0.),
                None => (
                    "reach-projectile-supercombine-damage",
                    radial.amount / share,
                    crate::halo_reach::rig::push_strength(radial.acceleration, metres) / share,
                ),
            };
            let (profile, report) =
                super::detonation::profile(reader, assets, symbol, template, amount, push)?;
            blast::retarget(p, q, start, Some(radius), profile)?;
            Ok(report)
        };
        match translate() {
            Ok(profile) => Ok(
                json!({"status":"translated","source":radial.source["tag"],"role":role,
                "radius_metres":radius,"falloff_start":start,"total_amount":radial.amount,
                "shard_amount":timed.unwrap_or(radial.amount / share),"shard_push":if timed.is_some() { 0. } else { crate::halo_reach::rig::push_strength(radial.acceleration, metres) / share },
                "profile":profile,
                "limits":["Each collected shard applies its share once as it finishes, one per native update, so a recipient at the centre takes the source amount over that many updates.",
                    "Other recipients inside the radius take the attached amount instead of the smaller source area damage.",
                    "The source minimum damage, falloff exponent and core radius are not translated. Push strength needs gameplay calibration."],
                "gameplay_verified":false}),
            ),
            Err(error) => {
                (*assets, graph.owners) = snapshot;
                Ok(json!({"status":"native","source":radial.source["tag"],
                    "reason":format!("The native supercombine has another structure: {error:#}")}))
            }
        }
    }
}

/// Appends a private copy of the stock graph `tag`. `variant` tells copies of one template apart.
fn append(
    reader: &mut Reader,
    assets: &mut Assets,
    tag: u32,
    mut graph: Assembly,
    variant: &str,
) -> Result<u32> {
    let root = assets.reserve(reader, format!("reach-projectile-graph-{tag:08X}{variant}"))?;
    let mut ids = BTreeMap::new();
    let mut allocations = BTreeMap::new();
    for (&owner, p) in &graph.owners {
        ids.insert(
            owner,
            assets.reserve(
                reader,
                format!("reach-projectile-{tag:08X}{variant}-{owner:08X}"),
            )?,
        );
        let allocation = p.u32(0x44)?;
        if let std::collections::btree_map::Entry::Vacant(entry) = allocations.entry(allocation) {
            entry.insert(assets.reserve(
                reader,
                format!("reach-projectile-{tag:08X}{variant}-allocation-{allocation:08X}"),
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
        let payload = match graph.allocations.remove(&tag) {
            Some(grown) => grown,
            None => (*reader.tag(tag, Some(0x80809bbb))?).clone(),
        };
        assets.push(id, tag, payload, None)?;
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
        let replication = assets.reserve(
            reader,
            format!("reach-projectile-{tag:08X}{variant}-replication"),
        )?;
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

/// Native finish events that only present: particles `80806CC6`, light definitions `80806F49`,
/// and `808093C2` and `80806F40`, which the stock supercombine finish uses for its surface marks
/// and flare.
const PRESENTATION: [u32; 4] = [0x8080_6cc6, 0x8080_6f49, 0x8080_93c2, 0x8080_6f40];

/// Whether a flow control holds a condition, which the trail's unconditional path also refuses.
fn conditional(p: &Payload, control: usize) -> bool {
    p.0[control + 0x18..control + 0x30].iter().any(|&v| v != 0)
        || p.0[control + 0x48..control + 0x58].iter().any(|&v| v != 0)
}

/// Replaces a native finish's unconditional particle bursts with the source detonation's
/// systems, one system per burst event as the flight trail swaps its one system, so no event holds
/// more systems than its native instance was built for. Each used burst lasts the systems' longest
/// life, since the converted systems end with their host. A light event naming the stock record of
/// `light` names its private copy. The flow's controls are then rebuilt to start only those
/// events and the finish's nonpresentation events, such as its damage query and audio. Removing
/// events from the stock controls' child arrays alone left them playing in game. A finish laid out
/// another way keeps its native presentation.
fn visuals(
    reader: &mut Reader,
    graph: &mut Assembly,
    (systems, life, light): (&[(u32, u8)], f32, Option<&Light>),
) -> Result<Value> {
    let owners = &mut graph.owners;
    if systems.is_empty() {
        return Ok(json!({"status":"native","reason":"No source detonation system was converted"}));
    }
    let flows = owners
        .iter()
        .filter(|(_, p)| Assembly::class(p).ok() == Some(0x808084e9))
        .map(|(&tag, _)| tag)
        .collect::<Vec<_>>();
    let [flow] = flows.as_slice() else {
        anyhow::bail!("Native finish flow is absent or ambiguous")
    };
    let snapshot = owners[flow].clone();
    let tag = snapshot.u32(0x44)?;
    let mut allocation = match graph.allocations.get(&tag) {
        Some(allocation) => allocation.clone(),
        None => (*reader.tag(tag, Some(0x80809bbb))?).clone(),
    };
    let result = burst(
        owners.get_mut(flow).unwrap(),
        &mut allocation,
        &snapshot,
        systems,
        life,
        light,
    );
    match result {
        Ok(report) => {
            graph.allocations.insert(tag, allocation);
            Ok(report)
        }
        Err(error) => {
            owners.insert(*flow, snapshot);
            Ok(json!({"status":"native",
                "reason":format!("The native finish presentation has another structure: {error:#}")}))
        }
    }
}

/// Each source system takes one burst event and as many of that event's stock locators, in order,
/// as its definition has transform rows (`+0x47`, carried with its id). System initialization
/// (exe+12190C0 in build 86657) gives the slot's heap 801 table one 16-byte entry per transform row
/// and then writes one entry per locator the event binds, so an event binding more locators than
/// rows writes past the table into the next block's header. The stock supercombine bursts bind two
/// locators for their two-row systems, while a converted local system has one row, its emitter's.
fn burst(
    p: &mut Payload,
    allocation: &mut Payload,
    stock: &Payload,
    systems: &[(u32, u8)],
    life: f32,
    light: Option<&Light>,
) -> Result<Value> {
    ensure!(
        life.is_finite() && life > 0.,
        "Source detonation particles have no life"
    );
    let d = p.pointer(24)?;
    let controls = p
        .array(d + 0x158, 24, Some(0x808093e6))?
        .into_iter()
        .map(|row| p.pointer(row + 16))
        .collect::<Result<Vec<_>>>()?;
    let events = p
        .array(d + 0x168, 24, Some(0x808093e6))?
        .into_iter()
        .map(|row| p.pointer(row + 16))
        .collect::<Result<Vec<_>>>()?;
    let bursts = find_bursts(p, &controls, &events)?;
    ensure!(
        !bursts.is_empty() && systems.len() <= bursts.len(),
        "Native finish has {} single-system bursts for {} source systems",
        bursts.len(),
        systems.len()
    );
    for (&(_, event), &(id, rows)) in bursts.iter().zip(systems) {
        let row = p.array(event + 0x40, 24, Some(0x80806cc8))?[0];
        let mut locators = p
            .array(row, 1, Some(0x80800009))?
            .into_iter()
            .map(|at| p.u8(at))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            rows >= 1 && locators.len() >= usize::from(rows),
            "Native finish burst binds {} locators for a system of {rows} transform rows",
            locators.len()
        );
        locators.truncate(usize::from(rows));
        let fields = crate::tiger::sequence::particle::write_bound(
            p,
            event,
            &[(format!("{id:08X}"), locators)],
        )?;
        for (field, _) in fields {
            put(&mut p.0, field, &id.to_le_bytes())?;
        }
        for field in [0x24, 0x28] {
            put(&mut p.0, event + field, &life.to_le_bytes())?;
        }
    }
    // The light event that names the source light's stock record takes its private copy.
    let hosts = bursts
        .iter()
        .take(systems.len())
        .map(|&(child, _)| child)
        .collect::<BTreeSet<_>>();
    let mut lit = BTreeSet::new();
    for &control in &controls {
        for at in p.array(control + 0x38, 4, Some(0x808093fb))? {
            let child = p.u32(at)?;
            if child & 0xffff != 1 || hosts.contains(&child) {
                continue;
            }
            let event = *events
                .get((child >> 16) as usize)
                .context("Native finish event")?;
            if p.u32(event - 4)? == 0x8080_6f49
                && light.is_some_and(|light| p.u32(event + 0x40).ok() == Some(light.stock))
            {
                lit.insert(event);
            }
        }
    }
    // A light event names its light record at `+0x40` and keeps its durations at `+0x24` and
    // `+0x28`, as particle events do.
    if let Some(light) = light {
        for &event in &lit {
            put(&mut p.0, event + 0x40, &light.record.to_le_bytes())?;
            if light.seconds > 0. {
                for field in [0x24, 0x28] {
                    put(&mut p.0, event + field, &light.seconds.to_le_bytes())?;
                }
            }
        }
    }
    // Every event the finish keeps: the hosts, recolored lights and each nonpresentation event.
    let host_events = bursts
        .iter()
        .take(systems.len())
        .map(|&(_, event)| event)
        .collect::<BTreeSet<_>>();
    let kept = events
        .iter()
        .copied()
        .filter(|event| {
            host_events.contains(event)
                || lit.contains(event)
                || p.u32(event - 4)
                    .is_ok_and(|class| !PRESENTATION.contains(&class))
        })
        .collect::<Vec<_>>();
    let name = |index: usize| crate::tiger::audio::bank::hash(&format!("reach/finish/{index}"));
    let root = sequence::Node {
        name: crate::tiger::audio::bank::hash("reach/finish/root"),
        timing: sequence::Timing::default(),
        kind: sequence::Kind::Parallel(
            kept.iter()
                .enumerate()
                .map(|(index, &event)| {
                    Ok(sequence::Node {
                        name: name(index),
                        timing: sequence::Timing::default(),
                        kind: sequence::Kind::Event(p.u32(event - 4)?),
                    })
                })
                .collect::<Result<_>>()?,
        ),
    };
    let written = sequence::write(p, allocation, &root, std::slice::from_ref(stock))?;
    for (index, &event) in kept.iter().enumerate() {
        sequence::adopt(p, written.events[&name(index)], event)?;
    }
    // The rebuilt controls and events are runtime records past the stock schema, which the
    // client's instance copy leaves out without a definition view after them.
    let end = p.0.len();
    crate::tiger::instance::seal_span(p, &mut Vec::new(), end)?;
    let removed = events
        .iter()
        .enumerate()
        .filter(|(_, event)| !kept.contains(event))
        .map(|(index, _)| u32::try_from(index))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(json!({"status":"translated","systems":systems.len(),
        "host_events":bursts.iter().take(systems.len()).map(|&(child, _)| child >> 16).collect::<Vec<_>>(),
        "kept_events":kept.len(),"detached_events":removed,
        "recolored_light_events":lit.len(),"host_seconds":life,"gameplay_verified":false}))
}

fn find_bursts(p: &Payload, controls: &[usize], events: &[usize]) -> Result<Vec<(u32, usize)>> {
    let mut bursts = Vec::new();
    for &control in controls {
        if !matches!(p.u32(control - 4)?, 0x808093d7 | 0x808093d9) || conditional(p, control) {
            continue;
        }
        for at in p.array(control + 0x38, 4, Some(0x808093fb))? {
            let child = p.u32(at)?;
            if child & 0xffff != 1 {
                continue;
            }
            let event = *events
                .get((child >> 16) as usize)
                .context("Native finish event")?;
            if p.u32(event - 4)? == 0x80806cc6
                && p.array(event + 0x40, 24, Some(0x80806cc8))?.len() == 1
            {
                bursts.push((child, event));
            }
        }
    }
    Ok(bursts)
}

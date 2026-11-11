//! A private launch sound follows an accepted native projectile spawn, and the vehicle's native
//! barrel fires at the source weapon's cadence and cone.
use super::super::secondary::Bundle;
use super::*;
use crate::halo_reach::{audio, cache::Cache};
use std::collections::BTreeSet;

/// Reach sound class 4, `weapon_fire`, from the sound tag's class byte (`snd! +0x2`, named by the
/// Assembly ReachMCC sound definition). Vehicle firing effects layer it with `weapon_fire_lod` (15),
/// the far-distance layer, and the first-person interior and exterior tails (58 and 59), and the
/// Scorpion's cannon adds its reload. The launch flow plays one cue, so it plays this layer.
const WEAPON_FIRE: u64 = 4;
/// Inputs a native Barrel definition's stat collection names, one `0x20`-byte row each from `+0x10`.
const BARREL_INPUTS: u32 = 74;

struct Source<'a> {
    sound: &'a Value,
    projectile: &'a Value,
    /// The firing layers the launch cue leaves out, empty for a single-layer effect.
    omitted: Vec<Value>,
}

/// The source address of the weapon's primary barrel, or none when its primary trigger names no
/// barrel, as a horn's does.
fn primary_barrel(node: &Value, cache: &Cache) -> Result<Option<usize>> {
    let triggers = node["data"]["object"]["gameplay"]["triggers"]
        .as_array()
        .context("Vehicle weapon triggers")?;
    // The launch sound follows the first primary trigger's primary barrel. Paired cannons, such
    // as the Seraph's, fire the same projectile and sound from each of their triggers and
    // barrels.
    let Some(primary) = triggers.iter().find(|t| t["input"] == 0) else {
        return Ok(None);
    };
    let Some(index) = primary["primary_barrel"].as_u64() else {
        return Ok(None);
    };
    let index = usize::try_from(index)?;
    let at = usize::try_from(
        node["tag"]["offset"]
            .as_u64()
            .context("Vehicle weapon source address")?,
    )?;
    let barrels = cache.block(at + 0x408, 0x184)?;
    barrels
        .get(index)
        .copied()
        .context("Primary barrel is outside source weapon")
        .map(Some)
}

/// The source weapon whose shot the vehicle's primary barrel fires: the one its converted
/// projectile section names, as a stationary gun's mounted weapon is, or else the driver's.
fn weapon_id(behavior: &Value, translated: Option<&Value>) -> Result<Option<u64>> {
    if let Some(id) = translated.and_then(|section| section["vehicle_weapon"]["datum"].as_u64()) {
        return Ok(Some(id));
    }
    crate::halo_reach::projectile::driver_weapon(behavior)?
        .map(|weapon| {
            weapon["tag"]["datum"]
                .as_u64()
                .context("Vehicle weapon identity")
        })
        .transpose()
}

fn primary<'a>(behavior: &'a Value, cache: &Cache, id: u64) -> Result<Option<Source<'a>>> {
    let edges = behavior["edges"]
        .as_array()
        .context("Vehicle weapon edges")?;
    let node = &behavior["nodes"][format!("{id:08X}")];
    let Some(barrel) = primary_barrel(node, cache)? else {
        return Ok(None);
    };
    let effects = cache.block(barrel + 0x178, 0xc4)?;
    let roots = edges
        .iter()
        .filter(|e| {
            e["owner"] == id
                && e["role"] == "fire"
                && e["offset"]
                    .as_u64()
                    .is_some_and(|offset| effects.iter().any(|row| offset == (*row + 4) as u64))
        })
        .map(|e| e["tag"]["datum"].as_u64().context("Primary firing effect"))
        .collect::<Result<Vec<_>>>()?;
    // A firing effect that layers several sounds plays its one `weapon_fire` layer. An effect with
    // none, or several, keeps no launch sound rather than refusing the vehicle.
    let layers = audio::fire_sound_layers(behavior, roots)?;
    let index = match layers.as_slice() {
        [] => return Ok(None),
        [_] => 0,
        _ => {
            let fire = layers
                .iter()
                .enumerate()
                .filter(|(_, sound)| sound["data"]["class"].as_u64() == Some(WEAPON_FIRE))
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            let [index] = fire.as_slice() else {
                return Ok(None);
            };
            *index
        }
    };
    let sound = layers[index];
    let omitted = layers
        .iter()
        .enumerate()
        .filter(|&(other, _)| other != index)
        .map(|(_, layer)| json!({"source":layer["tag"],"class":layer["data"]["class"]}))
        .collect::<Vec<_>>();
    let projectile = cache
        .reference(barrel + 0x104, Some("proj"))?
        .context("Source vehicle primary projectile")?;
    let projectile = &behavior["nodes"][format!("{:08X}", projectile.datum)]["data"];
    ensure!(
        projectile.is_object(),
        "Source vehicle projectile behavior is absent"
    );
    Ok(Some(Source {
        sound,
        projectile,
        omitted,
    }))
}

/// The layers a launch report leaves out, with the reason, or null when the effect has one layer.
fn omitted_layers(source: Option<&Source>) -> Value {
    match source {
        Some(source) if !source.omitted.is_empty() => json!({"layers":source.omitted,
            "reason":"The launch flow plays one cue, the weapon_fire layer, so the other layers, such as the far-distance layer and the first-person tails, are not played"}),
        _ => Value::Null,
    }
}

impl Assembly {
    /// The vehicle's primary barrel fires the source weapon's projectile and sound. With
    /// `translated`, the projectile section the import converted from the source weapon, the
    /// barrel fires that projectile, which hosts the launch sound. Without it a private copy of
    /// the carrier's projectile takes the source launch motion and hosts the sound.
    pub fn fire_audio(
        &mut self,
        native: &mut Reader,
        graph: &mut Graph,
        behavior: &Value,
        cache: &Cache,
        (bus, with_sound): (Option<u32>, bool),
        translated: Option<&Value>,
    ) -> Result<Value> {
        // Source PCM exists only when the import exported the vehicle's audio.
        let weapon = weapon_id(behavior, translated)?;
        let source = match weapon {
            Some(id) if with_sound => primary(behavior, cache, id)?,
            _ => None,
        };
        if source.is_none() && translated.is_none() {
            return Ok(json!({"status":"source_only","reason":"No source primary firing sound"}));
        }
        let mut barrels = Vec::new();
        for (&owner, payload) in &self.payloads {
            for definition in (0..payload.0.len().saturating_sub(0xb94)).step_by(8) {
                if payload.u32(definition)? != owner || payload.u32(definition + 4)? != 0x80803889 {
                    continue;
                }
                let instance = usize::try_from(payload.u64(definition + 8)?)?;
                if instance + 16 > payload.0.len()
                    || payload.u32(instance)? != owner
                    || payload.u32(instance + 4)? != 0x80803865
                    || payload.u64(instance + 8)? != definition as u64
                {
                    continue;
                }
                let tag = payload.u32(definition + 0xb80)?;
                ensure!(
                    tag == payload.u32(definition + 0xb90)? && ![0, u32::MAX].contains(&tag),
                    "Vehicle barrel has different native projectile routes"
                );
                barrels.push((owner, definition, tag));
            }
        }
        ensure!(
            barrels.len() == 1,
            "Vehicle primary native barrel is absent or ambiguous"
        );
        let (owner, definition, projectile) = barrels[0];
        ensure!(
            native.tag(projectile, Some(0x80809c0f))?.u16(0x96)? == 18,
            "Vehicle firing route is not a projectile"
        );
        let firing = self.firing(owner, definition, behavior, weapon)?;
        let layers = omitted_layers(source.as_ref());
        if let Some(section) = translated {
            let mut report = self.fire_translated(
                native,
                graph,
                source.as_ref(),
                bus,
                section,
                owner,
                definition,
                projectile,
            )?;
            report["firing"] = firing;
            report["omitted_sound_layers"] = layers;
            return Ok(report);
        }
        let Some(source) = source else {
            return Ok(
                json!({"status":"source_only","reason":"No source primary firing sound","firing":firing}),
            );
        };
        let (host, flow, field, source_cue) = template(native)?;
        let report =
            audio::once::cue(native, graph, source.sound, source_cue, bus, "vehicle-fire")?;
        let mut bundle = Bundle::read(native, projectile)?;
        let motion = crate::halo_reach::projectile::motion::apply(
            &bundle.entity().clone(),
            &mut bundle.owners,
            source.projectile,
            crate::halo_reach::rig::METRES_PER_UNIT,
            false,
        )?;
        bundle.add_unconnected(native, flow, host)?;
        bundle
            .resources
            .entry(flow)
            .or_default()
            .insert(field, "vehicle-fire-cue".into());
        let symbol = bundle.emit(graph, "vehicle-fire-projectile")?;
        for offset in [0xb80, 0xb90] {
            self.resource(owner, definition + offset, symbol.clone())?;
        }
        Ok(
            json!({"sound":report,"motion":motion,"projectile":symbol,"native_projectile":format!("{projectile:08X}"),
            "firing":firing,"omitted_sound_layers":layers,
            "trigger":"Native accepted projectile spawn","gameplay_verified":false,
            "limits":["Source initial gravity scale and range are adapted on the private native projectile. Native launch inputs, later motion curves, damage and visuals are retained. Source primary firing PCM uses a private global voice limit. Source pitch curves, spatial attenuation and secondary firing are not translated."]}),
        )
    }
}

/// The member a native Barrel definition at `definition` reads `input` from, when its stat
/// collection row names a kind 0 numeric member, whose Float32 at `+04` component initialization
/// reads. A row naming none, or another kind, holds no literal to write.
fn member(p: &Payload, definition: usize, input: u32) -> Result<Option<usize>> {
    let row = definition + 0x10 + usize::try_from(input)? * 0x20;
    if p.u64(row)? == 0 {
        return Ok(None);
    }
    let at = p.pointer(row)?;
    Ok((at > definition && at + 8 <= p.0.len() && p.u8(at)? == 0).then_some(at))
}

impl Assembly {
    /// The source driver weapon's cadence and cone on the vehicle's native barrel. No stat
    /// translator routes a vehicle weapon's Barrel inputs, so each one the source sets is written as
    /// the barrel definition's own literal, as a held weapon's Charge inputs are where no
    /// translator writes them. The receipt is the source firing's, with what stays native.
    fn firing(
        &mut self,
        owner: u32,
        definition: usize,
        behavior: &Value,
        weapon: Option<u64>,
    ) -> Result<Value> {
        let Some(id) = weapon else {
            return Ok(
                json!({"status":"native","reason":"The vehicle has no source driver weapon"}),
            );
        };
        let gameplay = &behavior["nodes"][format!("{id:08X}")]["data"]["object"]["gameplay"];
        let payload = self
            .payloads
            .get_mut(&owner)
            .context("Vehicle barrel owner")?;
        let mut writable = BTreeSet::new();
        for input in 0..BARREL_INPUTS {
            if member(payload, definition, input)?.is_some() {
                writable.insert((2, input));
            }
        }
        let (receipt, inputs) = crate::halo_reach::firing::vehicle(gameplay, &writable)?;
        for literal in &inputs {
            ensure!(
                literal["component"] == 2,
                "Vehicle firing writes another component"
            );
            let input = u32::try_from(literal["input"].as_u64().context("Firing literal input")?)?;
            let value = literal["value"].as_f64().context("Firing literal value")? as f32;
            ensure!(value.is_finite(), "Nonfinite vehicle firing literal");
            let at = member(payload, definition, input)?
                .context("Vehicle firing literal has no member")?;
            put(&mut payload.0, at + 4, &value.to_le_bytes())?;
        }
        Ok(receipt)
    }
}

/// Launch multiplier configurations of a native Barrel definition. A projectile that flies its
/// source speed curve holds them at one, as a held weapon's build does for its Barrel.
const LAUNCH_CONFIGS: std::ops::RangeInclusive<usize> = 43..=47;

/// The stock once-sound flow the vehicle's firing sound plays from, its host entity, its cue field
/// and the stock cue there. The flow has no input connections, so its complete registration and
/// instance allocation can join another entity without changing that entity's behavior.
fn template(native: &mut Reader) -> Result<(u32, u32, usize, u32)> {
    let host = 0x80fc7616;
    let entity = native.tag(host, Some(0x80809c0f))?;
    let mut candidates = Vec::new();
    for row in entity.array(16, 12, Some(0x80809c04))? {
        let tag = entity.u32(row)?;
        let p = native.tag(tag, Some(0x80809c36))?;
        let d = usize::try_from(p.u64(p.pointer(16)? + 8)?)?;
        if p.u32(d - 4)? != 0x808084e9 {
            continue;
        }
        ensure!(
            p.array(d + 0x188, 40, Some(0x80809789))?.is_empty(),
            "Native launch sound needs inputs"
        );
        let events = p.array(d + 0x168, 24, Some(0x808093e6))?;
        ensure!(
            events.len() == 1,
            "Native launch sound flow is not a single event"
        );
        let event = p.pointer(events[0] + 16)?;
        ensure!(
            p.u32(event - 4)? == 0x80806b38,
            "Native launch event is not a once sound"
        );
        candidates.push((tag, event + 0x40, p.u32(event + 0x40)?));
    }
    let [(flow, field, cue)] = candidates[..] else {
        anyhow::bail!("Native launch sound template is absent or ambiguous");
    };
    Ok((host, flow, field, cue))
}

/// The launch sound a vehicle's translated projectile hosts, when the vehicle's source weapon has
/// a primary firing sound. The weapon is `weapon`'s, a stationary gun's mounted weapon edge, or
/// else the driver's. The cue it names is the one `fire_audio` writes.
pub(in crate::halo_reach) fn launch(
    native: &mut Reader,
    behavior: &Value,
    cache: &Cache,
    weapon: Option<&Value>,
) -> Result<Option<crate::halo_reach::projectile::Launch>> {
    let id = match weapon {
        Some(weapon) => Some(
            weapon["tag"]["datum"]
                .as_u64()
                .context("Mounted weapon identity")?,
        ),
        None => weapon_id(behavior, None)?,
    };
    let Some(id) = id else {
        return Ok(None);
    };
    if primary(behavior, cache, id)?.is_none() {
        return Ok(None);
    }
    let (host, flow, field, _) = template(native)?;
    Ok(Some(crate::halo_reach::projectile::Launch {
        host,
        flow,
        field,
        cue: "vehicle-fire-cue".into(),
    }))
}

impl Assembly {
    #[allow(clippy::too_many_arguments)]
    fn fire_translated(
        &mut self,
        native: &mut Reader,
        graph: &mut Graph,
        source: Option<&Source>,
        bus: Option<u32>,
        section: &Value,
        owner: u32,
        definition: usize,
        carrier: u32,
    ) -> Result<Value> {
        let symbol = section["root"]
            .as_str()
            .filter(|symbol| !symbol.is_empty())
            .context("Translated vehicle projectile root")?
            .to_owned();
        // The projectile hosts the launch flow only when the source weapon has a firing sound, so
        // the cue it names is written exactly then.
        let sound = source
            .map(|source| -> Result<Value> {
                let (_, _, _, cue) = template(native)?;
                audio::once::cue(native, graph, source.sound, cue, bus, "vehicle-fire")
            })
            .transpose()?;
        for offset in [0xb80, 0xb90] {
            self.resource(owner, definition + offset, symbol.clone())?;
        }
        let trajectory = section["conversion"]["launch_policy"] == "source_player_trajectory";
        if trajectory {
            let payload = self
                .payloads
                .get_mut(&owner)
                .context("Vehicle barrel owner")?;
            for index in LAUNCH_CONFIGS {
                let config = definition + 0x10 + index * 0x20;
                put(&mut payload.0, config, &0u64.to_le_bytes())?;
                put(&mut payload.0, config + 12, &1f32.to_le_bytes())?;
            }
        }
        Ok(
            json!({"sound":sound,"projectile":symbol,"native_projectile":format!("{carrier:08X}"),
            "source_weapon":section["vehicle_weapon"],"launch_policy":section["conversion"]["launch_policy"],
            "launch_multipliers":if trajectory { "Held at one for the source speed curve" } else { "Retained" },
            "trigger":"Native accepted projectile spawn","gameplay_verified":false,
            "limits":["The vehicle's native barrel fires the source weapon's converted projectile, whose report lists what it translates. The firing receipt says which rate and cone the barrel takes. Its ammunition and heat stay native."]}),
        )
    }
}

//! A private launch sound follows an accepted native projectile spawn.
use super::super::secondary::Bundle;
use super::*;
use crate::halo_reach::{audio, cache::Cache};

struct Source<'a> {
    sound: &'a Value,
    projectile: &'a Value,
}

fn primary_barrel(node: &Value, cache: &Cache) -> Result<usize> {
    let triggers = node["data"]["object"]["gameplay"]["triggers"]
        .as_array()
        .context("Vehicle weapon triggers")?;
    let primary = triggers
        .iter()
        .filter(|t| t["input"] == 0)
        .collect::<Vec<_>>();
    ensure!(
        primary.len() == 1 && primary[0]["secondary_barrel"] == -1,
        "Vehicle primary input needs a unique single barrel"
    );
    let index = usize::try_from(
        primary[0]["primary_barrel"]
            .as_u64()
            .context("Primary barrel index")?,
    )?;
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
}

fn primary<'a>(behavior: &'a Value, cache: &Cache) -> Result<Option<Source<'a>>> {
    let edges = behavior["edges"]
        .as_array()
        .context("Vehicle weapon edges")?;
    let root = behavior["root"]["datum"]
        .as_u64()
        .context("Vehicle source identity")?;
    let Some(weapon) = edges
        .iter()
        .filter(|e| e["owner"] == root && e["role"] == "vehicle_weapon")
        .min_by_key(|e| e["offset"].as_u64())
    else {
        return Ok(None);
    };
    let id = weapon["tag"]["datum"]
        .as_u64()
        .context("Vehicle weapon identity")?;
    let node = &behavior["nodes"][format!("{id:08X}")];
    let barrel = primary_barrel(node, cache)?;
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
    let Some(sound) = audio::fire_sound_from(behavior, roots)? else {
        return Ok(None);
    };
    let projectile = cache
        .reference(barrel + 0x104, Some("proj"))?
        .context("Source vehicle primary projectile")?;
    let projectile = &behavior["nodes"][format!("{:08X}", projectile.datum)]["data"];
    ensure!(
        projectile.is_object(),
        "Source vehicle projectile behavior is absent"
    );
    Ok(Some(Source { sound, projectile }))
}

impl Assembly {
    pub fn fire_audio(
        &mut self,
        native: &mut Reader,
        graph: &mut Graph,
        behavior: &Value,
        cache: &Cache,
        bus: Option<u32>,
    ) -> Result<Value> {
        let Some(source) = primary(behavior, cache)? else {
            return Ok(json!({"status":"source_only","reason":"No source primary firing sound"}));
        };
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
        // This native once-flow has no input connections. Its complete registration
        // and instance allocation are added without changing projectile behavior.
        let host = 0x80fc7616;
        let template = native.tag(host, Some(0x80809c0f))?;
        let mut candidates = Vec::new();
        for row in template.array(16, 12, Some(0x80809c04))? {
            let tag = template.u32(row)?;
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
        ensure!(
            candidates.len() == 1,
            "Native launch sound template is absent or ambiguous"
        );
        let (flow, field, source_cue) = candidates[0];
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
            "trigger":"Native accepted projectile spawn","gameplay_verified":false,
            "limits":["Source initial gravity scale and range are adapted on the private native projectile. Native launch inputs, later motion curves, damage and visuals are retained. Source primary firing PCM uses a private global voice limit. Source pitch curves, spatial attenuation and secondary firing are not translated."]}),
        )
    }
}

//! Clone the complete timed effect and retarget its two spatial queries.
use super::source::{Radial, Source};
use crate::{
    halo_reach::blast,
    presentation::{Graph, put},
    tiger::{payload::Payload, projectile::Assets, reader::Reader},
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn query(p: &mut Payload, q: usize, radial: &Radial<'_>, metres: f32, profile: u32) -> Result<()> {
    let start = radial.radius[0] / radial.radius[1];
    let radius = radial.radius[1] * metres;
    blast::retarget(p, q, start, Some(radius), profile)
}

pub(super) fn build(
    native: &mut Reader,
    assets: &mut Assets,
    source: &Source<'_>,
    metres: f32,
    health: u32,
    impulse: u32,
    sounds: &mut Graph,
) -> Result<(u32, Value)> {
    let mut copy = blast::Copy::begin(native, assets, 0x81a6985b, "sword-effect")?;
    let mut found = BTreeSet::new();
    let mut impact = None;
    for p in copy.converted.values_mut() {
        let d = p.pointer(24)?;
        if p.u32(d - 4)? != 0x808084e9 {
            continue;
        }
        ensure!(
            p.u8(d + 0x2e0)? & 2 != 0,
            "Melee effect lacks native root retirement"
        );
        for event in p.array(d + 0x168, 24, Some(0x808093e6))? {
            let q = p.pointer(event + 16)?;
            if p.u32(q - 4)? == 0x80806b38
                && let Some(sound) = source.sound
            {
                ensure!(
                    impact.is_none(),
                    "Melee effect has multiple impact sound events"
                );
                // This bus is declared by the native pulse's primary sound layers.
                // The same event also has environment layers on other buses.
                let report = crate::halo_reach::audio::once::cue(
                    native,
                    sounds,
                    sound,
                    p.u32(q + 0x40)?,
                    Some(0x2ce93fb5),
                    "sword-impact",
                )?;
                let cue = assets.external(native, "sword-impact-cue")?;
                put(&mut p.0, q + 0x40, &cue.to_le_bytes())?;
                impact = Some(report);
            }
            if p.u32(q - 4)? != 0x80804ab9 {
                continue;
            }
            let profile = p.u32(blast::action(p, q)? + 8)?;
            ensure!(found.insert(profile), "Melee pulse repeats a profile");
            match profile {
                0x81a6985f => query(p, q, &source.blast, metres, health)?,
                0x81a69860 => query(p, q, &source.push, metres, impulse)?,
                _ => anyhow::bail!("Melee pulse has an unsupported outgoing profile"),
            }
        }
    }
    ensure!(
        found.len() == 2,
        "Melee pulse is missing damage or recipient push"
    );
    ensure!(
        source.sound.is_none() || impact.is_some(),
        "Melee effect has no impact sound event"
    );
    let root_id = copy.finish(native, assets, "sword-timed-blast")?;
    Ok((
        root_id,
        impact.unwrap_or_else(
            || json!({"status":"native","reason":"Source audio is not configured"}),
        ),
    ))
}

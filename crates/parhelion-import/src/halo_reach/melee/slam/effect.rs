//! Clone the complete timed effect and retarget its two spatial queries.
use super::source::{Radial, Source};
use crate::{
    halo_reach::projectile,
    presentation::{Graph, put},
    tiger::{entity::compiled, payload::Payload, projectile::Assets, reader::Reader},
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn query(p: &mut Payload, q: usize, radial: &Radial<'_>, metres: f32, profile: u32) -> Result<()> {
    let radius = radial.radius[1] * metres;
    ensure!(
        radius.is_finite() && radius > 0.,
        "Invalid translated melee radius"
    );
    ensure!(
        p.u8(q + 0x30)? == 0x1d
            && p.u32(q + 0xa0)? == 0
            && p.u64(q + 0xd0)? == 1
            && p.u64(q + 0xd8)? == 1
            && p.u64(q + 0x160)? == 0,
        "Native melee query uses another weight policy"
    );
    let shape = p.pointer(q + 0x48)?;
    ensure!(
        p.u32(shape - 4)? == 0x808092b1 && p.u32(shape)? == 2,
        "Native melee pulse requires a sphere query"
    );
    put(&mut p.0, shape + 4, &radius.to_le_bytes())?;
    let code = p.array(q + 0xb0, 1, Some(0x80800009))?;
    let expected = [
        0x3c, 0, 0x22, 0, 0x23, 0x22, 0, 0x37, 0, 0x23, 0x22, 0, 0x3e, 0,
    ];
    ensure!(
        code.len() == expected.len()
            && code.iter().zip(expected).all(|(&at, byte)| p.0[at] == byte),
        "Native melee pulse has another spatial program"
    );
    let constants = p.array(q + 0xc0, 16, Some(0x80800090))?;
    ensure!(
        constants.len() == 5,
        "Native melee pulse has another spline layout"
    );
    let start = radial.radius[0] / radial.radius[1];
    let slope = 1. / (1. - start);
    ensure!(
        start < 1. && slope.is_finite(),
        "Melee falloff interval is too narrow"
    );
    // This explicit translation uses the source inner/outer radii with a linear
    // attenuation between them. The Reach damage caller's exponent mapping is
    // retained in the receipt until its recipient input is independently joined.
    let values = [
        [0.; 4],
        [0.; 4],
        [0., -slope, 0., 0.],
        [1., slope, 0., 0.],
        [0., start, 1., 1.],
    ];
    for (&at, row) in constants.iter().zip(values) {
        for (lane, value) in row.into_iter().enumerate() {
            put(&mut p.0, at + lane * 4, &value.to_le_bytes())?
        }
    }
    let actions = p.array(q + 0x178, 0x58, Some(0x80804abf))?;
    let [action] = actions.as_slice() else {
        anyhow::bail!("Native melee query needs one outgoing action")
    };
    let action = p.pointer(action + 0x50)?;
    ensure!(
        p.u32(action - 4)? == 0x80804ac9,
        "Native melee query uses another action"
    );
    put(&mut p.0, action + 8, &profile.to_le_bytes())?;
    Ok(())
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
    let carrier = 0x81a6985b;
    let original = native.tag(carrier, Some(0x80809c0f))?;
    let rows = original.array(16, 12, Some(0x80809c04))?;
    let mut owners = BTreeMap::new();
    let mut ids = BTreeMap::new();
    let mut allocations = BTreeMap::new();
    for &row in &rows {
        let tag = original.u32(row)?;
        let p = native.tag(tag, Some(0x80809c36))?;
        ensure!(
            owners.insert(tag, (*p).clone()).is_none(),
            "Melee effect repeats a component"
        );
        ids.insert(
            tag,
            assets.reserve(native, format!("sword-effect-component-{tag:08X}"))?,
        );
        let allocation = p.u32(0x44)?;
        if let std::collections::btree_map::Entry::Vacant(slot) = allocations.entry(allocation) {
            slot.insert(
                assets.reserve(native, format!("sword-effect-allocation-{allocation:08X}"))?,
            );
        }
    }
    let mut converted = owners.clone();
    let mut found = BTreeSet::new();
    let mut impact = None;
    for p in converted.values_mut() {
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
            let actions = p.array(q + 0x178, 0x58, Some(0x80804abf))?;
            let [action] = actions.as_slice() else {
                anyhow::bail!("Melee pulse action is ambiguous")
            };
            let a = p.pointer(action + 0x50)?;
            let profile = p.u32(a + 8)?;
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
    for (&tag, p) in &mut converted {
        projectile::relocate(p, &owners, &ids, &[])?;
        let allocation = allocations[&owners[&tag].u32(0x44)?];
        put(&mut p.0, 0x44, &allocation.to_le_bytes())?;
        assets.push(ids[&tag], tag, p.clone(), None)?;
    }
    for (&tag, &id) in &allocations {
        assets.push(id, tag, (*native.tag(tag, Some(0x80809bbb))?).clone(), None)?;
    }
    let root_id = assets.reserve(native, "sword-timed-blast".into())?;
    let replication = assets.reserve(native, "sword-effect-replication".into())?;
    let components = rows
        .iter()
        .map(|&row| {
            let tag = original.u32(row)?;
            Ok(compiled::Component {
                owner: ids[&tag],
                payload: &converted[&tag],
                template: &owners[&tag],
                entity: &original,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let replication_template = original.u32(0x88)?;
    native.tag(replication_template, Some(0x80809bb6))?;
    assets.push(
        replication,
        replication_template,
        compiled::replication::emit(root_id, &components)?,
        None,
    )?;
    let mut entity = (*original).clone();
    projectile::relocate(&mut entity, &owners, &ids, &rows)?;
    put(&mut entity.0, 0x88, &replication.to_le_bytes())?;
    assets.push(root_id, carrier, entity, None)?;
    Ok((
        root_id,
        impact.unwrap_or_else(
            || json!({"status":"native","reason":"Source audio is not configured"}),
        ),
    ))
}

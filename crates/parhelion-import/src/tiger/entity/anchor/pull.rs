//! Native contact impulse toward an anchor, separate from the modern Grapple solver.
//!
//! `4B57 +1` admits signed strength in `B7C050`. Its negative vector reaches the
//! native `971D` movement listener. `4D7C` compares inherited attribution, while
//! `4D72` restricts the query to bipeds. Keeping both excludes the caster's other
//! objects. Creation, spatial acquisition and receiving authority still gate it.
use super::{object, pair, put, relative};
use crate::tiger::payload::{Payload, write_array};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

pub struct Prepared {
    pub projectile: Payload,
    pub profile: Payload,
    pub query: Payload,
    pub evidence: Value,
}

/// Compose a contact pulse using native profile and query envelopes. This is a
/// discrete impulse, with no retained rope constraint, steering or swing solver.
pub fn prepare(
    projectile: &Payload,
    native_projectile: &Payload,
    neutral_profile: &Payload,
    query: &Payload,
    pulse_tag: u32,
    profile_tag: u32,
) -> Result<Prepared> {
    ensure!(
        ![0, u32::MAX].contains(&pulse_tag) && ![0, u32::MAX].contains(&profile_tag),
        "native pull resource is absent"
    );
    let (_, pd) = pair(neutral_profile, 0x80804B01, 0x80804B02)?;
    let mut neutral = [0u8; 0x100];
    for offset in [0x14, 0x18] {
        let amount = neutral_profile.f32(pd + offset)?;
        neutral[offset - 0x10..offset - 0x0C].copy_from_slice(&amount.to_le_bytes());
    }
    for offset in [0x4C, 0x74, 0xD0] {
        neutral[offset - 0x10..offset - 0x0C].copy_from_slice(&1.0f32.to_le_bytes());
    }
    neutral[0x55 - 0x10] = 1;
    neutral[0xB0 - 0x10..0xB4 - 0x10].copy_from_slice(&u32::MAX.to_le_bytes());
    ensure!(
        neutral_profile.bytes::<0x100>(pd + 0x10)? == neutral,
        "native pull profile must have an absolute, nonrandom neutral envelope"
    );
    let mut profile = neutral_profile.clone();
    put(&mut profile, pd + 0x14, &[0; 12])?;
    let mut response = [0u8; 0x28];
    response[1] = 1;
    response[4..8].copy_from_slice(&(-24.0f32).to_le_bytes());
    response[8..12].copy_from_slice(&(-24.0f32).to_le_bytes());
    response[0x10..0x14].copy_from_slice(&1.0f32.to_le_bytes());
    response[0x20..0x24].copy_from_slice(&u32::MAX.to_le_bytes());
    let response = object(&mut profile, 0x80804B57, &response);
    write_array(&mut profile.0, pd + 0x78, 0x80804B3D, 1, &[0; 8])?;
    let slot = profile.array(pd + 0x78, 8, Some(0x80804B3D))?[0];
    relative(&mut profile, slot, response)?;

    let (mut configured, query_count) = configure_query(query, profile_tag)?;

    let (_, nd) = pair(native_projectile, 0x80803B73, 0x8080388F)?;
    let mut template = None;
    for group in native_projectile.array(nd + 0x218, 88, Some(0x808037CD))? {
        for node in native_projectile.array(group + 0x48, 392, Some(0x808037D0))? {
            for action in native_projectile.array(node + 0x178, 16, Some(0x808037F7))? {
                if native_projectile.u8(action + 8)? != 16 {
                    continue;
                }
                let at = native_projectile.pointer(action)?;
                ensure!(
                    native_projectile.u32(at - 4)? == 0x808037DB
                        && [0x78, 0x80, 0x88]
                            .into_iter()
                            .all(|offset| { native_projectile.u64(at + offset).ok() == Some(0) }),
                    "native contact spawn has unsupported embedded references"
                );
                template = Some(native_projectile.bytes::<0x90>(at)?);
                break;
            }
            if template.is_some() {
                break;
            }
        }
        if template.is_some() {
            break;
        }
    }
    let mut template = template.context("native projectile has no contact spawn template")?;
    template[8..12].copy_from_slice(&pulse_tag.to_le_bytes());
    let (_, ad) = pair(projectile, 0x80803B73, 0x8080388F)?;
    let mut attached = projectile.clone();
    let spawn = object(&mut attached, 0x808037DB, &template);
    let mut contacts = 0;
    for group in projectile.array(ad + 0x218, 88, Some(0x808037CD))? {
        for node in projectile.array(group + 0x48, 392, Some(0x808037D0))? {
            let actions = projectile.array(node + 0x178, 16, Some(0x808037F7))?;
            ensure!(
                actions.len() == 1 && projectile.u8(actions[0] + 8)? == 1,
                "native pull requires a prepared placement-only anchor"
            );
            let placement = projectile.pointer(actions[0])?;
            ensure!(
                projectile.u32(placement - 4)? == 0x808037E2,
                "native pull anchor placement type differs"
            );
            write_array(&mut attached.0, node + 0x178, 0x808037F7, 2, &[0; 32])?;
            let rows = attached.array(node + 0x178, 16, Some(0x808037F7))?;
            relative(&mut attached, rows[0], placement)?;
            relative(&mut attached, rows[1], spawn)?;
            put(&mut attached, rows[0] + 8, &[1])?;
            put(&mut attached, rows[1] + 8, &[16])?;
            contacts += 1;
        }
    }
    ensure!(contacts > 0, "native pull anchor has no contact actions");
    for payload in [&mut attached, &mut configured, &mut profile] {
        let size = payload.0.len() as u64;
        put(payload, 0, &size.to_le_bytes())?;
    }
    Ok(Prepared {
        projectile: attached,
        profile,
        query: configured,
        evidence: json!({
            "native_build":"86657.20.08.23.1800.d2_rc___release",
            "motion":"signed contact impulse", "strength":-24.0, "radius":50.0,
            "health_amount":0.0, "signed_strength":true,
            "target_predicates":["80804D7C","80804D72"], "biped_mask":1u32 << 12,
            "active_queries":1, "inactive_queries":query_count-1,
            "contact_nodes":contacts, "contact_selectors":[1,16],
            "limits":["Not the modern Grapple solver or a sustained pull",
                "Contact spawn requires a valid accepted hit object",
                "Each qualifying contact can create a pulse",
                "Native creation, authority, spatial acquisition and recipient acceptance still gate motion",
                "Live activation, repeated use, cancellation and subclass cleanup are unverified"]
        }),
    })
}

fn configure_query(query: &Payload, profile_tag: u32) -> Result<(Payload, usize)> {
    let (_, qd) = pair(query, 0x808084D7, 0x808084E9)?;
    let mut configured = query.clone();
    let mut queries = Vec::new();
    for row in query.array(qd + 0x168, 24, Some(0x808093E6))? {
        let target = query.pointer(row + 0x10)?;
        if target >= 4 && query.u32(target - 4)? == 0x80804AB9 {
            queries.push(target);
        }
    }
    ensure!(!queries.is_empty(), "native pull sequence has no query");
    for (index, &node) in queries.iter().enumerate() {
        let shape = query.pointer(node + 0x48)?;
        ensure!(
            query.u32(shape - 4)? == 0x808092B1 && query.u8(shape)? == 2,
            "native pull query must use a sphere"
        );
        let actions = query.array(node + 0x178, 0x58, Some(0x80804ABF))?;
        ensure!(actions.len() == 1, "native pull query must have one action");
        let action = query.pointer(actions[0] + 0x50)?;
        ensure!(
            query.u32(action - 4)? == 0x80804AC9,
            "native pull query action must apply an outgoing profile"
        );
        put(&mut configured, action + 8, &profile_tag.to_le_bytes())?;
        if index != 0 {
            // Retain the native sequence and compiled registrations, while making
            // its other queries unable to issue damage or attachment actions.
            put(&mut configured, node + 0x178, &[0; 16])?;
            continue;
        }
        put(&mut configured, shape + 4, &50.0f32.to_le_bytes())?;
        put(&mut configured, node + 0x50, &[0])?;
        let owner = object(&mut configured, 0x80804D7C, &[0]);
        let biped = object(&mut configured, 0x80804D72, &(1u32 << 12).to_le_bytes());
        write_array(&mut configured.0, node + 0x58, 0x80809316, 2, &[0; 32])?;
        let terms = configured.array(node + 0x58, 16, Some(0x80809316))?;
        relative(&mut configured, terms[0] + 8, owner)?;
        relative(&mut configured, terms[1] + 8, biped)?;
    }
    Ok((configured, queries.len()))
}

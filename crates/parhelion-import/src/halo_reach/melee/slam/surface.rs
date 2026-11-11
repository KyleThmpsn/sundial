//! A private copy of the sword contacts' surface impact table that names the source hit cue.
//!
//! Every `80804455` contact of the sword owner names one surface table at `+38`. The table
//! holds per-surface particle systems and `80800014` lists of `80809802` sound cues. The copy
//! names the source impact cue in every cue slot and keeps the native particles, so a contact
//! hit sounds like the source weapon on every surface.
use crate::{
    presentation::{Graph, put},
    tiger::{payload::Payload, reader::Reader},
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;

const CONTACT: u32 = 0x8080_4455;
const SURFACE: usize = 0x38;
const CUE: u32 = 0x8080_9802;

pub(super) fn build(
    native: &mut Reader,
    owner: &Payload,
    graph: &mut Graph,
    cue: &str,
) -> Result<(u32, Value)> {
    let mut tables = BTreeSet::new();
    let mut contacts = 0;
    for at in (4..owner.0.len().saturating_sub(SURFACE + 4)).step_by(4) {
        if owner.u32(at - 4)? == CONTACT {
            tables.insert(owner.u32(at + SURFACE)?);
            contacts += 1;
        }
    }
    let [table] = tables.into_iter().collect::<Vec<_>>()[..] else {
        anyhow::bail!("Sword contacts name no surface table or several");
    };
    let payload = native.tag(table, None)?;
    let mut bytes = payload.0.to_vec();
    let mut patches = Vec::new();
    for at in (0..payload.0.len().saturating_sub(3)).step_by(4) {
        let word = payload.u32(at)?;
        if (0x80A0_0000..0x81F0_0000).contains(&word) && native.tag(word, Some(CUE)).is_ok() {
            // Linking writes the cue's private tag only over an FFFFFFFF placeholder.
            put(&mut bytes, at, &u32::MAX.to_le_bytes())?;
            patches.push(json!({"offset":at,"symbol":cue}));
        }
    }
    ensure!(
        !patches.is_empty(),
        "Sword surface table names no sound cue"
    );
    graph.add("sword-surface", table, &bytes, None, patches.clone())?;
    Ok((
        table,
        json!({"source_table":format!("{table:08X}"),"contacts":contacts,"replaced_cues":patches.len(),
            "retained":"Native per-surface particles"}),
    ))
}

use super::source::Capture;
use crate::d2_mot::ability::{self, Contract, DamageType, Selection, source::number};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Deserialize, Serialize)]
pub struct Request {
    pub item_hash: u32,
    /// Explicit intent, not a claim that a source damage provider was decoded.
    #[serde(rename = "element")]
    pub damage_type: Option<DamageType>,
    pub selections: Vec<Selection>,
}

impl Request {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            !self.selections.is_empty(),
            "Select at least one source ability or passive"
        );
        let mut sockets = BTreeSet::new();
        for selection in &self.selections {
            ensure!(
                sockets.insert(selection.socket_index),
                "Select only one plug per source socket"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Input {
    pub request: Request,
    pub capture: Capture,
}

#[derive(Debug, Serialize)]
pub struct Damage {
    pub requested: Option<DamageType>,
    /// Source presentation continues to select Strand effects when gameplay uses Kinetic.
    pub presentation: Option<DamageType>,
    /// Representable native damage type only. No damage profiles have been emitted or verified.
    pub native: Option<DamageType>,
    pub policy: DamagePolicy,
    pub source_providers_resolved: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DamagePolicy {
    #[serde(rename = "native_element")]
    NativeDamageType,
    StrandAsKinetic,
    Unresolved,
}

#[derive(Debug, Serialize)]
pub struct Plan {
    pub abilities: Vec<ability::Plan>,
    /// Base-item effects are distinct from effects granted by selected socket plugs.
    pub innate: Option<ability::Runtime>,
    pub damage: Damage,
    pub pending: Vec<Contract>,
}

/// Validate source membership and derive outstanding translation work without mutating assets.
/// This deliberately assigns no native list entries, ability rows, property keys or package IDs.
pub fn plan(input: &Input) -> Result<Plan> {
    crate::cancellation::check()?;
    input.request.validate()?;
    input.capture.validate()?;
    ensure!(
        input.request.item_hash == input.capture.item_hash,
        "Capture belongs to another source item"
    );
    let mut traces = BTreeMap::new();
    for trace in &input.capture.traces {
        let plug = number(&trace["plug_hash"], "trace plug hash")?;
        ensure!(
            traces.insert(plug, trace).is_none(),
            "Duplicate source plug trace"
        );
    }
    let mut used = BTreeSet::new();
    let abilities = input
        .request
        .selections
        .iter()
        .map(|selection| {
            crate::cancellation::check()?;
            let choice = input
                .capture
                .choice(selection.socket_index, selection.plug_hash)?;
            let trace = traces.get(&selection.plug_hash).ok_or_else(|| {
                anyhow::anyhow!(
                    "Missing trace for selected plug {:08X}",
                    selection.plug_hash
                )
            })?;
            used.insert(selection.plug_hash);
            ability::plan(
                selection,
                &choice.tag,
                choice.category,
                trace,
                &input.capture.animations,
                input.capture.ability_catalog.as_ref(),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        used.len() == traces.len(),
        "Capture contains traces outside the requested selection"
    );
    let innate = input
        .capture
        .innate_trace
        .as_ref()
        .map(|trace| {
            ability::runtime(
                input.capture.item_hash,
                &input.capture.item_tag,
                trace,
                &input.capture.animations,
                input.capture.ability_catalog.as_ref(),
            )
        })
        .transpose()?;
    let inspected = abilities
        .iter()
        .map(|ability| &ability.runtime)
        .chain(innate.iter())
        .flat_map(|runtime| runtime.animation.attachments.iter())
        .map(|evidence| &evidence.entity_tag)
        .collect::<BTreeSet<_>>();
    ensure!(
        inspected.len() == input.capture.animations.len(),
        "Animation evidence is not reachable from the subclass or selected abilities"
    );
    let native = input.request.damage_type.and_then(DamageType::native);
    let mut pending = vec![
        Contract::SourceSubclassIdentity,
        Contract::SourceDamageProviders,
    ];
    if innate.is_none() {
        pending.push(Contract::InnateControllers);
    }
    if native.is_none() {
        pending.push(Contract::DamageType);
    }
    pending.extend([
        Contract::NativeSelection,
        Contract::NativeEmission,
        Contract::Gameplay,
    ]);
    Ok(Plan {
        abilities,
        innate,
        damage: Damage {
            requested: input.request.damage_type,
            presentation: input.request.damage_type,
            native,
            policy: match input.request.damage_type {
                Some(DamageType::Strand) => DamagePolicy::StrandAsKinetic,
                _ if native.is_some() => DamagePolicy::NativeDamageType,
                _ => DamagePolicy::Unresolved,
            },
            source_providers_resolved: false,
        },
        pending,
    })
}

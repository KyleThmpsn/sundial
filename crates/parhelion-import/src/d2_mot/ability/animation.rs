//! Animation dependencies reached through source effects and investment ability grants.
//!
//! Actor animation selection is a separate unresolved root. Attachment clips cannot stand in
//! for the Guardian's first-person or third-person ability animations, motion or event timing.

use super::{effects, grants, source::tag};
use crate::d2_mot::{reader::Reader, rig, rig_convert};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Evidence {
    pub entity_tag: String,
    pub components: Vec<Value>,
    pub skeletons: Vec<Value>,
    /// The shared clip reader's bank references and bounded conversion diagnostics.
    pub clips: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Contract {
    AbilitySelection,
    SkeletonBinding,
    FirstPerson,
    ThirdPerson,
    ClipTracks,
    RootMotion,
    EventTiming,
    InterruptionAndCleanup,
}

#[derive(Debug, Serialize)]
pub struct Plan {
    pub attachments: Vec<Evidence>,
    pub uninspected_attachments: Vec<String>,
    pub pending: Vec<Contract>,
    pub runtime_ready: bool,
}

impl Evidence {
    pub(crate) fn validate(&self) -> Result<()> {
        tag(&self.entity_tag)?;
        ensure!(
            self.clips["runtime_ready"] == false,
            "Source animation evidence cannot declare runtime readiness"
        );
        let mut owners = BTreeMap::new();
        for component in &self.components {
            let owner = tag(component["owner"]
                .as_str()
                .context("Animation component owner")?)?;
            let class = tag(component["definition_class"]
                .as_str()
                .context("Animation component class")?)?;
            if let Some(previous) = owners.insert(owner, class) {
                ensure!(previous == class, "Animation owner has conflicting classes");
            }
        }
        for skeleton in &self.skeletons {
            let owner = tag(skeleton["owner"].as_str().context("Skeleton owner")?)?;
            ensure!(
                owners.get(&owner)
                    == Some(&tag(skeleton["class"]
                        .as_str()
                        .context("Skeleton class")?)?),
                "Skeleton owner is outside its entity"
            );
        }
        let clips = self.clips["clips"]
            .as_object()
            .context("Animation clip reports")?;
        let mut reached = BTreeSet::new();
        for bank in self.clips["banks"].as_array().context("Animation banks")? {
            let owner = tag(bank["owner"].as_str().context("Animation bank owner")?)?;
            ensure!(
                owners.get(&owner) == Some(&0x808025F8),
                "Animation bank is not owned by a source lookup component"
            );
            tag(bank["bank"].as_str().context("Animation bank tag")?)?;
            for clip in bank["clips"].as_array().context("Animation bank members")? {
                let name = clip.as_str().context("Animation clip tag")?;
                tag(name)?;
                ensure!(
                    clips.contains_key(name),
                    "Animation bank references an uncaptured clip"
                );
                reached.insert(name);
            }
        }
        ensure!(
            reached.len() == clips.len(),
            "Animation evidence includes clips outside its banks"
        );
        Ok(())
    }
}

pub(crate) fn plan(
    effects: &effects::Inspection,
    grants: &[grants::Plan],
    evidence: &[Evidence],
) -> Result<Plan> {
    let mut attachments = Vec::new();
    let mut uninspected = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for entity in effects.attachments()?.into_iter().chain(
        grants
            .iter()
            .filter_map(|grant| grant.evidence.entity.as_ref()),
    ) {
        let name = entity["tag"].as_str().context("Attachment entity tag")?;
        if !visited.insert(tag(name)?) {
            continue;
        }
        let captured = evidence
            .iter()
            .find(|entry| tag(&entry.entity_tag).ok() == tag(name).ok());
        if let Some(captured) = captured {
            ensure!(
                entity["components"] == json!(captured.components),
                "Animation evidence differs from the attached entity"
            );
            captured.validate()?;
            attachments.push(captured.clone());
        } else {
            uninspected.insert(name.to_owned());
        }
    }
    Ok(Plan {
        attachments,
        uninspected_attachments: uninspected.into_iter().collect(),
        pending: vec![
            Contract::AbilitySelection,
            Contract::SkeletonBinding,
            Contract::FirstPerson,
            Contract::ThirdPerson,
            Contract::ClipTracks,
            Contract::RootMotion,
            Contract::EventTiming,
            Contract::InterruptionAndCleanup,
        ],
        runtime_ready: false,
    })
}

pub(crate) fn capture<'a>(
    reader: &mut Reader,
    traces: impl IntoIterator<Item = &'a Value>,
    catalog: &grants::Catalog,
) -> Result<Vec<Evidence>> {
    let mut entities = BTreeMap::new();
    for trace in traces {
        let inspection = effects::inspect(trace)?;
        let grants = grants::plan(trace, Some(catalog))?.unwrap_or_default();
        for entity in inspection.attachments()?.into_iter().chain(
            grants
                .iter()
                .filter_map(|grant| grant.evidence.entity.as_ref()),
        ) {
            let name = entity["tag"].as_str().context("Attachment entity tag")?;
            if let Some(previous) = entities.insert(tag(name)?, entity.clone()) {
                ensure!(
                    previous == *entity,
                    "Attached entity has conflicting source traces"
                );
            }
        }
    }
    let mut result = Vec::new();
    for (entity_tag, entity) in entities {
        crate::cancellation::check()?;
        let components = entity["components"]
            .as_array()
            .context("Attached components")?
            .clone();
        let mut skeletons = Vec::new();
        let mut lookups = Vec::new();
        for component in &components {
            let owner = tag(component["owner"]
                .as_str()
                .context("Attached component owner")?)?;
            let class = tag(component["definition_class"]
                .as_str()
                .context("Attached component class")?)?;
            if matches!(class, 0x808081D6 | 0x808081DE) {
                let payload = reader.tag(owner, Some(0x80809B06))?;
                skeletons.push(rig::decode(
                    &payload,
                    payload.pointer(24)?,
                    class,
                    owner,
                    true,
                )?);
            }
            if class == 0x808025F8 {
                lookups
                    .push(json!({"owner":format!("{owner:08X}"), "class":format!("{class:08X}")}));
            }
        }
        let clips =
            rig_convert::animation::inspect_clips(reader, &json!({"components":lookups}), true)?;
        let evidence = Evidence {
            entity_tag: format!("{entity_tag:08X}"),
            components,
            skeletons,
            clips,
        };
        evidence.validate()?;
        result.push(evidence);
    }
    Ok(result)
}

use super::{
    Contract, Selection, animation, effects, grants,
    source::{number, tag},
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerStatus {
    Captured,
    UnknownClass,
    Undecoded,
    Unresolved,
}

/// All identities in this record are from the modern source. No native selectors are assigned.
#[derive(Debug, Serialize)]
pub struct Controller {
    pub perk_hash: u32,
    pub runtime_key: u32,
    pub source_index: u32,
    pub tag: Option<String>,
    pub class: Option<String>,
    pub status: ControllerStatus,
    pub unresolved: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Plan {
    pub selection: Selection,
    pub source_tag: String,
    pub source_category: u32,
    #[serde(flatten)]
    pub runtime: Runtime,
    pub pending: Vec<Contract>,
}

#[derive(Debug, Serialize)]
pub struct Runtime {
    pub controllers: Vec<Controller>,
    /// None means this capture predates inspection of investment ability grants.
    pub grants: Option<Vec<grants::Plan>>,
    pub effects: effects::Inspection,
    pub animation: animation::Plan,
}

fn controller(value: &Value) -> Result<Controller> {
    let perk_hash = number(&value["perk_hash"], "perk hash")?;
    let runtime_key = number(&value["runtime_key"], "runtime key")?;
    let source_index = number(&value["source_index"], "source perk index")?;
    ensure!(
        value["states"].is_array(),
        "Controller trace has no state array"
    );
    let (source_tag, class, status, unresolved) = if let Some(reason) = value["unresolved"].as_str()
    {
        ensure!(
            !reason.trim().is_empty(),
            "Unresolved controller has no reason"
        );
        ensure!(
            value["tag"].is_null() && value["class"].is_null(),
            "Unresolved controller also names an assignment"
        );
        (
            None,
            None,
            ControllerStatus::Unresolved,
            Some(reason.to_owned()),
        )
    } else {
        let source_tag = value["tag"].as_str().context("Controller tag")?;
        tag(source_tag)?;
        let class = value["class"].as_str().context("Controller class")?;
        let class_id = tag(class)?;
        let decoded = value["decoded_controller"]
            .as_bool()
            .context("Controller decode status")?;
        ensure!(
            value["requires_native_lowering"] == true,
            "Source controller must require native lowering"
        );
        ensure!(
            !decoded || class_id == 0x8080B835,
            "Unknown controller class cannot be marked decoded"
        );
        let status = if decoded {
            ControllerStatus::Captured
        } else if class_id == 0x8080B835 {
            ControllerStatus::Undecoded
        } else {
            ControllerStatus::UnknownClass
        };
        (
            Some(source_tag.to_owned()),
            Some(class.to_owned()),
            status,
            None,
        )
    };
    Ok(Controller {
        perk_hash,
        runtime_key,
        source_index,
        tag: source_tag,
        class,
        status,
        unresolved,
    })
}

pub(crate) fn runtime(
    item_hash: u32,
    source_tag: &str,
    trace: &Value,
    animations: &[animation::Evidence],
    catalog: Option<&grants::Catalog>,
) -> Result<Runtime> {
    ensure!(trace["format"] == 1, "Unsupported source perk trace format");
    ensure!(
        number(&trace["plug_hash"], "trace item hash")? == item_hash,
        "Trace belongs to another source item"
    );
    ensure!(
        tag(trace["item_tag"].as_str().context("Trace item tag")?)? == tag(source_tag)?,
        "Trace item tag differs from its source identity"
    );
    ensure!(
        trace["installable"] == false,
        "Source trace cannot be installable"
    );
    let mut identities = BTreeSet::new();
    let controllers = trace["perks"]
        .as_array()
        .context("Source perk trace list")?
        .iter()
        .map(|value| {
            let controller = controller(value)?;
            ensure!(
                identities.insert(controller.perk_hash),
                "Duplicate traced perk identity"
            );
            Ok(controller)
        })
        .collect::<Result<Vec<_>>>()?;
    let grants = grants::plan(trace, catalog)?;
    let effects = effects::inspect(trace)?;
    let animation = animation::plan(&effects, grants.as_deref().unwrap_or_default(), animations)?;
    Ok(Runtime {
        controllers,
        grants,
        effects,
        animation,
    })
}

pub(crate) fn plan(
    selection: &Selection,
    source_tag: &str,
    source_category: u32,
    trace: &Value,
    animations: &[animation::Evidence],
    catalog: Option<&grants::Catalog>,
) -> Result<Plan> {
    let runtime = runtime(selection.plug_hash, source_tag, trace, animations, catalog)?;
    let controllers = &runtime.controllers;
    let mut pending = if selection.role.active() {
        vec![
            Contract::SourceAbilityGraph,
            Contract::NativeAbilityLowering,
        ]
    } else {
        vec![Contract::SourceSelectionRules]
    };
    if !controllers.is_empty() {
        pending.push(Contract::NativePerkLowering);
    }
    if controllers
        .iter()
        .any(|c| c.status == ControllerStatus::Unresolved)
    {
        pending.push(Contract::PerkAssignment);
    }
    if controllers.iter().any(|c| {
        matches!(
            c.status,
            ControllerStatus::UnknownClass | ControllerStatus::Undecoded
        )
    }) {
        pending.push(Contract::ControllerFormat);
    }
    pending.extend([
        Contract::Effects,
        Contract::Animation,
        Contract::PrivateOwnership,
        Contract::ActivationAndCleanup,
    ]);
    Ok(Plan {
        selection: selection.clone(),
        source_tag: source_tag.to_owned(),
        source_category,
        runtime,
        pending,
    })
}

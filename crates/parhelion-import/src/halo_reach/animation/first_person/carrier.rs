//! Resolve and validate the selected native first-person controller.
use super::*;
use std::sync::Arc;

pub(super) struct Carrier {
    pub runtime: u32,
    pub attachment: u32,
    pub entity: u32,
    pub lookup_tag: u32,
    pub lookup: Arc<Payload>,
    pub bank_tag: u32,
    pub bank: Arc<Payload>,
    pub parameter_tag: u32,
    pub parameters: Arc<Payload>,
    pub state_tag: u32,
    pub states: Arc<Payload>,
    pub pose_owner: u32,
    pub pose_tag: u32,
    pub pose_layers: Arc<Payload>,
    pub rig: animation::control::Rig,
    pub descriptors: Vec<usize>,
    pub consumers: Vec<(u32, Arc<Payload>)>,
}

impl Carrier {
    pub fn read(native: &mut Reader, donor: u32) -> Result<Self> {
        let item = crate::tiger::shadowkeep::extract(native, donor)?;
        let report = crate::tiger::rig::inspect(native, hex(&item["item_tag"])?, false)?;
        let runtime = hex(&report["runtime_entity"])?;
        let content = hex(&report["content_key"])?;
        let components = report["components"]
            .as_array()
            .context("Native weapon components")?;
        let mut chains = Vec::new();
        for component in components
            .iter()
            .filter(|c| c["class"] == "80804221" && hex(&c["entity"]).ok() == Some(runtime))
        {
            let owner = hex(&component["owner"])?;
            let attachments = native.tag(owner, Some(0x80809C36))?;
            for row in crate::tiger::rig::attachment_rows(
                &attachments,
                attachments.pointer(24)?,
                content,
                false,
            )? {
                chains.push((owner, attachments.u32(row + 0x78)?));
            }
        }
        chains.sort_unstable();
        chains.dedup();
        ensure!(
            chains.len() == 1,
            "Native first-person attachment is ambiguous"
        );
        let (attachment, entity) = chains[0];
        let owner = |class: &str| -> Result<u32> {
            let selected = components
                .iter()
                .filter(|c| c["class"] == class && hex(&c["entity"]).ok() == Some(entity))
                .collect::<Vec<_>>();
            ensure!(
                selected.len() == 1,
                "Native first-person component {class} is absent or ambiguous"
            );
            hex(&selected[0]["owner"])
        };
        let skeleton = native.tag(owner("80808546")?, Some(0x80809C36))?;
        let controls = native.tag(owner("80808F8F")?, Some(0x80809C36))?;
        let rig = animation::control::Rig::read(&skeleton, &controls)?;
        let lookup_tag = owner("8080344B")?;
        let lookup = native.tag(lookup_tag, Some(0x80809C36))?;
        let definition = lookup.pointer(24)?;
        let bank_tag = lookup.u32(definition + 0x90)?;
        let parameter_tag = lookup.u32(definition + 0x94)?;
        let state_tag = lookup.u32(definition + 0x9C)?;
        let bank = native.tag(bank_tag, Some(0x808036F6))?;
        let parameters = native.tag(parameter_tag, Some(0x80808EE1))?;
        let states = native.tag(state_tag, Some(0x80803465))?;
        let pose_owner = owner("80803640")?;
        let pose = native.tag(pose_owner, Some(0x80809C36))?;
        let pose_tag = pose.u32(pose.pointer(24)? + 0x11C)?;
        let pose_layers = native.tag(pose_tag, None)?;
        let descriptors = bank.array(0x68, 32, Some(0x80809002))?;
        ensure!(
            pose_layers.array(40, 2, Some(0x80800006))?.len() == descriptors.len(),
            "Native pose dispatch does not match its clip bank"
        );
        let mut consumers = Vec::new();
        for class in ["80803640", "808036CF"] {
            let tag = owner(class)?;
            let payload = native.tag(tag, Some(0x80809C36))?;
            ensure!(
                payload.u32(animation::bank_field(&payload)?)? == bank_tag,
                "Native arm consumer selects a different bank"
            );
            consumers.push((tag, payload));
        }
        Ok(Self {
            runtime,
            attachment,
            entity,
            lookup_tag,
            lookup,
            bank_tag,
            bank,
            parameter_tag,
            parameters,
            state_tag,
            states,
            pose_owner,
            pose_tag,
            pose_layers,
            rig,
            descriptors,
            consumers,
        })
    }
}

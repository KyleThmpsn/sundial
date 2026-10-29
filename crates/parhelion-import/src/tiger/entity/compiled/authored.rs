//! Registration rows for explicitly authored native implementations.
use super::{Component, Registration};
use crate::tiger::{entity::links, payload::Payload};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

/// A provider's interface and method table must be emitted by its native author.
/// The namespace and condition belong to the selected host lifecycle contract.
pub struct Provider<'a> {
    pub key: u32,
    pub offset: usize,
    pub metadata: &'a Payload,
    pub condition: u16,
}

pub(super) fn registrations(
    component: &Component<'_>,
    ordinal: usize,
    providers: &[Provider<'_>],
    masks: &mut Vec<Vec<u8>>,
    groups: &mut BTreeMap<u32, Vec<Registration>>,
) -> Result<()> {
    let p = component.payload;
    let instance = p.pointer(16)?;
    // The header names the loader's runtime boundary. Sealing may duplicate its
    // schema after appended mutable arrays while implementation references keep
    // pointing at the original reciprocal definition.
    let definition = usize::try_from(p.u64(instance + 8)?)?;
    let boundary = p.pointer(24)?;
    ensure!(
        p.u64(0)? == p.0.len() as u64
            && instance >= 4
            && instance < definition
            && definition <= boundary
            && p.u32(instance)? == component.owner
            && p.u32(definition)? == component.owner
            && p.u32(boundary)? == component.owner
            && p.u32(instance - 4)? == p.u32(definition + 4)?
            && p.u32(definition - 4)? == p.u32(instance + 4)?
            && p.u32(boundary - 4)? == p.u32(definition - 4)?
            && p.u32(boundary + 4)? == p.u32(definition + 4)?
            && p.u64(definition + 8)? == instance as u64
            && p.u64(boundary + 8)? == instance as u64,
        "Authored component root is not reciprocal"
    );
    let mut keys = BTreeSet::new();
    for provider in providers {
        // A root and a retained embedded helper may publish the same namespace with distinct
        // priority rows. Only a repeated binding to the same wrapper is ambiguous here.
        ensure!(
            provider.key != u32::MAX && keys.insert((provider.key, provider.offset)),
            "Authored component repeats an interface binding"
        );
        let object = links::Object {
            owner: component.owner,
            class: provider.metadata.u32(8)?,
            offset: u64::try_from(provider.offset)?,
        };
        let interface = links::Interface::read(p, object, provider.metadata, false)?;
        ensure!(
            object.class & 0xffff0000 == 0x80800000
                && interface.instance_class & 0xffff0000 == 0x80800000,
            "Authored provider has no native interface type"
        );
        let target = interface.definition_offset;
        let state = usize::try_from(p.u64(target + 8)?)?;
        let class = p.u32(target + 4)?;
        // A derived native owner retains provider tables that dispatch its registered bases.
        ensure!(
            p.u32(target)? == component.owner
                && p.u32(state)? == component.owner
                && p.u64(state + 8)? == target as u64
                && !interface.methods.is_empty()
                && interface.methods.iter().all(|method| {
                    sundial::package_authoring::runtime::native_type_inherits(
                        class,
                        method.implementation_class,
                    )
                }),
            "Authored provider does not resolve its implementation"
        );
        let mut map = vec![0; 40];
        let mut descriptor = vec![0; 24];
        if provider.condition != 0 {
            map[..2].copy_from_slice(&i16::try_from(masks.len())?.to_le_bytes());
            masks.push(vec![0; 2]);
        }
        map[2..4].copy_from_slice(&provider.condition.to_le_bytes());
        let relative = i32::try_from(state as i64 - instance as i64)?;
        map[4..8].copy_from_slice(&relative.to_le_bytes());
        let priority = i32::from(p.i16(provider.offset + 16)?);
        map[8..12].copy_from_slice(&priority.to_le_bytes());
        map[12..16].copy_from_slice(&class.to_le_bytes());
        map[16..20].copy_from_slice(&component.owner.to_le_bytes());
        map[20..24].copy_from_slice(&0x80809c50u32.to_le_bytes());
        map[24..32].copy_from_slice(&object.offset.to_le_bytes());
        map[32..36].copy_from_slice(&interface.metadata_tag.to_le_bytes());
        descriptor[..4].copy_from_slice(&component.owner.to_le_bytes());
        descriptor[4..8].copy_from_slice(&class.to_le_bytes());
        descriptor[8..16].copy_from_slice(&(state as u64).to_le_bytes());
        descriptor[16..20].copy_from_slice(&u32::try_from(ordinal)?.to_le_bytes());
        groups.entry(provider.key).or_default().push(Registration {
            priority,
            map,
            descriptor,
        });
    }
    ensure!(!providers.is_empty(), "Authored component has no providers");
    ensure!(
        masks.len() <= i16::MAX as usize,
        "Authored condition capacity exceeded"
    );
    p.0.get(instance..boundary)
        .context("Authored instance span")?;
    Ok(())
}

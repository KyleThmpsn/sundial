//! The sequence event source keeps each runtime's checked dispatch metadata.
use super::Bindings;
use crate::d2_mot::{
    entity::links::{Interface, Object},
    native::effects::controller::Relocation,
    payload::Payload,
};
use anyhow::{Context, Result, ensure};

pub(super) fn event(
    source: &Payload,
    native: &Payload,
    bindings: &Bindings,
) -> Result<Option<Relocation>> {
    let sd = source.pointer(24)?;
    // Older callers that do not request this endpoint have no provider metadata.
    // A later graph reference still fails linking if no mapping was qualified.
    if !bindings.metadata.contains_key(&source.u32(sd + 0xB0)?) {
        return Ok(None);
    }
    let read = |p: &Payload, modern: bool| -> Result<Object> {
        let d = p.pointer(24)?;
        let (offset, class, instance, implementation) = if modern {
            (0xA8, 0x80809190, 0x8080918F, 0x80809479)
        } else {
            (0x90, 0x808092F8, 0x808092F7, 0x808084D7)
        };
        let at = d + offset;
        let object = Object {
            owner: p.u32(p.pointer(16)?)?,
            class,
            offset: at as u64,
        };
        let metadata = bindings
            .metadata
            .get(&p.u32(at + 8)?)
            .context("sequence event interface metadata is absent")?;
        let contract = Interface::read(p, object, metadata, modern)?;
        ensure!(
            contract.definition_offset == d
                && contract.instance_class == instance
                && contract.methods.len() == 1
                && contract.methods[0].implementation_class == implementation
                && contract.methods[0].index == 3
                && contract.methods[0].arguments == [0, 0]
                && p.u64(at + 16)? == 0
                && (!modern || p.u64(at + 24)? == 0),
            "sequence event provider dispatch or initial state differs"
        );
        Ok(object)
    };
    Ok(Some(Relocation {
        source: read(source, true)?,
        target: read(native, false)?,
    }))
}

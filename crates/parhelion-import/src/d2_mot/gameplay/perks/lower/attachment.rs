//! Attachment action declarations retain cleanup and creation parameters separately.
use crate::d2_mot::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

const EMPTY: u32 = 0x811C9DC5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cleanup {
    Automatic,
    UnlessParentIsHost,
    Independent,
}

pub struct Attachment {
    pub graph: u32,
    pub retained: bool,
    pub target: u8,
    pub cleanup: Cleanup,
    pub removal_name: u32,
    pub removal_value: f32,
    pub stacks_name: u32,
    pub parameters: BTreeMap<u32, [u8; 16]>,
}

/// Read a complete action before deciding whether the host can express its
/// parent-sensitive lifetime. A nonempty native lifetime key is not this enum.
pub fn read(reader: &Reader, source: &Payload, at: usize) -> Result<Attachment> {
    ensure!(
        at >= 4 && source.u32(at - 4)? == 0x8080B7C6,
        "attachment action class differs"
    );
    let header = source.bytes::<8>(at)?;
    ensure!(
        header[0] == 1
            && header[1] <= 1
            && header[2] <= 3
            && header[3..].iter().all(|byte| *byte == 0),
        "attachment action header differs"
    );
    // The debug name is a relative string, not a resource reference.
    if source.u64(at + 8)? != 0 {
        let string = source.pointer(at + 8)?;
        let tail = source
            .0
            .get(string..)
            .context("attachment debug string extent")?;
        let length = tail
            .iter()
            .position(|byte| *byte == 0)
            .context("attachment debug string is unterminated")?;
        std::str::from_utf8(&tail[..length])?;
    }
    let graph = reader.ref64(source, at + 16)?;
    ensure!(
        reader.reference(graph)? == 0x80809AD8,
        "attachment action does not reference an entity graph"
    );
    let cleanup = match source.u32(at + 32)? {
        0 => Cleanup::Automatic,
        1 => Cleanup::UnlessParentIsHost,
        2 => Cleanup::Independent,
        value => anyhow::bail!("unsupported attachment cleanup mode {value}"),
    };
    let removal_name = source.u32(at + 36)?;
    let removal_value = source.f32(at + 40)?;
    ensure!(
        removal_value.is_finite() && (removal_name != EMPTY || removal_value == 0.0),
        "unnamed attachment removal has a value"
    );
    let mut parameters = BTreeMap::new();
    for row in source.array(at + 48, 96, Some(0x808022E4))? {
        let name = source.u32(row)?;
        ensure!(
            name != EMPTY && source.u32(row + 4)? == 0 && source.u64(row + 8)? == 0,
            "attachment parameter has a binding or invalid name"
        );
        let program = row + 16;
        let code = source.array(program, 1, Some(0x80800009))?;
        ensure!(
            code.len() == 4 && source.bytes::<4>(code[0])? == [0x42, 0, 0x4C, 0],
            "attachment parameter requires a nonconstant creation expression"
        );
        let constants = source.array(program + 16, 16, Some(0x80800090))?;
        ensure!(
            constants.len() == 1
                && source.u64(program + 32)? == 1
                && source.u64(program + 40)? == 1
                && source.bytes::<16>(program + 48)? == [0; 16]
                && source.u32(row + 80)? == 255
                && source.u32(row + 84)? == EMPTY
                && source.u64(row + 88)? == 0,
            "attachment parameter expression shape differs"
        );
        let constant = source.bytes::<16>(constants[0])?;
        for lane in 0..4 {
            ensure!(
                source.f32(constants[0] + lane * 4)?.is_finite()
                    && constant[lane * 4..lane * 4 + 4] == constant[..4],
                "attachment creation parameter is not a scalar broadcast"
            );
        }
        ensure!(
            parameters.insert(name, constant).is_none(),
            "attachment creation parameter is duplicated"
        );
    }
    Ok(Attachment {
        graph,
        retained: header[1] != 0,
        target: header[2],
        cleanup,
        removal_name,
        removal_value,
        stacks_name: source.u32(at + 44)?,
        parameters,
    })
}

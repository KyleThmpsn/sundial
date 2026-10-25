//! Particle VM conversion, kept separate from renderer material expressions.
//!
//! This produces native execution phases. The particle system's GPU passes,
//! allocation layout and owning sequencer still need their own conversion.
use super::payload::Payload;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub mod mesh;
pub mod native;
mod program;
pub mod renderer;
pub mod system;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Program {
    pub sections: [Vec<u8>; 8],
    pub defaults: Vec<[u8; 16]>,
    pub constants: Vec<[u8; 16]>,
    pub routes: Vec<[u8; 2]>,
    pub lifetime_ceiling: f32,
    /// Object-channel names declared by the source program, in authored order.
    #[serde(default)]
    pub channel_names: Vec<u32>,
    /// Pairs in the source 80806928 table, native 80806E2D. Their count is not
    /// the size of the owning runtime's transform table, which also includes
    /// transforms supplied by the controller.
    #[serde(default)]
    pub transform_bindings: Vec<[u8; 2]>,
    /// Source bank 5 allocation in bytes. Old intermediate files lack this
    /// metadata and cannot establish a safe native allocation.
    #[serde(default)]
    pub workspace_bytes: Option<u16>,
}

fn vectors(p: &Payload, offset: usize, maximum: usize) -> Result<Vec<[u8; 16]>> {
    ensure!(
        p.u64(offset)? <= maximum as u64,
        "particle vector table exceeds capacity"
    );
    p.array(offset, 16, Some(0x80800090))?
        .into_iter()
        .map(|at| p.bytes(at))
        .collect()
}

impl Program {
    /// Read the current class-80806927 dialect. This is not a native tag payload.
    pub fn read(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() >= 0x158 && bytes.len() <= 16 * 1024 * 1024,
            "particle program envelope size differs"
        );
        let p = Payload(bytes.to_vec());
        ensure!(
            p.u64(0)? == bytes.len() as u64,
            "particle program size differs"
        );
        let defaults = vectors(&p, 8, 256)?;
        let constants = vectors(&p, 0x68, 4096)?;
        ensure!(p.u64(0x58)? <= 65536, "particle bytecode exceeds capacity");
        let code = p
            .array(0x58, 1, Some(0x80800009))?
            .into_iter()
            .map(|at| bytes[at])
            .collect::<Vec<_>>();
        let mut at = 0usize;
        let mut sections: [Vec<u8>; 8] = std::array::from_fn(|_| Vec::new());
        for (index, section) in sections.iter_mut().enumerate() {
            let end = at + usize::from(p.u16(0x78 + index * 2)?);
            *section = code
                .get(at..end)
                .context("particle phase exceeds bytecode")?
                .to_vec();
            at = end;
        }
        ensure!(at == code.len(), "particle phases do not cover bytecode");
        let routes = (0..56)
            .map(|i| p.bytes::<2>(0x88 + i * 2))
            .collect::<Result<Vec<_>>>()?;
        for [bank, slot] in &routes {
            ensure!(
                *bank == 0xFF
                    || (*bank <= 6 && (*bank != 6 || usize::from(*slot) < defaults.len() * 4)),
                "particle route is outside its value bank"
            );
        }
        let lifetime_ceiling = p.f32(0x128)?;
        ensure!(
            lifetime_ceiling >= 0.0,
            "negative particle lifetime ceiling"
        );
        ensure!(
            p.u64(0x108)? <= 256,
            "particle channel table exceeds capacity"
        );
        let channel_names = p
            .array(0x108, 4, Some(0x80800070))?
            .into_iter()
            .map(|at| p.u32(at))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            p.u64(0x28)? <= 256,
            "particle transform table exceeds capacity"
        );
        let transform_bindings = p
            .array(0x28, 2, Some(0x80806928))?
            .into_iter()
            .map(|at| p.bytes(at))
            .collect::<Result<Vec<_>>>()?;
        let workspace_bytes = p.u16(0x148)?;
        ensure!(
            workspace_bytes != 0 && workspace_bytes.is_multiple_of(16),
            "particle workspace is not a nonempty vector allocation"
        );
        for section in &sections {
            for instruction in program::parse(section)? {
                if matches!(instruction.op, 0x4C | 0x4D) && instruction.args[0] == 5 {
                    ensure!(
                        (u16::from(instruction.args[1]) + 1) * 16 <= workspace_bytes,
                        "particle register exceeds its declared workspace"
                    );
                }
            }
        }
        for [bank, scalar] in &routes {
            ensure!(
                *bank != 5 || (u16::from(*scalar) + 1) * 4 <= workspace_bytes,
                "particle route exceeds its declared workspace"
            );
        }
        Ok(Self {
            sections,
            defaults,
            constants,
            routes,
            lifetime_ceiling,
            channel_names,
            transform_bindings,
            workspace_bytes: Some(workspace_bytes),
        })
    }

    /// Check allocation separately from instruction lowering. The captured
    /// native allocator at 011EC200 reserves 160 bytes and copies the count at
    /// native program +0x147. Larger source layouts need a coordinated VM and
    /// shader rewrite. Successful bytecode lowering does not remove that limit.
    pub fn native_workspace_bytes(&self) -> Result<u8> {
        let bytes = self
            .workspace_bytes
            .context("particle allocation metadata is missing")?;
        ensure!(
            bytes != 0 && bytes.is_multiple_of(16),
            "particle workspace alignment differs"
        );
        ensure!(
            bytes <= 160,
            "particle workspace requires {bytes} bytes, native allocation is 160"
        );
        Ok(u8::try_from(bytes)?)
    }

    /// Named channels require the indices assigned by the translated controller.
    /// Missing channels and unsupported operations are errors, never zero inputs.
    pub fn lower(&self, channels: &BTreeMap<u32, u8>) -> Result<Self> {
        let mut result = self.clone();
        ensure!(
            channels.len() <= 256,
            "particle channel table exceeds capacity"
        );
        let mut names = vec![None; channels.len()];
        for (&name, &slot) in channels {
            let entry = names
                .get_mut(usize::from(slot))
                .context("particle channel indices are not contiguous")?;
            ensure!(
                entry.replace(name).is_none(),
                "particle channels share an index"
            );
        }
        result.channel_names = names
            .into_iter()
            .map(|name| name.context("particle channel index is unassigned"))
            .collect::<Result<Vec<_>>>()?;
        for (phase, section) in self.sections.iter().enumerate() {
            result.sections[phase] =
                program::lower(section, self.constants.len(), self.defaults.len(), channels)
                    .with_context(|| format!("particle phase {phase}"))?;
        }
        Ok(result)
    }

    /// Native opcode 47 indexes a name table and then looks up that name in the
    /// owning runtime. Keep the source names and their authored indices. The
    /// runtime still has to publish their values when this program executes.
    pub fn lower_declared(&self) -> Result<Self> {
        let mut bindings = BTreeMap::new();
        for (index, name) in self.channel_names.iter().enumerate() {
            ensure!(
                bindings.insert(*name, u8::try_from(index)?).is_none(),
                "particle channel table contains duplicate names"
            );
        }
        self.lower(&bindings)
    }

    /// Discover bindings before lowering, without treating their values as tags.
    pub fn channels(&self) -> Result<BTreeSet<u32>> {
        let mut result = BTreeSet::new();
        for section in &self.sections {
            for instruction in program::parse(section)? {
                if instruction.op == 0x55 {
                    result.insert(u32::from_be_bytes(instruction.args.try_into()?));
                }
            }
        }
        Ok(result)
    }
}

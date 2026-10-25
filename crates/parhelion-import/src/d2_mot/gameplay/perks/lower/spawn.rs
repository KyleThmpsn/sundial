//! One-shot entity creation with the shared owner or event position policy.
use super::Node;
use crate::d2_mot::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};

pub struct Spawn {
    pub graph: u32,
    pub position: u8,
}

pub fn read(reader: &Reader, source: &Payload, at: usize) -> Result<Spawn> {
    ensure!(
        at >= 4 && source.u32(at - 4)? == 0x80803128,
        "spawn action class differs"
    );
    ensure!(
        source.bytes::<8>(at)? == [3, 0, 0, 0, 0, 0, 0, 0]
            && source.bytes::<24>(at + 8)? == [0; 24],
        "spawn action has a retained effect, custom creation mode or attachment policy"
    );
    if source.u64(at + 32)? != 0 {
        let string = source.pointer(at + 32)?;
        let bytes = source
            .0
            .get(string..)
            .context("spawn debug string extent")?;
        let length = bytes
            .iter()
            .position(|&byte| byte == 0)
            .context("spawn debug string is unterminated")?;
        std::str::from_utf8(&bytes[..length])?;
    }
    let graph = reader.ref64(source, at + 40)?;
    ensure!(
        reader.reference(graph)? == 0x80809AD8,
        "spawn action does not reference an entity graph"
    );
    let position = source.u8(at + 56)?;
    // Modern 1841230 passes +38 to 1019760. Mode one copies the event
    // position. Native 1089900 passes +04 to the corresponding EC0FC0.
    ensure!(
        position <= 1
            && source.bytes::<3>(at + 57)? == [0; 3]
            && source.u32(at + 60)? == 0x811C9DC5,
        "spawn position requires an additional target or locator"
    );
    ensure!(
        source.bytes::<16>(at + 64)? == [0; 16]
            && source.bytes::<16>(at + 80)? == [0, 0, 128, 63, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
            && source.bytes::<32>(at + 96)? == [0; 32]
            && source.bytes::<16>(at + 128)? == [0; 16],
        "spawn has an active transform, collision option or creation parameter"
    );
    Ok(Spawn { graph, position })
}

impl Spawn {
    /// The enclosing asset transaction supplies a private native graph identity.
    pub fn node(&self, graph: u32) -> Result<Node> {
        ensure!(
            (0x80800001..=0x81FFFFFF).contains(&graph),
            "invalid native spawn graph"
        );
        let mut bytes = vec![0; 24];
        bytes[0] = 3;
        bytes[4] = self.position;
        bytes[16..20].copy_from_slice(&graph.to_le_bytes());
        Ok(Node {
            class: 0x80803E43,
            kind: 3,
            bytes,
        })
    }
}

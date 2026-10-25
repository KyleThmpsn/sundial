//! Incoming damage callbacks retain their filter, multiplier and private self-reference.
use super::*;
use sundial::package_authoring::sandbox_perk::action::native::{Block, Graph, NodeKind};

pub(super) fn read(
    source: &Source,
    p: &Payload,
    at: usize,
    native: Option<&mut Reader>,
) -> Result<Action> {
    ensure!(
        p.bytes::<3>(at + 1)? == [1, 0, 0]
            && p.bytes::<4>(at + 8)? == [255, 0, 0, 0]
            && p.f32(at + 4)? >= 0.0,
        "Source incoming damage has unqualified lifecycle, recipient or stat input"
    );
    let _ = p.f32(at + 12)?;
    // The native callback admits damage and ability filters. A separate D1 object
    // filter cannot be dropped. The bounded route accepts only its empty form.
    ensure!(
        p.bytes::<4>(at + 0x9C)? == [0; 4]
            && p.bytes::<0x40>(at + 0xA0)? == [0; 0x40]
            && p.bytes::<0x80>(at + 0xF0)? == [0; 0x80]
            && p.bytes::<4>(at + 0x170)? == [1; 4]
            && p.u32(at + 0x174)? == 0
            && p.u64(at + 0x178)? == 0x808026E4
            && p.u64(at + 0x188)? == at as u64
            && p.u64(at + 0x190)? == 0x808026E4,
        "Source incoming damage has an object filter or incompatible callback"
    );
    let mut bytes = vec![0; 184];
    bytes[..16].copy_from_slice(&p.bytes::<16>(at)?);
    bytes[0] = 33;
    put(&mut bytes, 0xA0, &0x80803E3Cu64.to_le_bytes())?;
    put(&mut bytes, 0xAC, &0x80803E3Cu32.to_le_bytes())?;
    let mut graph = Graph {
        blocks: vec![Block {
            class: 0x80803E3C,
            count: None,
            bytes,
            links: BTreeMap::new(),
        }],
    };
    filter::damage(source, p, at + 16, at + 0xE0, &mut graph, 16, native)?;
    // The ordinary private-action relocation writes the new owner and node offset.
    graph
        .validate_node(NodeKind::Effect(33))
        .map_err(anyhow::Error::msg)?;
    Ok(Action::Native {
        node: NativeNode {
            kind: 33,
            bytes: graph.emit().map_err(anyhow::Error::msg)?,
        },
    })
}

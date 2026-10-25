//! Default-orb selection, including structurally identical copies of the source default.
use super::*;
mod equivalent;

pub(super) fn read(source: &Source, data: &Payload, at: usize) -> Result<Action> {
    ensure!(
        data.bytes::<3>(at + 1)? == [0; 3]
            && data.u32(at + 4)? <= i32::MAX as u32
            && data.f32(at + 8)? >= 0.0
            && data.u32(at + 12)? == 0
            && data.bytes::<12>(at + 28)? == [0; 12],
        "Source orb generation requires owner position and the supported lifecycle"
    );
    // The resource name precedes its tag. It is a serialized path, not callback state.
    if data.u64(at + 16)? != 0 {
        let path = data.pointer(at + 16)?;
        let tail = data
            .0
            .get(path..)
            .context("Source orb path is outside its action")?;
        let end = tail
            .iter()
            .take(4096)
            .position(|&b| b == 0)
            .context("Source orb path has no terminator")?;
        std::str::from_utf8(&tail[..end]).context("Source orb path is not UTF-8")?;
    }
    let graph = data.u32(at + 24)?;
    if graph != u32::MAX {
        let (_, globals) = source.table(0x80802257)?;
        // The original orb service's fallback is game globals +0x90, then +0x38.
        let default = globals.u32(0xC8)?;
        ensure!(
            source.reference(default)? == 0x80800734,
            "Source default orb is not a pattern"
        );
        equivalent::check(source, graph, default)?;
    }
    // Source 71A160 and native 1089AF0 retain count and value. Native FFFFFFFF
    // selects its own default pickup without retaining a source resource or path.
    let mut bytes = data.bytes::<32>(at)?.to_vec();
    bytes[0] = 5;
    bytes[12..24].fill(0);
    bytes[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
    Ok(Action::Native {
        node: NativeNode { kind: 5, bytes },
    })
}

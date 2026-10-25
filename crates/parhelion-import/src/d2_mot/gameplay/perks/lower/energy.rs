//! Ordinary ability-energy nodes. Controller inputs remain a separate contract.
use super::{Node, append_array, scalar_rows};
use crate::d2_mot::payload::Payload;
use anyhow::{Result, ensure};

/// Translate a single-input energy adjustment into the native kind-8 action.
/// The Demolitionist package pair establishes the program envelope. The native
/// EC4D70 consumer preserves slot, activity, base/current bank, scale and limit.
/// Source typed providers and normalization need another behavior bridge.
pub fn component_value(source: &Payload, at: usize) -> Result<Node> {
    ensure!(
        at >= 4 && source.u32(at - 4)? == 0x8080_22E5,
        "unsupported source energy-adjustment class"
    );
    let root = source.bytes::<100>(at)?;
    ensure!(
        root[0] == 8
            && root[1] == 0
            && matches!(root[2], 0 | 1 | 2 | 7)
            && root[3] <= 2
            && root[4] <= 1
            && root[5..8] == [0; 3]
            && source.f32(at + 8)?.is_finite()
            && source.f32(at + 12)?.is_finite()
            && source.u64(at + 16)? == 0,
        "unsupported source energy target, activity, bank or scalar fields"
    );
    ensure!(
        source.u64(at + 56)? == 1
            && source.u64(at + 64)? == 1
            && source.u64(at + 72)? == 0
            && source.u64(at + 80)? == 0
            && matches!(root[88], 0 | 1 | 255)
            && root[89..92] == [0; 3]
            && source.u32(at + 92)? == 0x811C_9DC5
            && source.u32(at + 96)? == 0,
        "source energy adjustment needs an unsupported input provider or normalization"
    );
    let code_count = usize::try_from(source.u64(at + 24)?)?;
    let constant_count = usize::try_from(source.u64(at + 40)?)?;
    ensure!(
        code_count > 0 && code_count <= 4096 && constant_count <= 256,
        "source energy program exceeds native limits"
    );
    let code = scalar_rows(source, at + 24, 1, 0x8080_0009, code_count)?
        .into_iter()
        .map(|row| source.u8(row))
        .collect::<Result<Vec<_>>>()?;
    let constants = if constant_count == 0 {
        ensure!(
            source.u64(at + 48)? == 0,
            "empty energy constants have a pointer"
        );
        Vec::new()
    } else {
        scalar_rows(source, at + 40, 16, 0x8080_0090, constant_count)?
            .into_iter()
            .map(|row| source.bytes::<16>(row))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect()
    };
    let code = crate::d2_mot::native::effects::lower_program(&code, constant_count, 1)?;
    let mut bytes = vec![0; 80];
    bytes[..24].copy_from_slice(&root[..24]);
    bytes[56..60].copy_from_slice(&1u32.to_le_bytes());
    bytes[64..68].copy_from_slice(&1u32.to_le_bytes());
    bytes[72] = root[88];
    append_array(&mut bytes, 24, 0x8080_0009, &code, 1)?;
    append_array(&mut bytes, 40, 0x8080_0090, &constants, 16)?;
    Ok(Node {
        class: 0x8080_3E4D,
        kind: 8,
        bytes,
    })
}

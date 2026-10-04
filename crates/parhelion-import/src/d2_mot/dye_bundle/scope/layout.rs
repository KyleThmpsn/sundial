//! Native scope metadata and repair of the first imported shader layout.
use crate::d2_mot::payload::Payload;
use anyhow::{Result, bail, ensure};

pub const REVISION: u64 = 3;
const STAGES: [usize; 5] = [0xD8, 0x170, 0x208, 0x2A0, 0x338];

/// Normalize a private in-memory copy. Never rewrite the pinned graph or its
/// texture bindings or recipe edits. Old component mappings are repaired with
/// their live expressions before authoring applies the user's edits.
pub fn normalize_scope(bytes: &mut Vec<u8>, revision: Option<u64>) -> Result<()> {
    ensure!(bytes.len() >= 0x3D0, "Imported shader scope is truncated");
    let original = Payload(bytes.to_vec());
    ensure!(
        original.u64(0)? as usize == bytes.len() && matches!(original.u32(0x10)?, 26..=28),
        "Imported shader scope envelope differs"
    );
    ensure!(
        original.array(0x88, 16, Some(0x80800090))?.len() == 27,
        "Imported shader output vectors differ"
    );
    let program = original.array(0x58, 1, Some(0x80800009))?;
    match revision {
        None | Some(1) => {
            // The old converter cleared these fields and wrote each otherwise
            // empty stage at an 0x88 stride. Require that exact structural form
            // before recovering its metadata at the native 0x98 stride.
            ensure!(
                original.0[0xA8..0xB8].iter().all(|b| *b == 0)
                    && original.0[0xC0..0xC8].iter().all(|b| *b == 0),
                "Unrecognized imported shader layout. Import this shader again"
            );
            let old_stages = [0xC8, 0x150, 0x1D8, 0x260, 0x2E8];
            for at in old_stages {
                ensure!(
                    original.0[at..at + 0x88].iter().enumerate().all(
                        |(i, b)| matches!(i, 0x10..=0x17 | 0x58..=0x67 | 0x78..=0x7F) || *b == 0
                    ),
                    "Unrecognized imported shader stage. Import this shader again"
                );
            }
            bytes[0x98..0xA8].fill(0);
            bytes[0xA8..0xB8].copy_from_slice(&original.0[0x98..0xA8]);
            bytes[0xC0..0xD8].fill(0);
            for (from, to) in old_stages.into_iter().zip(STAGES) {
                bytes[to..to + 0x98].fill(0);
                bytes[to + 0x10..to + 0x18].copy_from_slice(&original.0[from + 0x10..from + 0x18]);
                bytes[to + 0x68..to + 0x78].copy_from_slice(&original.0[from + 0x58..from + 0x68]);
                bytes[to + 0x78..to + 0x80].copy_from_slice(&original.0[from + 0x78..from + 0x80]);
            }
        }
        Some(2 | REVISION) => {}
        Some(other) => bail!("Unsupported imported shader scope layout {other}"),
    }
    let native = Payload(bytes.to_vec());
    ensure!(
        native.0[0x98..0xA8].iter().all(|b| *b == 0)
            && native.0[0xC0..0xD8].iter().all(|b| *b == 0),
        "Imported shader has unsupported native scope metadata"
    );
    ensure!(
        program.is_empty() || native.u32(0xAC)? & 0x10 != 0,
        "Imported shader animation lacks writable outputs. Import this shader again"
    );
    for at in STAGES {
        for offset in [0, 0x18, 0x28, 0x38, 0x48] {
            ensure!(
                native.u64(at + offset)? == 0,
                "Imported shader has an unsupported native stage at 0x{at:X}"
            );
        }
        ensure!(
            [0, u32::MAX, 0x811C9DC5].contains(&native.u32(at + 0x7C)?),
            "Imported shader has an unsupported native stage buffer"
        );
    }
    if revision != Some(REVISION) {
        super::emission::translate(bytes)?;
    }
    Ok(())
}

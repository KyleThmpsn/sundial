//! Source effect 30, `808020EB`, writes named animation selection inputs on its target. Native
//! effect 35, `80803E1C`, writes the same inputs.
//!
//! The eight-byte source record stores the retained flag at `+1`, the target at `+2`, a selector at
//! `+3` and the name at `+4`. Activation `71E680` writes builtin 3 for selector 3, and builtin 9
//! then builtin 3 for selector 9. Removal `71E890` writes the empty name to builtin 9 for selector
//! 9, or the default `42F51AF9` to builtin 3 for selector 3. Other selectors write nothing.
//!
//! Source builtin 9 is native builtin 14. The native Full Auto actions `80BBCBA0`, `80BC2902` and
//! `8162C891` write the source Full Auto name `75DDB3C9` there, and native `80BC2A92` writes the
//! empty name to builtin 3 as source `80A745F0` does. Each written builtin becomes one retained
//! native write in source order. Native removal `108E8B0` reapplies the name each write saved, so
//! cleanup matches the source when builtin 14 was empty and builtin 3 held the default. Native
//! activation writes nothing without retained state, so source selector 9's builtin 3, which
//! source removal leaves set, returns to its saved name instead.
use super::*;

pub(super) fn read(data: &Payload, at: usize) -> Result<Vec<Action>> {
    let target = data.u8(at + 2)?;
    ensure!(
        data.u8(at + 1)? == 1 && matches!(target, 0 | 2),
        "Source animation input has another lifecycle or target"
    );
    let builtins: &[u8] = match data.u8(at + 3)? {
        3 => &[3],
        9 => &[14, 3],
        _ => anyhow::bail!("Source animation input selector writes no builtin"),
    };
    let name = data.u32(at + 4)?;
    Ok(builtins
        .iter()
        .map(|&builtin| {
            let mut bytes = vec![0; 12];
            bytes[..4].copy_from_slice(&[35, 1, target, builtin]);
            bytes[4..8].copy_from_slice(&name.to_le_bytes());
            Action::Native {
                node: NativeNode { kind: 35, bytes },
            }
        })
        .collect())
}

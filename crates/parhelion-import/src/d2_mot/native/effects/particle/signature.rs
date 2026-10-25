//! Repair decompiler declarations using the shader's actual packed inputs.
use crate::d2_mot::{native::shader::replace_once, payload::Payload};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub(super) fn restore(source: &str, bytecode: &[u8]) -> Result<String> {
    let p = Payload(bytecode.to_vec());
    ensure!(
        p.bytes::<4>(0)? == *b"DXBC" && p.u32(24)? as usize == bytecode.len(),
        "particle shader bytecode envelope differs"
    );
    let chunks = usize::try_from(p.u32(28)?)?;
    ensure!(
        chunks <= 128,
        "particle shader chunk count exceeds capacity"
    );
    let mut signature = None;
    for index in 0..chunks {
        let at = usize::try_from(p.u32(32 + index * 4)?)?;
        let size = usize::try_from(p.u32(at + 4)?)?;
        let start = at.checked_add(8).context("shader chunk overflow")?;
        let end = start.checked_add(size).context("shader chunk overflow")?;
        let chunk = bytecode.get(start..end).context("shader chunk extent")?;
        if p.bytes::<4>(at)? == *b"ISGN" {
            ensure!(signature.is_none(), "ambiguous shader input signature");
            signature = Some(Payload(chunk.to_vec()));
        }
    }
    let signature = signature.context("particle shader input signature absent")?;
    let count = usize::try_from(signature.u32(0)?)?;
    ensure!(count <= 64, "particle shader input count exceeds capacity");
    let mut inputs = BTreeMap::new();
    for index in 0..count {
        let at = 8 + index * 24;
        signature.bytes::<24>(at)?;
        let name = usize::try_from(signature.u32(at)?)?;
        let tail = signature
            .0
            .get(name..)
            .context("shader semantic name extent")?;
        let end = tail
            .iter()
            .position(|byte| *byte == 0)
            .context("unterminated shader semantic")?;
        if &tail[..end] == b"TEXCOORD" {
            ensure!(
                signature.u32(at + 8)? == 0 && signature.u32(at + 12)? == 3,
                "particle interpolant is not an ordinary float input"
            );
            ensure!(
                inputs
                    .insert(
                        signature.u32(at + 4)?,
                        (signature.u32(at + 16)?, signature.u8(at + 20)?)
                    )
                    .is_none(),
                "duplicate particle interpolant"
            );
        }
    }
    // When every interpolant has a register of its own, only declaration widths can differ: the
    // decompiler declares each as float4 whatever its mask. Each takes its mask's width.
    let registers = inputs
        .values()
        .map(|(register, _)| *register)
        .collect::<std::collections::BTreeSet<_>>();
    if registers.len() == inputs.len() {
        let mut source = source.to_owned();
        for (semantic, (register, mask)) in &inputs {
            let width = match mask {
                0b1 => "float",
                0b11 => "float2",
                0b111 => "float3",
                0b1111 => continue,
                _ => anyhow::bail!("particle interpolant {semantic} has a non-leading mask"),
            };
            let declared = format!("float4 v{register} : TEXCOORD{semantic},");
            if source.contains(&declared) {
                source = replace_once(
                    &source,
                    &declared,
                    &format!("{width} v{register} : TEXCOORD{semantic},"),
                )?;
            }
        }
        return Ok(source);
    }
    ensure!(
        inputs.get(&1) == Some(&(1, 7))
            && inputs.get(&6) == Some(&(1, 8))
            && inputs.get(&2) == Some(&(2, 7)),
        "particle packed input signature differs"
    );
    ensure!(
        source.contains("float3 v1 : TEXCOORD1,") && source.contains("float w1 : TEXCOORD6,"),
        "particle packed input declarations differ"
    );
    // The instruction addresses physical register 1.xww. Its x lane belongs
    // to TEXCOORD1 and its w lane belongs to the scalar TEXCOORD6 input.
    let source = replace_once(source, "w1.xww", "float3(v1.x, w1, w1)")?;
    replace_once(&source, "float4 v2 : TEXCOORD2,", "float3 v2 : TEXCOORD2,")
}

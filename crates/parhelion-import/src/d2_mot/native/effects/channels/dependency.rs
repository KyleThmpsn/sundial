//! Channel dependency arrays store 32-bit words in declaration-ordinal order.
use super::{Payload, append_array};
use anyhow::{Result, ensure};

fn words(p: &Payload, field: usize, count: usize) -> Result<Vec<u32>> {
    let rows = p.array(field, 4, Some(0x8080000B))?;
    ensure!(
        rows.is_empty() || rows.len() == count.div_ceil(32),
        "channel dependency mask has {} words for {count} declarations, expected {}",
        rows.len(),
        count.div_ceil(32)
    );
    rows.into_iter().map(|at| p.u32(at)).collect()
}

pub(super) fn read(p: &Payload, field: usize, count: usize) -> Result<Vec<usize>> {
    let mut dependencies = Vec::new();
    for (index, mut word) in words(p, field, count)?.into_iter().enumerate() {
        while word != 0 {
            let ordinal = index * 32 + word.trailing_zeros() as usize;
            ensure!(
                ordinal < count,
                "channel dependency {ordinal} is outside the {count}-declaration table"
            );
            dependencies.push(ordinal);
            word &= word - 1;
        }
    }
    Ok(dependencies)
}

fn write_words(p: &mut Payload, field: usize, words: Vec<u32>) -> Result<()> {
    let bytes = words
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    append_array(&mut p.0, field, 0x8080000B, &bytes, 4)?;
    Ok(())
}

pub(super) fn write(
    p: &mut Payload,
    field: usize,
    count: usize,
    dependencies: &[usize],
) -> Result<()> {
    let mut words = if dependencies.is_empty() {
        Vec::new()
    } else {
        vec![0u32; count.div_ceil(32)]
    };
    for &ordinal in dependencies {
        ensure!(
            ordinal < count,
            "channel dependency exceeds declaration table"
        );
        words[ordinal / 32] |= 1 << (ordinal % 32);
    }
    write_words(p, field, words)
}

pub(super) fn extend_pending(
    p: &mut Payload,
    field: usize,
    before: usize,
    after: usize,
) -> Result<(usize, usize)> {
    ensure!(
        after >= before,
        "channel pending mask cannot shrink its declaration table"
    );
    let mut pending = words(p, field, before)?;
    let old_words = pending.len();
    // Pending masks may mark unused trailing bits. Preserve the native initial
    // state, including an absent mask, while marking appended declarations dirty.
    if !pending.is_empty() {
        pending.resize(after.div_ceil(32), 0);
        for ordinal in before..after {
            pending[ordinal / 32] |= 1 << (ordinal % 32);
        }
    }
    let new_words = pending.len();
    write_words(p, field, pending)?;
    Ok((old_words, new_words))
}

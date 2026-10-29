//! The source mode-three family has zero damage and two named health categories.
use super::*;

pub(super) const TEMPLATE: u32 = 0x8162C86E;

pub(super) fn read(
    source: &Source,
    native: &mut Reader,
    owner: &Payload,
    expected: &mut [u8],
) -> Result<HealthResponse> {
    let root = owner.pointer(24)?;
    let rows = owner.array(root + 0x28, 8, Some(0x8080276E))?;
    ensure!(rows.len() == 1, "Source healing needs one response");
    let at = owner.pointer(rows[0])?;
    ensure!(
        at == root + 0x14C
            && owner.u32(at - 4)? == 0x80802976
            && owner.bytes::<4>(at)? == [3, 1, 1, 0],
        "Source healing has another response mode or flags"
    );
    let health = HealthResponse {
        amount_bits: owner.u32(at + 4)?,
        overflow_bits: owner.u32(at + 8)?,
    };
    ensure!(
        health.valid(),
        "Source healing has invalid response operands"
    );
    let names = [0x4EFCDCF5, 0xCD887E0F];
    let labels = owner.array(root + 0xC8, 24, Some(0x80800365))?;
    ensure!(
        labels.len() == names.len(),
        "Source healing has other categories"
    );
    let mut mask = [0u8; 32];
    let mut dictionary = None;
    for (&row, name) in labels.iter().zip(names) {
        ensure!(owner.u32(row)? == name, "Source healing category differs");
        let tag = owner.u32(row + 16)?;
        ensure!(
            dictionary.is_none_or(|previous| previous == tag),
            "Source healing mixes dictionaries"
        );
        dictionary = Some(tag);
        let table = source.read(tag)?;
        let atoms = table.array(8, 4, Some(0x80800070))?;
        ensure!(
            !atoms.is_empty() && atoms.len() <= 256,
            "Source category dictionary width differs"
        );
        let matches = atoms
            .iter()
            .enumerate()
            .filter(|(_, r)| table.u32(**r).ok() == Some(name))
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "Source healing category is absent or ambiguous"
        );
        mask[matches[0] / 8] |= 1 << (matches[0] % 8);
    }
    let registry = native.tag(0x80C70CA1, None)?;
    ability_damage::healing_labels(&registry.0).map_err(anyhow::Error::msg)?;
    for (offset, value) in [
        (0x28, 1u64),
        (0x30, 0x100),
        (0xC8, 2),
        (0xD0, 0x90),
        (0x130, 1),
        (0x140, 0xC),
        (0x160, 2),
    ] {
        put(expected, root + offset, &value.to_le_bytes())?;
    }
    for (offset, value) in [
        (0xF0, 1u32),
        (0x12C, 0x80800184),
        (0x138, 0x8080276E),
        (0x148, 0x80802976),
        (0x14C, 0x10103),
        (0x150, health.amount_bits),
        (0x154, health.overflow_bits),
        (0x15C, 0x80800184),
        (0x168, 0x80800365),
        (0x170, names[0]),
        (0x180, dictionary.context("Source healing dictionary")?),
        (0x188, names[1]),
        (0x198, dictionary.context("Source healing dictionary")?),
    ] {
        put(expected, root + offset, &value.to_le_bytes())?;
    }
    put(expected, root + 0x90, &mask)?;
    Ok(health)
}

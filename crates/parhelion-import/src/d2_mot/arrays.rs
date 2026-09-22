//! Canonical headers for imported item appearance arrays. No in-place padding guesses.
fn read<const N: usize>(b: &[u8], at: usize) -> Result<[u8; N], String> {
    b.get(at..at.checked_add(N).ok_or("offset overflow")?)
        .ok_or_else(|| "array field outside payload".into())
        .map(|s| s.try_into().unwrap())
}
fn u64_at(b: &[u8], at: usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(read(b, at)?))
}
fn pointer(b: &[u8], at: usize) -> Result<usize, String> {
    let n = at as i128 + i64::from_le_bytes(read(b, at)?) as i128;
    if n < 0 || n >= b.len() as i128 {
        return Err("array pointer outside payload".into());
    }
    Ok(n as usize)
}
fn descriptors(b: &[u8]) -> Result<[usize; 4], String> {
    let at = pointer(b, 0x88)?;
    Ok([at, at + 40, at + 56, at + 72])
}
fn row(b: &[u8], at: usize, class: u32) -> Result<Option<(usize, usize)>, String> {
    let count = u64_at(b, at)?;
    if count == 0 {
        return Ok(None);
    }
    if count > 32 {
        return Err("appearance row count exceeds limit".into());
    }
    let h = pointer(b, at + 8)?;
    if u64_at(b, h)? != count || u32::from_le_bytes(read(b, h + 8)?) != class {
        return Err("appearance header class/count mismatch".into());
    }
    let end = h
        .checked_add(16 + count as usize * 4)
        .ok_or("array size overflow")?;
    if end > b.len() {
        return Err("appearance rows outside payload".into());
    }
    Ok(Some((h, end)))
}
pub fn validate(b: &[u8]) -> Result<(), String> {
    for (i, at) in descriptors(b)?.into_iter().enumerate() {
        if let Some((h, _)) = row(b, at, if i == 0 { 0x808077B5 } else { 0x808077B3 })? {
            if h < 4 || u32::from_le_bytes(read(b, h - 4)?) >> 16 != 0x8080 {
                return Err(format!(
                    "appearance array at {at:X} lacks native header marker"
                ));
            }
        }
    }
    Ok(())
}
pub fn repair(b: &mut Vec<u8>) -> Result<usize, String> {
    let mut repaired = 0;
    for (i, at) in descriptors(b)?.into_iter().enumerate() {
        let Some((h, end)) = row(b, at, if i == 0 { 0x808077B5 } else { 0x808077B3 })? else {
            continue;
        };
        if h >= 4 && u32::from_le_bytes(read(b, h - 4)?) >> 16 == 0x8080 {
            continue;
        }
        let payload = b[h..end].to_vec();
        let next = (b.len() + 19) & !15;
        b.resize(next - 4, 0);
        b.extend_from_slice(&0x80809FBDu32.to_le_bytes());
        b.extend_from_slice(&payload);
        b[at + 8..at + 16].copy_from_slice(&((next as i64) - (at + 8) as i64).to_le_bytes());
        repaired += 1;
    }
    let len = b.len() as u64;
    b[..8].copy_from_slice(&len.to_le_bytes());
    validate(b)?;
    Ok(repaired)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_marker_is_rejected_and_relocated_without_clobbering_neighbors() {
        let mut b = vec![0; 0x170];
        b[0x88..0x90].copy_from_slice(&0x18i64.to_le_bytes());
        b[0xA0..0xA8].copy_from_slice(&1u64.to_le_bytes());
        b[0xA8..0xB0].copy_from_slice(&0xA8i64.to_le_bytes());
        b[0x150..0x158].copy_from_slice(&1u64.to_le_bytes());
        b[0x158..0x15C].copy_from_slice(&0x808077B5u32.to_le_bytes());
        b[0x160..0x164].copy_from_slice(&[255, 0, 206, 16]);
        b[0x14C..0x150].copy_from_slice(&0x12345678u32.to_le_bytes());
        assert!(validate(&b).is_err());
        assert_eq!(repair(&mut b).unwrap(), 1);
        validate(&b).unwrap();
        assert_eq!(&b[0x14C..0x150], &0x12345678u32.to_le_bytes());
        let h = pointer(&b, 0xA8).unwrap();
        assert_eq!(&b[h + 16..h + 20], &[255, 0, 206, 16]);
        assert_eq!(repair(&mut b).unwrap(), 0);
    }
}

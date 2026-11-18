//! Loading words must compose as independent append operations. Several mods cannot each use
//! the spare bits in the last stock word, since the loader includes that word only once.
use crate::package_payload::{native_array_at, rows_fit, u32_at, write_bytes};

fn array(
    bytes: &[u8],
    descriptor: usize,
    class: u32,
    stride: usize,
) -> Result<(usize, usize), String> {
    let (count, header, rows, actual) = native_array_at(bytes, descriptor)?;
    if actual != class
        || header < 0x30
        || !header.is_multiple_of(8)
        || u32_at(bytes, header - 4)? != 0x8080_9FBD
    {
        return Err("Runtime name map has an unsupported array layout".into());
    }
    rows_fit(bytes, rows, count, stride)?;
    Ok((count, rows))
}

fn counts(bytes: &[u8]) -> Result<(usize, usize, usize), String> {
    let (names, name_rows) = array(bytes, 8, 0x8080_9252, 8)?;
    let (words, word_rows) = array(bytes, 24, 0x8080_000B, 4)?;
    if words < names.div_ceil(32)
        || (name_rows - 20 < word_rows + words * 4 && word_rows - 20 < name_rows + names * 8)
        || bytes[word_rows..word_rows + words * 4]
            .iter()
            .any(|byte| *byte != 0)
    {
        return Err("Runtime name map has invalid loading bits".into());
    }
    Ok((names, words, word_rows))
}

/// Gives a mod one new zeroed word per group of up to 32 names it adds. The loader appends
/// each member's word delta independently of its sorted name rows. Stock slack belongs to
/// the stock map alone. Existing larger reservations remain valid and are never reduced.
pub fn reserve(stock: &[u8], built: &mut Vec<u8>) -> Result<(), String> {
    let (stock_names, stock_words, _) = counts(stock)?;
    let (names, words, rows) = counts(built)?;
    let added = names
        .checked_sub(stock_names)
        .ok_or("Runtime name map removes stock rows")?;
    let required = stock_words
        .checked_add(added.div_ceil(32))
        .ok_or("Runtime loading word count overflowed")?;
    if words < stock_words {
        return Err("Runtime name map removes stock loading words".into());
    }
    if words >= required {
        return Ok(());
    }
    let header = built
        .len()
        .checked_add(19)
        .ok_or("Runtime name map size overflowed")?
        & !15;
    let end = required
        .checked_mul(4)
        .and_then(|size| header.checked_add(16)?.checked_add(size))
        .filter(|end| *end <= u32::MAX as usize)
        .ok_or("Runtime name map exceeds the package entry size")?;
    let mut prefix = built[rows - 16..rows].to_vec();
    prefix[..8].copy_from_slice(&(required as u64).to_le_bytes());
    built.resize(end, 0);
    write_bytes(built, header - 4, &0x8080_9FBDu32.to_le_bytes())?;
    write_bytes(built, header, &prefix)?;
    write_bytes(built, 24, &(required as u64).to_le_bytes())?;
    write_bytes(built, 32, &((header - 32) as i64).to_le_bytes())?;
    write_bytes(built, 0, &(end as u64).to_le_bytes())?;
    Ok(())
}

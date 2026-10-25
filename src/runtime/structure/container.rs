//! Package array containers omitted by wire operation 32.
use super::*;

#[cfg(test)]
mod tests;

pub(super) fn read(
    data: &[u8],
    start: usize,
    relative: usize,
    end: usize,
    declaration: &Record,
) -> Result<Option<(String, String)>, String> {
    // A wire no-op alone says nothing about storage. Require the independently
    // declared typed pointer immediately after the native count, then verify
    // its concrete array header and the duplicated package element count.
    let Some(pointer) = relative.checked_add(8) else {
        return Ok(None);
    };
    if !declaration.fields.contains(&(pointer, 3)) {
        return Ok(None);
    }
    let at = start
        .checked_add(relative)
        .ok_or("Array container offset overflow")?;
    let pointer = start
        .checked_add(pointer)
        .ok_or("Array pointer offset overflow")?;
    if pointer.checked_add(8).is_none_or(|last| last > end) {
        return Err("Native array container exceeds its declaring element".into());
    }
    let count = read_u64(data, at)?;
    if read_i64(data, pointer)? == 0 {
        return if count == 0 {
            Ok(Some((
                "Array Element Count".into(),
                "0 (empty typed array)".into(),
            )))
        } else {
            Err("Native array container has a count but no typed array".into())
        };
    }
    let target = relative_target(data, pointer)?;
    let marker = target
        .checked_sub(4)
        .ok_or("Array container has no type header")?;
    if read_u32(data, marker)? != 0x8080_9FBD {
        return Ok(None);
    }
    if read_u64(data, target)? != count {
        return Err("Native array container count disagrees with its typed array".into());
    }
    Ok(Some((
        "Array Element Count".into(),
        format!("{count} entries of 0x{:08X}", read_u32(data, target + 8)?),
    )))
}

//! Follow checked native type headers and their bounded array storage.
use super::*;

pub(super) struct Pointer {
    pub representation: &'static str,
    pub value: String,
    pub children: Vec<(usize, u32, Option<usize>)>,
}

pub(super) fn read(
    data: &[u8],
    at: usize,
    remaining: usize,
    record: &mut impl FnMut(u32) -> Result<Record, String>,
) -> Result<Pointer, String> {
    if read_i64(data, at)? == 0 {
        return Ok(Pointer {
            representation: "Typed Pointer",
            value: "None".into(),
            children: Vec::new(),
        });
    }
    let target = relative_target(data, at)?;
    let marker = target
        .checked_sub(4)
        .ok_or("Native pointer has no type header")?;
    let child = read_u32(data, marker)?;
    if child != 0x8080_9FBD {
        return Ok(Pointer {
            representation: "Typed Pointer",
            value: format!("Type 0x{child:08X} at owner +0x{target:X}"),
            children: vec![(target, child, None)],
        });
    }
    let count = usize::try_from(read_u64(data, target)?).map_err(|_| "Array count overflow")?;
    let child = read_u32(data, target + 8)?;
    let stride = record(child)?.size;
    let rows = target.checked_add(16).ok_or("Array offset overflow")?;
    if stride == 0
        || count > MAX_OBJECTS
        || count
            .checked_mul(stride)
            .and_then(|size| rows.checked_add(size))
            .is_none_or(|end| end > data.len())
    {
        return Err("Native array has invalid bounds or stride".into());
    }
    if count > remaining {
        return Err("Native array exceeds the inspection limit".into());
    }
    Ok(Pointer {
        representation: "Typed Array",
        value: format!("{count} entries of 0x{child:08X}, stride {stride}, owner +0x{rows:X}"),
        children: (0..count)
            .map(|i| (rows + i * stride, child, None))
            .collect(),
    })
}

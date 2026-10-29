//! Construct inactive sequence consumers independently of the source format.
use super::*;

pub struct Written {
    pub runtime: usize,
    pub definition: usize,
    pub patches: Vec<(usize, String)>,
}

/// The containing graph must connect this consumer to a checked native sequence provider.
pub fn write(
    target: &mut Payload,
    runtime: usize,
    definition: usize,
    parent: usize,
    symbol: &str,
) -> Result<Written> {
    ensure!(
        !symbol.is_empty()
            && symbol
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_')),
        "Native sequence input owner symbol differs"
    );
    let owner = target.u32(parent)?;
    ensure!(owner != u32::MAX, "Native sequence input has no owner");
    target.bytes::<80>(runtime)?;
    target.bytes::<48>(definition)?;
    ensure!(
        parent < runtime && runtime + 80 <= definition,
        "Native sequence input storage overlaps its definition or parent"
    );
    let mut live = [0u8; 80];
    for field in [0x20, 0x24, 0x30, 0x38, 0x3C] {
        put(&mut live, field, &u32::MAX.to_le_bytes())?;
    }
    let mut settings = [0u8; 48];
    put(&mut settings, 24, &0x808092F7u64.to_le_bytes())?;
    put(&mut settings, 40, &u64::from(u32::MAX).to_le_bytes())?;
    let mut output = target.clone();
    put(&mut output.0, runtime, &live)?;
    put(&mut output.0, definition, &settings)?;
    pair(
        &mut output,
        owner,
        runtime,
        definition,
        0x808092F3,
        0x808092F4,
    )?;
    relative(&mut output, runtime + 16, parent)?;
    *target = output;
    Ok(Written {
        runtime,
        definition,
        patches: vec![(runtime, symbol.into()), (definition, symbol.into())],
    })
}

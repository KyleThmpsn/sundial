//! Read damage declarations and native ability masks through emitted references.
use super::*;

pub(super) fn damage(
    reader: &Source,
    stock: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    for operation in 0..2 {
        let original = source.array(s + operation * 16, 1, None)?;
        let emitted = native.array(n + operation * 16, 1, Some(0x80806B02))?;
        let original = original
            .into_iter()
            .map(|r| source.u8(r))
            .collect::<Result<Vec<_>>>()?;
        let emitted = emitted
            .into_iter()
            .map(|r| native.u8(r))
            .collect::<Result<Vec<_>>>()?;
        assert_eq!(original, emitted);
        let mask = original
            .iter()
            .fold(0u32, |mask, &value| mask | (1 << value));
        assert_eq!(source.u32(s + 0x40 + operation * 4)?, mask);
        assert_eq!(native.u32(n + 0x70 + operation * 4)?, mask);
        assert_eq!(
            native.u8(n + 0x88 + operation)?,
            u8::from(original.is_empty())
        );
    }
    let dictionary = stock.tag(native.u32(n + 0x68)?, None)?;
    let atoms = dictionary.array(8, 4, None)?;
    let groups = dictionary.array(24, 44, None)?;
    let mode = native.u8(n + 0x78)?;
    assert!(matches!(mode, 0 | 255));
    for (op, target_op) in [(0, 0), (1, 2)] {
        let rows = source.array(s + 0x20 + op * 16, 24, Some(0x80800365))?;
        let mut original = Vec::new();
        let mut source_mask = [0u8; 32];
        if let Some(&first) = rows.first() {
            let tag = source.u32(first + 16)?;
            let dictionary = reader.read(tag)?;
            let atoms = dictionary.array(8, 4, Some(0x80800070))?;
            let groups = dictionary.array(24, 36, Some(0x80800513))?;
            for row in rows {
                assert_eq!(source.u32(row + 16)?, tag);
                let name = source.u32(row)?;
                let mut bits = [0u8; 32];
                if let Some(index) = atoms
                    .iter()
                    .position(|&a| dictionary.u32(a).ok() == Some(name))
                {
                    bits[index / 8] = 1 << (index % 8);
                } else if let Some(&group) = groups
                    .iter()
                    .find(|&&g| dictionary.u32(g).ok() == Some(name))
                {
                    bits = dictionary.bytes::<32>(group + 4)?;
                } else {
                    assert_eq!(name, 0x811C9DC5, "Unregistered source ability label");
                }
                if bits.iter().any(|&bit| bit != 0) {
                    original.push(name);
                }
                for (out, bits) in source_mask.iter_mut().zip(bits) {
                    *out |= bits;
                }
            }
        }
        assert_eq!(source.bytes::<32>(s + 0x48 + op * 32)?, source_mask);
        assert_eq!(source.u8(s + 0x8A + op)?, u8::from(source_mask == [0; 32]));
        let emitted = native
            .array(n + 0x20 + target_op * 16, 24, Some(0x808094B3))?
            .into_iter()
            .map(|r| native.u32(r))
            .collect::<Result<Vec<_>>>()?;
        assert_eq!(original, emitted);
        let mut mask = [0u8; 40];
        for name in &original {
            if let Some(index) = atoms
                .iter()
                .position(|&r| dictionary.u32(r).ok() == Some(*name))
            {
                mask[index / 8] |= 1 << (index % 8);
            } else {
                let row = groups
                    .iter()
                    .find(|&&r| dictionary.u32(r).ok() == Some(*name))
                    .ok_or("Emitted event label is unregistered")?;
                for (out, bits) in mask.iter_mut().zip(dictionary.bytes::<40>(row + 4)?) {
                    *out |= bits;
                }
            }
        }
        if mode == 255 {
            assert!(original.is_empty());
        } else {
            let compiled = native.pointer(n + 0x80)?;
            assert_eq!(native.bytes::<40>(compiled + op * 40)?, mask);
            assert_eq!(
                native.u8(compiled + 80 + op)?,
                u8::from(original.is_empty())
            );
        }
    }
    assert_eq!(native.u64(n + 0x30)?, 0);
    assert_eq!(native.u64(n + 0x50)?, 0);
    Ok(())
}

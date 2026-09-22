use crate::d2_mot::payload::Payload;
use anyhow::{Result, ensure};

pub(super) fn validate(model: &Payload, mesh: usize, parts: &[usize]) -> Result<()> {
    for stage in 0..23 {
        let start = model.u16(mesh + 40 + stage * 2)? as usize;
        let end = model.u16(mesh + 42 + stage * 2)? as usize;
        ensure!(
            start <= end && end <= parts.len(),
            "draw stage {stage} range is invalid"
        );
        let mut index = start;
        while index < end {
            let count = model.u8(parts[index] + 29)? as usize;
            ensure!(
                count > 0,
                "draw stage {stage}, part {index} has zero group length and stalls the native iterator"
            );
            ensure!(count <= end - index, "draw group exceeds its stage range");
            index += count;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_iterator_requires_progress_within_each_stage() {
        let mut bytes = vec![0; 160];
        for stage in 1..24 {
            bytes[40 + stage * 2..42 + stage * 2].copy_from_slice(&2u16.to_le_bytes());
        }
        let mut model = Payload(bytes);
        assert!(validate(&model, 0, &[96, 128]).is_err());
        model.0[96 + 29] = 1;
        model.0[128 + 29] = 1;
        assert!(validate(&model, 0, &[96, 128]).is_ok());
        model.0[96 + 29] = 2;
        assert!(validate(&model, 0, &[96, 128]).is_ok());
        model.0[96 + 29] = 3;
        assert!(validate(&model, 0, &[96, 128]).is_err());
    }
}

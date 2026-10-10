//! Scalar constant lanes, established independently on movement and damage twins.

/// Modern scalar pushes may leave the other lanes zero. The native serialized
/// row carries signed ones there, except for an entirely zero constant. Call
/// after validating the program and its constant indices.
pub(in crate::d2_mot::native::effects) fn broadcast(code: &[u8], rows: &mut [u8]) {
    let mut at = 0;
    while at < code.len() {
        let op = code[at];
        if op == 0x29 || (0x42..=0x4F).contains(&op) {
            if op == 0x42 && at + 1 < code.len() {
                let index = usize::from(code[at + 1]) * 16;
                if let Some(row) = rows.get_mut(index..index + 16)
                    && row[4..] == [0; 12]
                    && row[..4] != [0; 4]
                {
                    let x = f32::from_le_bytes(row[..4].try_into().unwrap());
                    let lane = 1f32.copysign(x).to_le_bytes();
                    for n in 1..4 {
                        row[n * 4..n * 4 + 4].copy_from_slice(&lane);
                    }
                }
            }
            at += 2;
        } else {
            at += 1;
        }
    }
}

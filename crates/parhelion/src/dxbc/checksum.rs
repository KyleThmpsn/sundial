//! Native DXBC checksum, using the same container algorithm as particle translation.
use crate::{AuthoringResult, error::invalid};

fn md5_block(state: &mut [u32; 4], block: &[u8]) {
    const SHIFT: [u32; 16] = [7, 12, 17, 22, 5, 9, 14, 20, 4, 11, 16, 23, 6, 10, 15, 21];
    let words: Vec<u32> = block
        .chunks_exact(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect();
    let [mut a, mut b, mut c, mut d] = *state;
    for i in 0..64 {
        let (f, g) = match i / 16 {
            0 => ((b & c) | (!b & d), i),
            1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
            2 => (b ^ c ^ d, (3 * i + 5) % 16),
            _ => (c ^ (b | !d), (7 * i) % 16),
        };
        let k = ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32;
        let rotated = f
            .wrapping_add(a)
            .wrapping_add(k)
            .wrapping_add(words[g])
            .rotate_left(SHIFT[(i / 16) * 4 + i % 4]);
        a = d;
        d = c;
        c = b;
        b = b.wrapping_add(rotated);
    }
    for (value, add) in state.iter_mut().zip([a, b, c, d]) {
        *value = value.wrapping_add(add);
    }
}

/// The DXBC container hash: MD5 rounds over everything after the hash field, with the bit
/// length stored ahead of the tail and a derived length in the final word.
pub(super) fn compute(container: &[u8]) -> AuthoringResult<[u8; 16]> {
    let data = container
        .get(20..)
        .ok_or_else(|| invalid("Shader container header is truncated"))?;
    let bits = u32::try_from(data.len() * 8)
        .map_err(|_| invalid("Shader container exceeds checksum size"))?;
    let tail_bits = (bits >> 2) | 1;
    let mut state = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476];
    let full = data.len() - data.len() % 64;
    for block in data[..full].chunks_exact(64) {
        md5_block(&mut state, block);
    }
    let left = &data[full..];
    let mut block = Vec::with_capacity(128);
    if left.len() < 56 {
        block.extend(bits.to_le_bytes());
        block.extend_from_slice(left);
        block.push(0x80);
        block.resize(60, 0);
        block.extend(tail_bits.to_le_bytes());
        md5_block(&mut state, &block);
    } else {
        block.extend_from_slice(left);
        block.push(0x80);
        block.resize(64, 0);
        md5_block(&mut state, &block);
        let mut last = bits.to_le_bytes().to_vec();
        last.resize(60, 0);
        last.extend(tail_bits.to_le_bytes());
        md5_block(&mut state, &last);
    }
    let mut out = [0; 16];
    for (i, word) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    Ok(out)
}

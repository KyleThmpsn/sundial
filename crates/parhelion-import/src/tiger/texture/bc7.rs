//! BC7 encoding of RGBA8 colour art, the block format stock keeps most effect and model art in.
//!
//! BC7 (DXGI 98, or 99 for sRGB) keeps each 4 by 4 texel block in 16 bytes, a quarter of RGBA8,
//! and keeps all four channels. Each block is written in whichever of modes 1, 3, 5, 6 and 7
//! decodes with the least squared RGBA error. Mode 6 holds one gradient with alpha. Mode 5 keeps
//! alpha, or a colour channel rotated into its place, on indices of its own. Modes 1 and 3 hold
//! opaque blocks of two colour groups, and mode 7 two groups with alpha, over the 64 two-subset
//! partitions. Every BC7 decoder reads all eight modes, so leaving out modes 0, 2 and 4 limits only
//! how closely a block can match its source.
//!
//! Endpoints start at the ends of a subset's spread along its principal axis, are refined by least
//! squares against the indices they select and are quantized with the best p-bits. Interpolation,
//! endpoint expansion and the bit layout follow the D3D11 BC7 format, so the error a block reports
//! is the error of its decoded texels.
use std::ops::Range;

/// One RGBA8 texel.
type Texel = [u8; 4];

/// The weights out of 64 that 2-, 3- and 4-bit indices give the second endpoint.
const WEIGHTS2: [u32; 4] = [0, 21, 43, 64];
const WEIGHTS3: [u32; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
const WEIGHTS4: [u32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

/// The two-subset partitions. Bit `i` is set when texel `i`, in row order, is in the second
/// subset. Texel 0 is always in the first.
const PARTITIONS: [u16; 64] = [
    0xCCCC, 0x8888, 0xEEEE, 0xECC8, 0xC880, 0xFEEC, 0xFEC8, 0xEC80, 0xC800, 0xFFEC, 0xFE80, 0xE800,
    0xFFE8, 0xFF00, 0xFFF0, 0xF000, 0xF710, 0x008E, 0x7100, 0x08CE, 0x008C, 0x7310, 0x3100, 0x8CCE,
    0x088C, 0x3110, 0x6666, 0x366C, 0x17E8, 0x0FF0, 0x718E, 0x399C, 0xAAAA, 0xF0F0, 0x5A5A, 0x33CC,
    0x3C3C, 0x55AA, 0x9696, 0xA55A, 0x73CE, 0x13C8, 0x324C, 0x3BDC, 0x6996, 0xC33C, 0x9966, 0x0660,
    0x0272, 0x04E4, 0x4E40, 0x2720, 0xC936, 0x936C, 0x39C6, 0x639C, 0x9336, 0x9CC6, 0x817E, 0xE718,
    0xCCF0, 0x0FCC, 0x7744, 0xEE22,
];
/// The texel of each partition's second subset whose index is stored one bit short. The first
/// subset's is texel 0.
const ANCHORS: [u8; 64] = [
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 2, 8, 2, 2, 8, 8, 15, 2, 8,
    2, 2, 8, 8, 2, 2, 15, 15, 6, 8, 2, 8, 15, 15, 2, 8, 2, 2, 2, 15, 15, 6, 6, 2, 6, 8, 15, 15, 2,
    2, 15, 15, 15, 15, 15, 2, 2, 15,
];

/// A block whose error is at most this, a squared level per channel and texel, keeps mode 6 or 5
/// without trying the two-subset modes.
const CLOSE: u32 = 64;
/// The best-estimated partitions each two-subset mode is fitted to.
const PARTITION_TRIALS: usize = 3;
/// Least-squares passes after the principal-axis fit.
const REFINEMENTS: usize = 2;

/// How a mode stores the channels of an endpoint pair.
#[derive(Clone, Copy)]
struct Precision {
    /// Stored bits per channel, before any p-bit.
    bits: u32,
    /// The p-bits, each a low bit shared by every channel of an endpoint.
    pbits: Pbits,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pbits {
    Absent,
    /// One for both endpoints of a subset.
    PerSubset,
    /// One for each endpoint.
    PerEndpoint,
}

/// Mode 6: one RGBA pair of seven bits and a p-bit each, with 4-bit indices.
const MODE6: Precision = Precision {
    bits: 7,
    pbits: Pbits::PerEndpoint,
};
/// Mode 5: a seven-bit RGB pair and an eight-bit alpha pair, each with 2-bit indices.
const MODE5_COLOR: Precision = Precision {
    bits: 7,
    pbits: Pbits::Absent,
};
const MODE5_ALPHA: Precision = Precision {
    bits: 8,
    pbits: Pbits::Absent,
};

/// A two-subset mode: its number, stored channels, endpoint precision and index bits.
struct Paired {
    mode: u32,
    channels: Range<usize>,
    precision: Precision,
    index_bits: u32,
}

/// Opaque, six bits and a subset p-bit, 3-bit indices.
const MODE1: Paired = Paired {
    mode: 1,
    channels: 0..3,
    precision: Precision {
        bits: 6,
        pbits: Pbits::PerSubset,
    },
    index_bits: 3,
};
/// Opaque, seven bits and an endpoint p-bit, 2-bit indices.
const MODE3: Paired = Paired {
    mode: 3,
    channels: 0..3,
    precision: Precision {
        bits: 7,
        pbits: Pbits::PerEndpoint,
    },
    index_bits: 2,
};
/// With alpha, five bits and an endpoint p-bit, 2-bit indices.
const MODE7: Paired = Paired {
    mode: 7,
    channels: 0..4,
    precision: Precision {
        bits: 5,
        pbits: Pbits::PerEndpoint,
    },
    index_bits: 2,
};

/// The BC7 blocks of a `width` by `height` RGBA8 surface, in row order. Blocks past an edge repeat
/// the surface's last column and row, so a level under four texels fills a whole block.
pub(crate) fn surface(width: usize, height: usize, rgba: &[u8]) -> Vec<u8> {
    let (columns, rows) = (width.div_ceil(4), height.div_ceil(4));
    let mut bytes = Vec::with_capacity(columns * rows * 16);
    for row in 0..rows {
        for column in 0..columns {
            bytes.extend_from_slice(&block(&gather(width, height, rgba, column, row)).bytes);
        }
    }
    bytes
}

/// The texels of block `column`, `row`, clamped to the surface.
fn gather(width: usize, height: usize, rgba: &[u8], column: usize, row: usize) -> [Texel; 16] {
    std::array::from_fn(|texel| {
        let x = (column * 4 + texel % 4).min(width - 1);
        let y = (row * 4 + texel / 4).min(height - 1);
        let at = (y * width + x) * 4;
        [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]]
    })
}

/// An encoded block and the squared error of its decoded texels over all four channels.
#[derive(Clone, Copy)]
struct Encoded {
    bytes: [u8; 16],
    error: u32,
}

fn better(current: Encoded, other: Encoded) -> Encoded {
    if other.error < current.error {
        other
    } else {
        current
    }
}

/// Encodes one block of texels in row order.
fn block(texels: &[Texel; 16]) -> Encoded {
    let mut best = single(texels);
    if best.error <= CLOSE {
        return best;
    }
    for rotation in 0..4 {
        best = better(best, separate(texels, rotation));
    }
    if best.error <= CLOSE {
        return best;
    }
    // Opaque two-subset modes decode alpha as 255, so they serve only blocks that are opaque.
    let opaque = texels.iter().all(|texel| texel[3] == 255);
    let channels = if opaque { 0..3 } else { 0..4 };
    for partition in ranked(texels, &channels) {
        if opaque {
            best = better(best, paired(texels, partition, &MODE1));
            best = better(best, paired(texels, partition, &MODE3));
        } else {
            best = better(best, paired(texels, partition, &MODE7));
        }
    }
    best
}

/// Mode 6.
fn single(texels: &[Texel; 16]) -> Encoded {
    let mut fit = fit(texels, &(0..4), MODE6, &WEIGHTS4);
    anchor(&mut fit, 0, 4, 16);
    let mut bits = Bits::default();
    bits.put(1 << 6, 7);
    for (low, high) in fit.codes[0].iter().zip(&fit.codes[1]) {
        bits.put(*low, 7);
        bits.put(*high, 7);
    }
    for &pbit in &fit.pbits {
        bits.put(pbit, 1);
    }
    for (texel, &index) in fit.indices.iter().enumerate() {
        bits.put(u32::from(index), if texel == 0 { 3 } else { 4 });
    }
    Encoded {
        bytes: bits.bytes(),
        error: fit.error,
    }
}

/// Mode 5 with `rotation`. Rotations 1 to 3 trade alpha with red, green or blue before encoding,
/// and the decoder trades them back, so a colour channel can take the separate indices.
fn separate(texels: &[Texel; 16], rotation: u32) -> Encoded {
    let rotated = texels.map(|texel| rotate(texel, rotation));
    let mut color = fit(&rotated, &(0..3), MODE5_COLOR, &WEIGHTS2);
    let mut alpha = fit(&rotated, &(3..4), MODE5_ALPHA, &WEIGHTS2);
    anchor(&mut color, 0, 2, 16);
    anchor(&mut alpha, 0, 2, 16);
    let mut bits = Bits::default();
    bits.put(1 << 5, 6);
    bits.put(rotation, 2);
    for (low, high) in color.codes[0][..3].iter().zip(&color.codes[1][..3]) {
        bits.put(*low, 7);
        bits.put(*high, 7);
    }
    bits.put(alpha.codes[0][3], 8);
    bits.put(alpha.codes[1][3], 8);
    for indices in [&color.indices, &alpha.indices] {
        for (texel, &index) in indices.iter().enumerate() {
            bits.put(u32::from(index), if texel == 0 { 1 } else { 2 });
        }
    }
    Encoded {
        bytes: bits.bytes(),
        error: color.error + alpha.error,
    }
}

fn rotate(mut texel: Texel, rotation: u32) -> Texel {
    if let Some(channel) = rotation.checked_sub(1) {
        texel.swap(channel as usize, 3);
    }
    texel
}

/// A two-subset mode under `partition`.
fn paired(texels: &[Texel; 16], partition: usize, mode: &Paired) -> Encoded {
    let mask = PARTITIONS[partition];
    let second_anchor = usize::from(ANCHORS[partition]);
    let weights = weights(mode.index_bits);
    let fits = [0, 1].map(|subset| {
        let (points, numbers, count) = members(texels, mask, subset);
        let mut fit = fit(&points[..count], &mode.channels, mode.precision, weights);
        let anchor_texel = if subset == 0 { 0 } else { second_anchor };
        let position = numbers[..count]
            .iter()
            .position(|&number| number == anchor_texel)
            .unwrap_or(0);
        anchor(&mut fit, position, mode.index_bits, count);
        fit
    });
    let mut bits = Bits::default();
    bits.put(1 << mode.mode, mode.mode + 1);
    bits.put(partition as u32, 6);
    for channel in mode.channels.clone() {
        for fit in &fits {
            bits.put(fit.codes[0][channel], mode.precision.bits);
            bits.put(fit.codes[1][channel], mode.precision.bits);
        }
    }
    for fit in &fits {
        match mode.precision.pbits {
            Pbits::PerSubset => bits.put(fit.pbits[0], 1),
            Pbits::PerEndpoint => {
                bits.put(fit.pbits[0], 1);
                bits.put(fit.pbits[1], 1);
            }
            Pbits::Absent => {}
        }
    }
    let mut next = [0usize; 2];
    for number in 0..16 {
        let subset = usize::from((mask >> number) & 1);
        let index = fits[subset].indices[next[subset]];
        next[subset] += 1;
        let short = number == 0 || number == second_anchor;
        bits.put(u32::from(index), mode.index_bits - u32::from(short));
    }
    Encoded {
        bytes: bits.bytes(),
        error: fits[0].error + fits[1].error,
    }
}

fn weights(index_bits: u32) -> &'static [u32] {
    match index_bits {
        2 => &WEIGHTS2,
        3 => &WEIGHTS3,
        _ => &WEIGHTS4,
    }
}

/// The texels of `subset` under partition `mask` in texel order, their texel numbers and count.
fn members(texels: &[Texel; 16], mask: u16, subset: u16) -> ([Texel; 16], [usize; 16], usize) {
    let mut points = [[0; 4]; 16];
    let mut numbers = [0; 16];
    let mut count = 0;
    for (number, texel) in texels.iter().enumerate() {
        if (mask >> number) & 1 == subset {
            points[count] = *texel;
            numbers[count] = number;
            count += 1;
        }
    }
    (points, numbers, count)
}

/// The partitions whose two subsets each lie closest to a line over `channels`, best first.
fn ranked(texels: &[Texel; 16], channels: &Range<usize>) -> [usize; PARTITION_TRIALS] {
    let mut scores: [(f32, usize); 64] = std::array::from_fn(|partition| {
        let score = [0, 1]
            .iter()
            .map(|&subset| {
                let (points, _, count) = members(texels, PARTITIONS[partition], subset);
                spread(&points[..count], channels)
            })
            .sum();
        (score, partition)
    });
    scores.sort_by(|a, b| a.0.total_cmp(&b.0));
    std::array::from_fn(|rank| scores[rank].1)
}

/// The squared distance of `texels` from their principal line, which no endpoint pair follows.
fn spread(texels: &[Texel], channels: &Range<usize>) -> f32 {
    let mean = mean(texels, channels);
    let matrix = covariance(texels, channels, mean);
    let axis = axis(&matrix, channels);
    let total: f32 = channels
        .clone()
        .map(|channel| matrix[channel][channel])
        .sum();
    let along: f32 = (0..4)
        .map(|i| axis[i] * (0..4).map(|j| matrix[i][j] * axis[j]).sum::<f32>())
        .sum();
    (total - along).max(0.)
}

/// A quantized endpoint pair, each fitted texel's index and their squared error.
#[derive(Clone, Copy)]
struct Fit {
    /// Stored endpoint codes without p-bits, by endpoint and channel.
    codes: [[u32; 4]; 2],
    pbits: [u32; 2],
    indices: [u8; 16],
    error: u32,
}

/// The closest quantized endpoints for `texels` over `channels`, indexed with `weights`.
fn fit(texels: &[Texel], channels: &Range<usize>, precision: Precision, weights: &[u32]) -> Fit {
    let mut best = quantized(
        texels,
        channels,
        precision,
        weights,
        principal(texels, channels),
    );
    for _ in 0..REFINEMENTS {
        let Some(ends) = refined(texels, channels, weights, &best.indices) else {
            break;
        };
        let next = quantized(texels, channels, precision, weights, ends);
        if next.error >= best.error {
            break;
        }
        best = next;
    }
    best
}

/// `ends` quantized under each p-bit choice `precision` allows, keeping the closest.
fn quantized(
    texels: &[Texel],
    channels: &Range<usize>,
    precision: Precision,
    weights: &[u32],
    ends: [[f32; 4]; 2],
) -> Fit {
    let choices: &[[u32; 2]] = match precision.pbits {
        Pbits::Absent => &[[0, 0]],
        Pbits::PerSubset => &[[0, 0], [1, 1]],
        Pbits::PerEndpoint => &[[0, 0], [0, 1], [1, 0], [1, 1]],
    };
    let first = with_pbits(texels, channels, precision, weights, ends, choices[0]);
    choices[1..]
        .iter()
        .map(|&pbits| with_pbits(texels, channels, precision, weights, ends, pbits))
        .fold(
            first,
            |best, fit| if fit.error < best.error { fit } else { best },
        )
}

fn with_pbits(
    texels: &[Texel],
    channels: &Range<usize>,
    precision: Precision,
    weights: &[u32],
    ends: [[f32; 4]; 2],
    pbits: [u32; 2],
) -> Fit {
    let mut codes = [[0; 4]; 2];
    let mut decoded = [[255; 4]; 2];
    for (end, (target, pbit)) in ends.iter().zip(pbits).enumerate() {
        let pbit = (precision.pbits != Pbits::Absent).then_some(pbit);
        for channel in channels.clone() {
            (codes[end][channel], decoded[end][channel]) =
                quantize(target[channel], precision.bits, pbit);
        }
    }
    let mut indices = [0; 16];
    let error = assign(texels, channels, decoded, weights, &mut indices);
    Fit {
        codes,
        pbits,
        indices,
        error,
    }
}

/// The stored code nearest `target` and its decoded value, for `bits` per channel and an optional
/// p-bit below them.
fn quantize(target: f32, bits: u32, pbit: Option<u32>) -> (u32, u8) {
    let total = bits + u32::from(pbit.is_some());
    let top = (1 << bits) - 1;
    let scaled = target.clamp(0., 255.) * ((1u32 << total) - 1) as f32 / 255.;
    let ideal = pbit.map_or(scaled, |pbit| (scaled - pbit as f32) / 2.);
    let low = (ideal.max(0.) as u32).min(top);
    let high = (low + 1).min(top);
    let decode = |code: u32| expand(pbit.map_or(code, |pbit| (code << 1) | pbit), total);
    let (below, above) = (decode(low), decode(high));
    if (f32::from(below) - target).abs() <= (f32::from(above) - target).abs() {
        (low, below)
    } else {
        (high, above)
    }
}

/// A `bits`-bit endpoint value, 6 to 8 bits, widened to eight by repeating its high bits.
fn expand(value: u32, bits: u32) -> u8 {
    ((value << (8 - bits)) | (value >> (2 * bits - 8))) as u8
}

/// The value the decoder interpolates `weight` sixty-fourths of the way from `low` to `high`.
fn mix(low: u8, high: u8, weight: u32) -> u8 {
    (((64 - weight) * u32::from(low) + weight * u32::from(high) + 32) >> 6) as u8
}

/// Writes each texel's nearest interpolated value between `ends` over `channels` to `indices` and
/// returns the summed squared error.
fn assign(
    texels: &[Texel],
    channels: &Range<usize>,
    ends: [Texel; 2],
    weights: &[u32],
    indices: &mut [u8; 16],
) -> u32 {
    let palette: [Texel; 16] = std::array::from_fn(|entry| {
        let weight = weights.get(entry).copied().unwrap_or(0);
        std::array::from_fn(|channel| mix(ends[0][channel], ends[1][channel], weight))
    });
    let palette = &palette[..weights.len()];
    let mut total = 0;
    for (texel, index) in texels.iter().zip(indices.iter_mut()) {
        let (error, nearest) = palette
            .iter()
            .enumerate()
            .map(|(entry, value)| (distance(texel, value, channels), entry))
            .min()
            .unwrap_or((0, 0));
        *index = nearest as u8;
        total += error;
    }
    total
}

fn distance(a: &Texel, b: &Texel, channels: &Range<usize>) -> u32 {
    channels
        .clone()
        .map(|channel| {
            (i32::from(a[channel]) - i32::from(b[channel]))
                .unsigned_abs()
                .pow(2)
        })
        .sum()
}

/// Swaps a fit's endpoints, and mirrors its first `count` indices, when the index at `position`
/// has its top bit set, which the anchor texel's shortened index cannot store. The weights are
/// symmetric, so the decoded texels stay the same.
fn anchor(fit: &mut Fit, position: usize, index_bits: u32, count: usize) {
    let top = (1u8 << index_bits) - 1;
    if fit.indices[position] > top / 2 {
        fit.codes.swap(0, 1);
        fit.pbits.swap(0, 1);
        for index in &mut fit.indices[..count] {
            *index = top - *index;
        }
    }
}

/// The ends of `texels`' spread along their principal axis over `channels`.
fn principal(texels: &[Texel], channels: &Range<usize>) -> [[f32; 4]; 2] {
    let mean = mean(texels, channels);
    let axis = axis(&covariance(texels, channels, mean), channels);
    let (mut low, mut high) = (0f32, 0f32);
    for texel in texels {
        let along: f32 = channels
            .clone()
            .map(|channel| (f32::from(texel[channel]) - mean[channel]) * axis[channel])
            .sum();
        low = low.min(along);
        high = high.max(along);
    }
    let end = |along: f32| std::array::from_fn(|channel| mean[channel] + axis[channel] * along);
    [end(low), end(high)]
}

fn mean(texels: &[Texel], channels: &Range<usize>) -> [f32; 4] {
    let mut sum = [0f32; 4];
    for texel in texels {
        for channel in channels.clone() {
            sum[channel] += f32::from(texel[channel]);
        }
    }
    sum.map(|total| total / texels.len().max(1) as f32)
}

/// The summed outer products of `texels` about `mean` over `channels`, zero elsewhere.
fn covariance(texels: &[Texel], channels: &Range<usize>, mean: [f32; 4]) -> [[f32; 4]; 4] {
    let mut matrix = [[0f32; 4]; 4];
    for texel in texels {
        let offset: [f32; 4] =
            std::array::from_fn(|channel| f32::from(texel[channel]) - mean[channel]);
        for i in channels.clone() {
            for j in channels.clone() {
                matrix[i][j] += offset[i] * offset[j];
            }
        }
    }
    matrix
}

/// The covariance's principal eigenvector, by power iteration from its widest row, or zero when
/// every texel is the same.
fn axis(matrix: &[[f32; 4]; 4], channels: &Range<usize>) -> [f32; 4] {
    let widest = channels
        .clone()
        .max_by(|&a, &b| matrix[a][a].total_cmp(&matrix[b][b]))
        .unwrap_or(0);
    let mut axis = matrix[widest];
    for _ in 0..8 {
        let length = axis.iter().map(|value| value * value).sum::<f32>().sqrt();
        if length < 1e-6 {
            return [0.; 4];
        }
        let unit = axis.map(|value| value / length);
        axis = std::array::from_fn(|i| (0..4).map(|j| matrix[i][j] * unit[j]).sum());
    }
    let length = axis.iter().map(|value| value * value).sum::<f32>().sqrt();
    if length < 1e-6 {
        [0.; 4]
    } else {
        axis.map(|value| value / length)
    }
}

/// Endpoints that least-squares fit `texels` at the weights `indices` select, or `None` when every
/// texel has the same weight.
fn refined(
    texels: &[Texel],
    channels: &Range<usize>,
    weights: &[u32],
    indices: &[u8; 16],
) -> Option<[[f32; 4]; 2]> {
    let (mut aa, mut ab, mut bb) = (0f32, 0f32, 0f32);
    let (mut low, mut high) = ([0f32; 4], [0f32; 4]);
    for (texel, &index) in texels.iter().zip(indices) {
        let b = weights[usize::from(index)] as f32 / 64.;
        let a = 1. - b;
        aa += a * a;
        ab += a * b;
        bb += b * b;
        for channel in channels.clone() {
            low[channel] += a * f32::from(texel[channel]);
            high[channel] += b * f32::from(texel[channel]);
        }
    }
    let determinant = aa * bb - ab * ab;
    if determinant.abs() < 1e-6 {
        return None;
    }
    Some([
        std::array::from_fn(|channel| (bb * low[channel] - ab * high[channel]) / determinant),
        std::array::from_fn(|channel| (aa * high[channel] - ab * low[channel]) / determinant),
    ])
}

/// A block's 128 bits, written from the lowest.
#[derive(Default)]
struct Bits {
    value: u128,
    at: u32,
}

impl Bits {
    fn put(&mut self, value: u32, count: u32) {
        debug_assert!(count <= 8 && value < (1 << count) && self.at + count <= 128);
        self.value |= u128::from(value) << self.at;
        self.at += count;
    }

    fn bytes(self) -> [u8; 16] {
        debug_assert_eq!(self.at, 128);
        self.value.to_le_bytes()
    }
}

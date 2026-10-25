//! MCC U13 CPU waveforms on the native particle interpreter.
//!
//! The static byte tables match the original haloreach.dll and its mapped image.
//! CPU consumer 000A7238 supplies phase conversion and interpolation. The GPU
//! analytical expressions are separate. See Almanac's Reach source contracts.
use super::Expr;
use crate::tiger::particle::Program;
use anyhow::{Result, ensure};

pub(super) const TABLES: &[u8; 12 * 1024] = include_bytes!("periodic/tables.bin");

fn trunc(value: Expr) -> Expr {
    let normal = Expr::select(
        value.clone().binary(0x0a, Expr::scalar(0.)),
        value.clone().unary(0x18),
        value.clone().unary(0x17),
    );
    // MCC uses cvttss2si, including its signed minimum on integer overflow.
    // Masked multiplication avoids cancellation when selecting that sentinel.
    let valid = Expr::scalar(-2147483648.)
        .binary(0x0a, value.clone())
        .mul(Expr::scalar(1.).binary(2, Expr::scalar(2147483648.).binary(0x0a, value)));
    normal
        .mul(valid.clone())
        .add(Expr::scalar(-2147483648.).mul(Expr::scalar(1.).binary(2, valid)))
}

struct Writer<'a, F> {
    program: &'a mut Program,
    phase: usize,
    input: &'a F,
    bank: u8,
    first: u8,
}

impl<F> Writer<'_, F>
where
    F: Fn(&mut Program, usize, u8) -> Result<()>,
{
    fn emit(&mut self, value: &Expr) -> Result<()> {
        value.emit(self.program, self.phase, self.input)
    }

    fn register(&self, scalar: u8) -> Expr {
        Expr::Register(self.bank, self.first + scalar / 4, 4 + scalar % 4)
    }

    fn store_stack(&mut self, scalar: u8) -> Result<Expr> {
        self.program.code(
            self.phase,
            &[0x3f, self.bank, self.first + scalar / 4, 4 + scalar % 4],
        )?;
        Ok(self.register(scalar))
    }

    fn store(&mut self, scalar: u8, value: Expr) -> Result<Expr> {
        self.emit(&value)?;
        self.store_stack(scalar)
    }

    fn constant(&mut self, value: [f32; 4]) -> Result<()> {
        self.program.constant(self.phase, value)
    }

    fn sample(&mut self, wave: u8, index: Expr, destination: u8) -> Result<Expr> {
        let start = usize::from(wave) * 1024;
        let table = &TABLES[start..start + 1024];
        // Compact only exact byte repetition. Analytic periodicity is not enough
        // because quantized source tables can differ at nominally equal phases.
        let period = (0..=10)
            .map(|power| 1usize << power)
            .find(|&period| {
                table
                    .chunks_exact(period)
                    .all(|part| part == &table[..period])
            })
            .unwrap_or(table.len());
        let index = if period < table.len() {
            index.clone().binary(
                2,
                index
                    .mul(Expr::scalar(1. / period as f32))
                    .unary(0x17)
                    .mul(Expr::scalar(period as f32)),
            )
        } else {
            index
        };
        let table = &table[..period];
        // Three bytes fit exactly in a Float32 integer. Eight packed words per
        // cubic span reduce constant use without depending on dynamic indexing.
        let words = table
            .chunks(3)
            .map(|bytes| {
                bytes.iter().enumerate().fold(0u32, |value, (lane, byte)| {
                    value | (u32::from(*byte) << (lane * 8))
                }) as f32
            })
            .collect::<Vec<_>>();
        let chunk = index.clone().mul(Expr::scalar(1. / 3.)).unary(0x17);
        let cursor = self.store(4, chunk.clone())?;
        self.constant([0.; 4])?;
        for start in (0..words.len()).step_by(8) {
            for group in 0..2 {
                // A, B and C are zero. D selects one packed integer per segment.
                for _ in 0..3 {
                    self.constant([0.; 4])?;
                }
                self.constant(std::array::from_fn(|lane| {
                    words[(start + group * 4 + lane).min(words.len() - 1)]
                }))?;
            }
            self.constant([0., 1., 2., 3.])?;
            self.constant([4., 5., 6., 7.])?;
            self.emit(&cursor)?;
            self.program.code(self.phase, &[0x31])?;
            if start + 8 < words.len() {
                // Fixed knots share constants across every table and property.
                // The earlier span remains on the stack as the fallback.
                self.store(4, cursor.clone().binary(2, Expr::scalar(8.)))?;
            }
        }
        let lane = index.binary(2, chunk.mul(Expr::scalar(3.)));
        let gain = Expr::select(
            lane.clone().binary(0x0a, Expr::scalar(0.)),
            Expr::scalar(1. / 256.),
            Expr::select(
                lane.binary(0x0a, Expr::scalar(1.)),
                Expr::scalar(1. / 65536.),
                Expr::scalar(1. / 16777216.),
            ),
        );
        self.emit(&gain)?;
        self.program.code(self.phase, &[3, 0x1a])?;
        self.constant([256.; 4])?;
        self.program.code(self.phase, &[3, 0x17])?;
        self.constant([1. / 255.; 4])?;
        self.program.code(self.phase, &[3])?;
        self.store_stack(destination)
    }
}

pub(super) fn emit(
    wave: u8,
    time: &Expr,
    program: &mut Program,
    phase: usize,
    input: &impl Fn(&mut Program, usize, u8) -> Result<()>,
) -> Result<()> {
    ensure!(wave < 12, "Unsupported source periodic waveform {wave}");
    if wave < 2 {
        return program.constant(phase, [f32::from(1 - wave); 4]);
    }
    let (bank, first) = if matches!(phase, 0 | 1 | 7) {
        // Native callers bind sixteen fixed emitter rows. This translator owns
        // rows zero through two, leaving rows three and four for this expression.
        (4, 3)
    } else {
        ensure!(
            matches!(phase, 2 | 3 | 4 | 6),
            "CPU waveform phase has no qualified temporary storage"
        );
        // Existing particle state occupies rows zero through seven. Private
        // temporaries avoid writing shared emitter values from particle phases.
        // CPU spawn and frame callers alias the state read/write pointers.
        // Ten rows also fit the spawn caller's sixteen-row stack region.
        program.require_state_rows(10)?;
        (0, 8)
    };
    let mut writer = Writer {
        program,
        phase,
        input,
        bank,
        first,
    };
    let scaled = writer.store(0, time.clone().mul(Expr::scalar(36.57143)))?;
    let fraction = writer.store(1, scaled.clone().binary(2, trunc(scaled.clone())))?;
    let whole = writer.store(0, scaled.binary(2, fraction.clone()))?;
    let rounded = trunc(whole.clone().add(Expr::select(
        Expr::scalar(0.).binary(0x0a, whole),
        Expr::scalar(0.5),
        Expr::scalar(-0.5),
    )));
    let index = writer.store(
        0,
        rounded.clone().binary(
            2,
            rounded
                .mul(Expr::scalar(1. / 1024.))
                .unary(0x17)
                .mul(Expr::scalar(1024.)),
        ),
    )?;
    let a = writer.sample(wave, index.clone(), 2)?;
    let next = writer.store(
        0,
        index
            .add(Expr::scalar(1.))
            .mul(Expr::scalar(1. / 1024.))
            .unary(0x1a)
            .mul(Expr::scalar(1024.)),
    )?;
    let mut b = writer.sample(wave, next, 3)?;
    if matches!(wave, 6 | 7) {
        let wrap = Expr::scalar(1.)
            .binary(2, a.clone().binary(0x0a, Expr::scalar(0.75)))
            .mul(Expr::scalar(1.).binary(2, Expr::scalar(0.25).binary(0x0a, b.clone())));
        b = writer.store(3, b.add(wrap))?;
    }
    let mut result = Expr::scalar(1.)
        .binary(2, fraction.clone())
        .mul(a)
        .add(fraction.mul(b));
    if matches!(wave, 6 | 7) {
        result = writer.store(0, result)?;
        result = result.clone().binary(
            2,
            Expr::scalar(1.).binary(2, result.binary(0x0a, Expr::scalar(1.))),
        );
    }
    writer.emit(&result)
}

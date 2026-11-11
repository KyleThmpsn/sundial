//! Scalar callback parameters and their bank-owned history around checked channel procedures.
use super::*;

pub(super) struct Callback {
    object: Vec<u8>,
    class: u32,
    pub(super) spring: bool,
    pub(super) program: program::Program,
}

pub(super) fn read(source: &Payload, row: usize, channels: usize) -> Result<Option<Callback>> {
    if source.u64(row + 96)? == 0 {
        return Ok(None);
    }
    let program = program::read(source, row, channels)?;
    ensure!(
        !program.inputs.is_empty(),
        "Source callback has no channel input"
    );
    let object = source.pointer(row + 96)?;
    // Rise of Iron dispatcher 003A7960 and Shadowkeep dispatcher 00599140 number the same seven
    // kinds and read the same parameter offsets with the same constants. Source sizes come from
    // the class records. Shadowkeep's kind 1 record is four bytes longer than its single rate, and
    // the unused tail stays zero.
    let kind = source.u32(object)?;
    let (source_class, source_size, class, size) = match kind {
        1 => (0x80800593, 12, 0x808097AF, 16),
        2 => (0x8080072D, 16, 0x808097AE, 16),
        3 => (0x80800857, 24, 0x808097AD, 24),
        4 => (0x80800572, 32, 0x808097AC, 32),
        5 => (0x808004EE, 16, 0x808097AB, 16),
        6 => (0x80800905, 16, 0x808097AA, 16),
        7 => (0x8080033D, 24, 0x808097A9, 24),
        _ => anyhow::bail!("Source channel callback kind {kind} requires translation"),
    };
    ensure!(
        object >= 4 && source.u32(object - 4)? == source_class,
        "Source callback class differs"
    );
    let state = source.u32(object + 4)?;
    // Kinds 3 and 4 clear opposing velocity when the byte at +08 is set.
    let first_float = if matches!(kind, 3 | 4) {
        ensure!(
            source.u32(object + 8)? <= 1,
            "Source callback Boolean differs"
        );
        12
    } else {
        8
    };
    let states = source.array(source.pointer(16)? + 0x70, 16, Some(0x80800090))?;
    ensure!(
        (state == u32::MAX || (state as usize) < states.len())
            && (first_float..source_size)
                .step_by(4)
                .all(|at| source.f32(object + at).is_ok_and(f32::is_finite)),
        "Source callback parameters or history index differ"
    );
    let mut bytes = source
        .0
        .get(object..object + source_size)
        .context("Source callback extent")?
        .to_vec();
    bytes.resize(size, 0);
    if source_class == 0x808004EE {
        let stiffness = source.f32(object + 8)?;
        let damping = source.f32(object + 12)?;
        ensure!(
            stiffness > 0.0 && damping >= 0.0,
            "Source spring needs positive stiffness and nonnegative damping"
        );
        // D1 FAAEE0 integrates acceleration k * (target - value) - c * velocity.
        // Shadowkeep 59938E supplies ratio and frequency to its analytic spring solver.
        // These preserve the physical coefficients, not D1's bounded integration steps.
        let omega = stiffness.sqrt();
        let ratio = damping / (2.0 * omega);
        let frequency = omega / std::f32::consts::TAU;
        ensure!(
            ratio.is_finite() && frequency.is_finite() && frequency > 0.0,
            "Source spring parameters exceed native bounds"
        );
        bytes[8..12].copy_from_slice(&ratio.to_le_bytes());
        bytes[12..16].copy_from_slice(&frequency.to_le_bytes());
    }
    Ok(Some(Callback {
        object: bytes,
        class,
        spring: source_class == 0x808004EE,
        program,
    }))
}

impl Callback {
    pub(super) fn write(&self, p: &mut Payload, row: usize) -> Result<()> {
        self.program.write(p, row)?;
        // The callback object follows inline, eight-byte aligned behind its class word.
        let object = (p.0.len() + 4).next_multiple_of(8);
        p.0.resize(object - 4, 0);
        p.0.extend(self.class.to_le_bytes());
        p.0.extend_from_slice(&self.object);
        put(
            &mut p.0,
            row + 96,
            &(object as i64 - (row + 96) as i64).to_le_bytes(),
        )?;
        Ok(())
    }
}

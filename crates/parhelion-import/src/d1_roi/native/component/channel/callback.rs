//! Kind-2 callback objects around checked four-lane channel procedures.
use super::*;

const SOURCE_CLASS: u32 = 0x8080_072D;
const NATIVE_CLASS: u32 = 0x8080_97AE;

pub(super) struct Callback {
    object: [u8; 16],
    pub(super) program: program::Program,
}

pub(super) fn read(source: &Payload, row: usize, channels: usize) -> Result<Option<Callback>> {
    if source.u64(row + 96)? == 0 {
        return Ok(None);
    }
    let program = program::read(source, row, channels)?;
    ensure!(
        !program.inputs.is_empty(),
        "Source kind-2 callback has no channel input"
    );
    let object = source.pointer(row + 96)?;
    ensure!(
        object >= 4 && source.u32(object - 4)? == SOURCE_CLASS,
        "Source channel callback kind requires translation"
    );
    ensure!(
        source.u32(object)? == 2
            && source.u32(object + 4)? == u32::MAX
            && [8, 12]
                .iter()
                .all(|at| source.f32(object + at).is_ok_and(f32::is_finite)),
        "Source kind-2 callback layout differs"
    );
    Ok(Some(Callback {
        object: source.bytes::<16>(object)?,
        program,
    }))
}

impl Callback {
    pub(super) fn write(&self, p: &mut Payload, row: usize) -> Result<()> {
        self.program.write(p, row)?;
        // The callback object follows inline, eight-byte aligned behind its class word.
        let object = (p.0.len() + 4).next_multiple_of(8);
        p.0.resize(object - 4, 0);
        p.0.extend(NATIVE_CLASS.to_le_bytes());
        p.0.extend(self.object);
        put(
            &mut p.0,
            row + 96,
            &(object as i64 - (row + 96) as i64).to_le_bytes(),
        )?;
        Ok(())
    }
}

//! Source screen color events name a grading record that Shadowkeep ports field for field.
//!
//! Rise of Iron event `8080052F`, kind 20, names a `80801B7B` record at `+24`. The lens talents'
//! records hold mode 1000 at `+08`, bytes `03 05` at `+0C`, the shared textures `80A6A379` at
//! `+2C` and `+30`, shared program `80AAD358` at `+3C` and a parameter program of twelve initial
//! vectors and no bytecode at `+40`. Shadowkeep has seven `8080707E` records of that shape. They
//! hold the shared textures `80C70F20`, shared program `80C70F1E` (the same bytecode with
//! Shadowkeep's global index and store opcode) and a parameter program of thirteen vectors: the
//! source's first eleven, a twelfth `(1, 1, 0, 0)` the source lacks, and the source's last.
//! Stock record `80C0E2D4` with program `80C0E2D3` holds exactly Grayscale's eleven vectors, and
//! stock sequence `80EF2635` plays it from an event of the source event's own name `44823B33`.
//! A record may name a lookup texture at `+28`. Source and stock lookups are both `R8G8B8A8`
//! surfaces of 1024 by 32 texels, so a source lookup becomes a private texture of the same texels.
use super::*;
use crate::d1_roi::{Source, dependency};

const SOURCE_TEXTURE: u32 = 0x80A6_A379;
const SOURCE_PROGRAM: u32 = 0x80AA_D358;
const RECORD: u32 = 0x80C0_E2D4;
const PARAMETERS: u32 = 0x80C0_E2D3;

/// The omitted-set entry of a caller that translates lens records: only screen records of
/// another form keep their timing as delays.
pub const OTHER_RECORDS: (u32, u32) = (0x8080052F, 20 | 0x8000_0000);

/// The entry type of a texture header, which stands in for a class where a typed patch names a
/// private lookup texture. Texture headers have no class.
pub const LOOKUP: u32 = 32;

/// The stock record and parameter program a translated record starts from.
#[derive(Clone)]
pub struct Twin {
    record: Payload,
    parameters: Payload,
}

impl Twin {
    pub fn read(native: &mut crate::tiger::reader::Reader) -> Result<Self> {
        let record = (*native.tag(RECORD, Some(0x8080707E))?).clone();
        let parameters = (*native.tag(PARAMETERS, Some(0x808071DC))?).clone();
        ensure!(
            record.0.len() == 0x50
                && record.u64(0)? == 0x50
                && record.u32(8)? == 1000
                && record.bytes::<2>(0xC)? == [3, 5]
                && record.u32(0x28)? == u32::MAX
                && record.u32(0x40)? == PARAMETERS
                && record.bytes::<8>(0x48)? == [0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0],
            "Native screen color record differs"
        );
        ensure!(
            parameters.u64(0x30)? == 0
                && parameters.u64(0x40)? == 0
                && parameters.u32(0x80)? == 0
                && parameters.array(0x60, 16, Some(0x80800090))?.len() == 13,
            "Native screen color parameters differ"
        );
        Ok(Self { record, parameters })
    }
}

pub struct Screen {
    source_offset: usize,
    resource: u32,
    /// The source lookup texture at `+28`.
    lookup: Option<u32>,
    /// The source parameter program's twelve initial vectors.
    vectors: Vec<[u8; 16]>,
    /// Source record words `+08` to `+27` and `+44`, which the stock record must also hold.
    head: [u8; 0x20],
    tail: [u8; 4],
}

impl Screen {
    pub fn resource(&self) -> u32 {
        self.resource
    }

    pub fn lookup(&self) -> Option<u32> {
        self.lookup
    }

    /// Whether the event names a record of the lens form. Other records keep the caller's
    /// omission, since their modes and textures have no established translation.
    pub fn qualifies(source: &Source, owner: &Payload, at: usize) -> Result<bool> {
        let resource = owner.u32(at + 0x24)?;
        if resource == u32::MAX || source.reference(resource)? != 0x80801B7B {
            return Ok(false);
        }
        let data = source.read(resource)?;
        Ok(data.0.len() == 0x48
            && data.u32(8)? == 1000
            && data.bytes::<2>(0xC)? == [3, 5]
            && data.u32(0x2C)? == SOURCE_TEXTURE
            && data.u32(0x30)? == SOURCE_TEXTURE
            && data.u32(0x3C)? == SOURCE_PROGRAM)
    }

    pub fn read(source: &Source, owner: &Payload, at: usize) -> Result<Self> {
        ensure!(
            owner.u32(at.checked_sub(4).context("Source screen event marker")?)? == 0x8080052F
                && owner.u32(at + 0x1C)? == 20
                && owner.u32(at + 0x20)? == 0
                && owner.u64(at + 0x28)? == 0,
            "Source screen event class, kind or padding differs"
        );
        ensure!(
            Self::qualifies(source, owner, at)?,
            "Source screen record is not of the lens form"
        );
        let resource = owner.u32(at + 0x24)?;
        let data = source.read(resource)?;
        ensure!(
            data.u64(0)? == 0x48 && data.u32(0x34)? == u32::MAX && data.u32(0x38)? == u32::MAX,
            "Source screen record has material handles"
        );
        let lookup = Some(data.u32(0x28)?).filter(|tag| *tag != u32::MAX);
        if let Some(tag) = lookup {
            let header = source.read(tag)?;
            ensure!(
                header.0.len() == 0x44
                    && header.u32(0)? == 0x1C
                    && header.bytes::<8>(0x30)? == [0, 4, 0x20, 0, 1, 0, 1, 0],
                "Source screen lookup texture is not a 1024 by 32 R8G8B8A8 surface"
            );
        }
        let program = data.u32(0x40)?;
        ensure!(
            source.reference(program)? == 0x80801AF2,
            "Source screen parameter class differs"
        );
        let parameters = source.read(program)?;
        ensure!(
            parameters.u64(0x30)? == 0 && parameters.u64(0x40)? == 0 && parameters.u32(0x80)? == 0,
            "Source screen parameters have bytecode, constants or inputs"
        );
        let vectors = parameters
            .array(0x60, 16, Some(0x80800090))?
            .into_iter()
            .map(|row| parameters.bytes::<16>(row))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            vectors.len() == 12
                && vectors.iter().all(|vector| {
                    vector.chunks_exact(4).all(|lane| {
                        f32::from_le_bytes(lane.try_into().expect("Float32 lane")).is_finite()
                    })
                }),
            "Source screen parameters need twelve finite vectors"
        );
        Ok(Self {
            source_offset: at,
            resource,
            lookup,
            vectors,
            head: data.bytes(8)?,
            tail: data.bytes(0x44)?,
        })
    }

    pub fn mapping(&self) -> (usize, NativeEvent) {
        (
            self.source_offset,
            NativeEvent {
                source_class: 0x8080052F,
                source_kind: 20,
                class: 0x808093C2,
            },
        )
    }

    /// The private record and its parameter program. The record keeps the stock shared textures
    /// and first program, and the program takes the source vectors in the stock positions.
    pub fn translate(
        &self,
        symbol: &str,
        twin: &Twin,
        lookup: Option<&str>,
    ) -> Result<Vec<dependency::Asset>> {
        dependency::symbol(symbol)?;
        ensure!(
            lookup.is_some() == self.lookup.is_some(),
            "Screen lookup binding differs from the source record"
        );
        ensure!(
            twin.record.bytes::<0x20>(8)? == self.head
                && twin.record.bytes::<4>(0x44)? == self.tail,
            "Source screen record fields differ from the stock record"
        );
        let parameters = format!("{symbol}-parameters");
        let mut program = twin.parameters.clone();
        let rows = program.array(0x60, 16, Some(0x80800090))?;
        for (index, vector) in self.vectors.iter().enumerate() {
            // The stock program inserts one vector before the source's last.
            let row = rows[if index == 11 { 12 } else { index }];
            put(&mut program.0, row, vector)?;
        }
        let mut record = twin.record.clone();
        put(&mut record.0, 0x40, &u32::MAX.to_le_bytes())?;
        Ok(vec![
            dependency::Asset {
                symbol: symbol.into(),
                class: 0x8080707E,
                payload: record,
                // The stock record's lookup slot at `+28` is already unresolved.
                patches: lookup
                    .map(|texture| dependency::Patch {
                        offset: 0x28,
                        symbol: texture.into(),
                        class: LOOKUP,
                    })
                    .into_iter()
                    .chain([dependency::Patch {
                        offset: 0x40,
                        symbol: parameters.clone(),
                        class: 0x808071DC,
                    }])
                    .collect(),
            },
            dependency::Asset {
                symbol: parameters,
                class: 0x808071DC,
                payload: program,
                patches: Vec::new(),
            },
        ])
    }

    /// The event body stock sequence `80EF2635` gives its events: locator zero, as the source
    /// event holds at `+20`, and three constant programs of one. The event class also plays
    /// decal records, which name a locator there.
    pub fn write(&self, target: &mut Payload, event: usize, symbol: &str) -> Result<ResourcePatch> {
        dependency::symbol(symbol)?;
        target.bytes::<0x118>(event)?;
        let instance = usize::try_from(target.u64(event + 8)?)?;
        target.bytes::<0x50>(instance)?;
        ensure!(
            target.u32(event.checked_sub(4).context("Native screen event marker")?)? == 0x808093C2
                && target.u32(event + 0x30)? == 23
                && target.u32(event + 4)? == 0x808093C1
                && target.u32(
                    instance
                        .checked_sub(4)
                        .context("Native screen runtime marker")?
                )? == 0x808093C1
                && target.u32(instance + 4)? == 0x808093C2
                && target.u32(instance)? == target.u32(event)?
                && target.u64(instance + 8)? == event as u64
                && instance + 0x50 <= event,
            "Native screen event pair or kind differs"
        );
        let mut output = target.clone();
        // The common scheduler owns timing and parent state. The stock instance tail is empty
        // but for its unresolved handle.
        put(&mut output.0, instance + 0x18, &[0; 0x28])?;
        put(
            &mut output.0,
            instance + 0x40,
            &[0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0],
        )?;
        put(&mut output.0, instance + 0x48, &[0; 8])?;
        put(&mut output.0, event + 0x40, &[0; 4])?;
        put(&mut output.0, event + 0x44, &u32::MAX.to_le_bytes())?;
        put(&mut output.0, event + 0x48, &[0; 0xD0])?;
        let one = program::Program::constant(1.0, 1);
        for at in [0x48, 0x90, 0xD8] {
            program::Program::write(Some(&one), &mut output, event + at, false)?;
        }
        *target = output;
        Ok(ResourcePatch {
            offset: event + 0x44,
            symbol: symbol.into(),
            source: self.resource,
            source_class: 0x80801B7B,
            native_class: 0x8080707E,
        })
    }
}

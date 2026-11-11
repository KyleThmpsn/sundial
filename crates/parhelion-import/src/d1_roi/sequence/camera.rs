//! Source camera configuration, rails and transition settings on the native override event.
//!
//! The native configuration adds three nullable fields. Existing source settings shift by
//! eight bytes after priority and flags. The final field is a locator ordinal, read by source
//! `70CC0A` and native `DBB502` rather than a camera flag mask. Source `3B7130` and native
//! `5892D0` resolve it through the locator rows at definition `+1B0` and native `+1A0` and the
//! owner points at instance `+150`. With no locator the camera follows the owning entity.
use super::{program::Program, *};
use crate::{
    d1_roi::{Source, dependency},
    presentation::append,
};

pub struct Camera {
    offset: usize,
    body: Vec<u8>,
    programs: Vec<Option<Program>>,
    rails: Vec<([u8; 8], u32)>,
    assets: BTreeMap<u32, dependency::Asset>,
    transition: Transition,
    locator: Option<u8>,
}

/// Source `808024FB` and native `808066C4` share one `2A8`-byte transition layout: leading values
/// at `+00`, nine pairs of a nullable pointer and a bound one-input program from `+18` with stride
/// `48`, and two trailing values at `+2A0`. Only the source lists each program's builtin input.
struct Transition {
    head: [u8; 0x18],
    programs: Vec<Option<Program>>,
    tail: [u8; 8],
}

const TRANSITION_SIZE: usize = 0x2A8;
const TRANSITION_PAIRS: usize = 9;

impl Transition {
    fn read(owner: &Payload, at: usize) -> Result<Self> {
        ensure!(
            owner.u32(
                at.checked_sub(4)
                    .context("Source camera transition marker")?
            )? == 0x808024FB,
            "Source camera transition class differs"
        );
        owner.bytes::<TRANSITION_SIZE>(at)?;
        let head = owner.bytes::<0x18>(at)?;
        let tail = owner.bytes::<8>(at + 0x2A0)?;
        ensure!(
            head[..12]
                .chunks_exact(4)
                .chain(tail.chunks_exact(4))
                .all(|b| f32::from_le_bytes(b.try_into().unwrap()).is_finite())
                && head[12..].iter().all(|b| *b == 0),
            "Source camera transition values differ"
        );
        let mut programs = Vec::new();
        for pair in (0..TRANSITION_PAIRS).map(|k| at + 0x18 + k * 0x48) {
            ensure!(
                owner.u64(pair)? == 0,
                "Source camera transition pointer needs translation"
            );
            programs.push(Program::read(owner, pair + 8, true, 1)?);
        }
        Ok(Self {
            head,
            programs,
            tail,
        })
    }

    fn write(&self, target: &mut Payload) -> Result<usize> {
        let at = native::object(target, 0x808066C4, TRANSITION_SIZE)?;
        put(&mut target.0, at, &self.head)?;
        for (k, program) in self.programs.iter().enumerate() {
            Program::write(program.as_ref(), target, at + 0x20 + k * 0x48, true)?;
        }
        put(&mut target.0, at + 0x2A0, &self.tail)?;
        Ok(at)
    }
}

fn symbol(tag: u32) -> String {
    format!("camera-rail-{tag:08X}")
}

impl Camera {
    pub fn read(source: &Source, owner: &Payload, at: usize) -> Result<Self> {
        ensure!(
            owner.u32(at.checked_sub(4).context("Source camera marker")?)? == 0x80802045
                && owner.u32(at + 0x1C)? == 18,
            "Source camera class or kind differs"
        );
        let body = owner
            .0
            .get(at + 0x20..at + 0x248)
            .context("Source camera extent")?
            .to_vec();
        for offset in [0x178, 0x180, 0x188, 0x190, 0x198, 0x1A0, 0x230] {
            ensure!(
                owner.u64(at + offset)? == 0,
                "Source camera extension at {offset:X} needs translation"
            );
        }
        ensure!(
            owner.0[at + 0x242..at + 0x248].iter().all(|&b| b == 0),
            "Source camera locator padding differs"
        );
        let locator = match owner.u16(at + 0x240)? {
            u16::MAX => None,
            value => {
                ensure!(
                    usize::from(value)
                        < owner
                            .array(owner.pointer(24)? + 0x1B0, 5, Some(0x80800429))?
                            .len(),
                    "Source camera locator is undeclared"
                );
                Some(u8::try_from(value).context("Source camera locator ordinal")?)
            }
        };
        let programs = [0x48, 0x90, 0x110, 0x1B0, 0x1F0]
            .into_iter()
            .map(|offset| Program::read(owner, at + offset, true, 1))
            .collect::<Result<Vec<_>>>()?;
        let transition = Transition::read(owner, owner.pointer(at + 0x238)?)?;
        let mut rails = Vec::new();
        let mut assets = BTreeMap::new();
        for row in owner.array(at + 0xF0, 16, Some(0x8080261A))? {
            let tag = owner.u32(row + 8)?;
            ensure!(
                owner.u32(row + 12)? == 0 && source.reference(tag)? == 0x8080288F,
                "Source camera rail reference differs"
            );
            let rail = source.read(tag)?;
            ensure!(
                rail.u64(0)? == rail.0.len() as u64,
                "Source camera rail extent differs"
            );
            let points = rail.array(8, 32, Some(0x8080229B))?;
            ensure!(!points.is_empty(), "Source camera rail has no points");
            let points = points
                .into_iter()
                .map(|p| rail.bytes::<32>(p))
                .collect::<Result<Vec<_>>>()?
                .concat();
            ensure!(
                points
                    .chunks_exact(4)
                    .all(|b| f32::from_le_bytes(b.try_into().unwrap()).is_finite()),
                "Source camera rail is nonfinite"
            );
            let mut payload = Payload(vec![0; 24]);
            append(&mut payload.0, 8, 0x808066FD, &points, 32)?;
            let length = payload.0.len() as u64;
            put(&mut payload.0, 0, &length.to_le_bytes())?;
            assets.entry(tag).or_insert(dependency::Asset {
                symbol: symbol(tag),
                class: 0x808066FB,
                payload,
                patches: Vec::new(),
            });
            rails.push((owner.bytes::<8>(row)?, tag));
        }
        Ok(Self {
            offset: at,
            body,
            programs,
            rails,
            assets,
            transition,
            locator,
        })
    }

    pub fn assets(&self) -> impl Iterator<Item = &dependency::Asset> {
        self.assets.values()
    }

    /// The source locator ordinal the camera follows, if any.
    pub fn locator(&self) -> Option<u8> {
        self.locator
    }

    pub fn mapping(&self) -> (usize, NativeEvent) {
        (
            self.offset,
            NativeEvent {
                source_class: 0x80802045,
                source_kind: 18,
                class: 0x808066A3,
            },
        )
    }

    pub fn write(
        &self,
        target: &mut Payload,
        at: usize,
        locators: &BTreeMap<u8, u8>,
    ) -> Result<Vec<ResourcePatch>> {
        ensure!(
            target.u32(at - 4)? == 0x808066A3 && target.u32(at + 0x30)? == 21,
            "Native camera event class or kind differs"
        );
        let instance = usize::try_from(target.u64(at + 8)?)?;
        ensure!(
            target.u32(instance - 4)? == 0x808066A2 && target.u64(instance + 8)? == at as u64,
            "Native camera event pair differs"
        );
        target.bytes::<0x70>(instance)?;
        put(&mut target.0, at + 0x38, &self.body[..8])?;
        put(&mut target.0, at + 0x40, &[0; 8])?;
        put(&mut target.0, at + 0x48, &self.body[8..0x220])?;
        put(&mut target.0, at + 0x260, &[0; 24])?;
        for (offset, program) in [0x68, 0xB0, 0x130, 0x1D0, 0x210]
            .into_iter()
            .zip(&self.programs)
        {
            Program::write(program.as_ref(), target, at + offset, true)?;
        }
        let mut rail_rows = Vec::new();
        for (selection, _) in &self.rails {
            rail_rows.extend_from_slice(selection);
            rail_rows.extend_from_slice(&u32::MAX.to_le_bytes());
            rail_rows.extend_from_slice(&[0; 4]);
        }
        append(&mut target.0, at + 0x110, 0x808066FA, &rail_rows, 16)?;
        let rows = target.array(at + 0x110, 16, Some(0x808066FA))?;
        let patches = rows
            .into_iter()
            .zip(&self.rails)
            .map(|(row, (_, tag))| ResourcePatch {
                offset: row + 8,
                symbol: symbol(*tag),
                source: *tag,
                source_class: 0x8080288F,
                native_class: 0x808066FB,
            })
            .collect();
        let transition = self.transition.write(target)?;
        native::relative(target, at + 0x258, transition)?;
        let locator = match self.locator {
            None => u16::MAX,
            Some(locator) => u16::from(
                *locators
                    .get(&locator)
                    .context("Native camera locator mapping is absent")?,
            ),
        };
        put(&mut target.0, at + 0x270, &locator.to_le_bytes())?;
        put(&mut target.0, instance + 0x18, &[0; 0x58])?;
        for offset in [0x40, 0x58] {
            put(&mut target.0, instance + offset, &u64::MAX.to_le_bytes())?;
        }
        for offset in [0x50, 0x68] {
            put(&mut target.0, instance + offset, &u32::MAX.to_le_bytes())?;
        }
        Ok(patches)
    }
}

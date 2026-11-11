//! Modern evade profiles lowered into the native movement profile representation.
//!
//! The profile's local meter is separate from common ability energy. Source class and
//! investment slot ordinals do not select a native ability bucket. The containing ability
//! compiler must supply that binding, private actions, the effect graph and preparation.
pub mod native;
mod preparation;

pub use preparation::{Motion, replace_motion};

use crate::tiger::payload::Payload;
use anyhow::{Context, Result, ensure};
use serde::Serialize;

const SOURCE_INSTANCE: u32 = 0x8080340E;
const SOURCE_DEFINITION: u32 = 0x8080360B;
const CONFIGURATION: usize = 0x688;
const PROFILE_CLASS: u32 = 0x80803413;
const PROFILE_SIZE: usize = 0xE8;

/// A required adaptation when the source updates its direction during the evade.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Preserve,
    FixedAtActivation,
}

#[derive(Clone, Debug, Serialize)]
pub struct Profile {
    pub offset: usize,
    pub instance: usize,
    pub identity: u32,
    pub cost: f32,
    pub recovery_delay: f32,
    pub recovery_rate: f32,
    pub readiness: [f32; 2],
    pub eligibility: u32,
    pub input_buffer: f32,
    pub actions: [u32; 4],
    pub effect: [u8; 16],
    pub input: u32,
    pub request_flag: bool,
    pub current_direction: bool,
    pub preparation: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct Source {
    pub owner: u32,
    pub definition: usize,
    pub instance: usize,
    pub profiles: Vec<Profile>,
}

fn pair(p: &Payload, definition: usize, instance: usize, classes: [u32; 2]) -> Result<()> {
    let owner = p.u32(p.pointer(16)?)?;
    ensure!(
        p.u32(definition)? == owner
            && p.u32(instance)? == owner
            && p.u32(definition + 4)? == classes[0]
            && p.u32(instance + 4)? == classes[1]
            && p.u64(definition + 8)? == instance as u64
            && p.u64(instance + 8)? == definition as u64,
        "Evade records are not a reciprocal owner pair"
    );
    Ok(())
}

fn finite(p: &Payload, at: usize) -> Result<f32> {
    let value = p.f32(at)?;
    ensure!(value.is_finite(), "Evade value at {at:X} is not finite");
    Ok(value)
}

fn recovery(p: &Payload, definition: usize, profile: usize) -> Result<f32> {
    let instance = usize::try_from(p.u64(definition + 8)?)?;
    pair(p, definition, instance, [0x8080862F, 0x80808630])?;
    ensure!(
        p.pointer(instance + 16)? == profile
            && p.u64(instance + 24)? == 0
            && p.u64(definition + 48)? == 1
            && p.u64(definition + 56)? == 1
            && p.u64(definition + 64)? == 0
            && p.bytes::<16>(definition + 80)?
                == [0, 0, 0, 0, 0, 0, 0, 0, 0xC5, 0x9D, 0x1C, 0x81, 0, 0, 0, 0],
        "Evade local recovery has inputs or references requiring another native producer"
    );
    let code = p
        .array(definition + 16, 1, Some(0x80800009))?
        .into_iter()
        .map(|at| p.u8(at))
        .collect::<Result<Vec<_>>>()?;
    let constants = p.array(definition + 32, 16, Some(0x80800090))?;
    // The source program returns constant zero's first lane. Other programs require an
    // evaluated native input contract rather than treating the unused literal as its rate.
    ensure!(
        code == [0x42, 0, 0x4C, 0] && constants.len() == 1,
        "Evade local recovery is not a constant native rate"
    );
    let value = finite(p, constants[0])?;
    ensure!(
        (1..4).all(|lane| p.u32(constants[0] + lane * 4).ok() == Some(value.to_bits()))
            && value >= 0.,
        "Evade local recovery constant is not a nonnegative scalar"
    );
    Ok(value)
}

/// Read the actual controller configuration and every paired profile, not investment labels.
pub fn read(p: &Payload) -> Result<Source> {
    let instance = p.pointer(16)?;
    let definition = p.pointer(24)?;
    ensure!(
        p.u64(0)? == p.0.len() as u64
            && p.u32(instance.checked_sub(4).context("Evade runtime marker")?)? == SOURCE_INSTANCE
            && p.u32(
                definition
                    .checked_sub(4)
                    .context("Evade definition marker")?
            )? == SOURCE_DEFINITION,
        "Source is not a modern evade controller"
    );
    pair(
        p,
        definition,
        instance,
        [SOURCE_INSTANCE, SOURCE_DEFINITION],
    )?;
    let configuration = definition + CONFIGURATION;
    let configuration_instance = usize::try_from(p.u64(configuration + 8)?)?;
    pair(
        p,
        configuration,
        configuration_instance,
        [0x8080341A, 0x8080341C],
    )?;
    let definitions = p.array(configuration + 16, PROFILE_SIZE, Some(PROFILE_CLASS))?;
    let instances = p.array(configuration_instance + 32, 0x90, Some(0x80803412))?;
    ensure!(
        !definitions.is_empty() && definitions.len() <= 32 && definitions.len() == instances.len(),
        "Source evade profile arrays are empty or disagree"
    );
    let mut profiles = Vec::new();
    for (offset, runtime) in definitions.into_iter().zip(instances) {
        pair(p, offset, runtime, [0x80803412, PROFILE_CLASS])?;
        let condition = offset + 0x80;
        let condition_instance = runtime + 0x30;
        pair(p, condition, condition_instance, [0x80802E48, 0x80802E49])?;
        ensure!(
            p.u64(condition + 16)? == 0
                && p.u64(condition + 24)? == 0
                && p.u64(condition_instance + 32)? == 0
                && p.u64(condition_instance + 40)? == 0
                && p.u64(offset + 0xA8)? == 0,
            "Evade profile has conditions requiring native translation"
        );
        let cost = finite(p, offset + 0x14)?;
        let recovery_delay = finite(p, offset + 0x18)?;
        let readiness = [finite(p, offset + 0xA0)?, finite(p, offset + 0xA4)?];
        let eligibility = p.u32(offset + 0xC0)?;
        let input_buffer = finite(p, offset + 0xC4)?;
        ensure!(
            cost >= 0.
                && recovery_delay >= 0.
                && p.u32(offset + 0x1C)? == 0
                && readiness[0] <= readiness[1]
                && eligibility != 0
                && eligibility & !3 == 0
                && input_buffer >= 0.
                && p.u8(offset + 0xDC)? <= 1
                && p.u8(offset + 0xDD)? <= 1
                && p.u16(offset + 0xDE)? == 0,
            "Evade profile has unsupported numeric values or request flags"
        );
        let mut actions = [0; 4];
        for (index, action) in actions.iter_mut().enumerate() {
            *action = p.u32(offset + 0xC8 + index * 4)?;
        }
        profiles.push(Profile {
            offset,
            instance: runtime,
            identity: p.u32(offset + 16)?,
            cost,
            recovery_delay,
            recovery_rate: recovery(p, offset + 32, runtime)?,
            readiness,
            eligibility,
            input_buffer,
            actions,
            effect: p.bytes(offset + 0xB0)?,
            input: p.u32(offset + 0xD8)?,
            request_flag: p.u8(offset + 0xDC)? != 0,
            current_direction: p.u8(offset + 0xDD)? != 0,
            preparation: (p.u64(offset + 0xE0)? != 0)
                .then(|| p.pointer(offset + 0xE0))
                .transpose()?,
        });
    }
    Ok(Source {
        owner: p.u32(instance)?,
        definition,
        instance,
        profiles,
    })
}

/// Fill only the profile's translated values. The native author supplies all paired references,
/// copied instance storage, provider registration, effects and named human actions. The native
/// absent-reference sentinel is valid when the containing author explicitly omits presentation.
pub fn profile(
    source: &Profile,
    native: &mut [u8; 0x80],
    effect: u32,
    actions: [u32; 4],
    direction: Direction,
) -> Result<()> {
    ensure!(
        !source.current_direction || matches!(direction, Direction::FixedAtActivation),
        "Source evade updates its direction during movement. The native evade needs an explicit direction adaptation"
    );
    ensure!(
        effect != 0
            && actions
                .iter()
                .all(|key| !matches!(*key, 0 | 0x811C9DC5 | u32::MAX)),
        "Evade profile needs its translated effect and four private actions"
    );
    let mut write = |at: usize, value: u32| {
        native[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    for (at, value) in [
        (0x10, source.identity),
        (0x14, source.cost.to_bits()),
        (0x18, source.recovery_delay.to_bits()),
        (0x1C, source.recovery_rate.to_bits()),
        (0x40, source.readiness[0].to_bits()),
        (0x44, source.readiness[1].to_bits()),
        (0x50, effect),
        (0x54, 0),
        (0x58, source.eligibility),
        (0x5C, source.input_buffer.to_bits()),
        (0x70, source.input),
        (0x74, u32::from(source.request_flag)),
    ] {
        write(at, value);
    }
    for (index, action) in actions.into_iter().enumerate() {
        write(0x60 + index * 4, action);
    }
    Ok(())
}

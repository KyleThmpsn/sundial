//! D1 cue events preserve numeric parameter names and explicitly translated attachment locators.
use super::*;
use crate::{d1_roi::Source, presentation::append};

pub struct Sound {
    source_offset: usize,
    cue: u32,
    locator: u8,
    /// Source sequence input ordinal and its parameter name, in playback order.
    parameters: Vec<(u32, u32)>,
}

impl Sound {
    pub fn cue(&self) -> u32 {
        self.cue
    }

    pub fn locator(&self) -> u8 {
        self.locator
    }

    pub fn parameters(&self) -> &[(u32, u32)] {
        &self.parameters
    }

    pub fn read(source: &Source, owner: &Payload, at: usize) -> Result<Self> {
        ensure!(
            owner.u32(at.checked_sub(4).context("Source sound event marker")?)? == 0x80801F00
                && owner.u32(at + 0x1C)? == 2
                && owner.u32(at + 0x24)? == 0
                && owner.u64(at + 0x38)? == 0
                && owner.u64(at + 0x40)? == 0
                && owner.bytes::<7>(at + 0x49)? == [0; 7],
            "Source sound event has unsupported switch or extension fields"
        );
        let cue = owner.u32(at + 0x20)?;
        ensure!(
            source.reference(cue)? == 0x8080080A,
            "Source sound event does not name a cue"
        );
        let definition = owner.pointer(24)?;
        let inputs = owner.array(definition + 0x1A0, 48, Some(0x80800927))?;
        let locator = owner.u8(at + 0x48)?;
        ensure!(
            (locator as usize) < owner.array(definition + 0x1B0, 5, Some(0x80800429))?.len(),
            "Source sound locator is undeclared"
        );
        let rows = owner.array(at + 0x28, 4, Some(0x80800007))?;
        // Source 7E9437 clamps creation to three parameters. Keep only forms that fit
        // both engines, rather than silently adding native-only fourth through sixth values.
        ensure!(
            rows.len() <= 3,
            "Source sound parameter capacity requires additional translation"
        );
        let parameters = rows
            .into_iter()
            .map(|row| {
                let index = owner.u32(row)?;
                let input = *inputs
                    .get(index as usize)
                    .context("Source sound input is undeclared")?;
                Ok((index, owner.u32(input + 0x28)?))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            source_offset: at,
            cue,
            locator,
            parameters,
        })
    }

    pub fn mapping(&self) -> (usize, NativeEvent) {
        (
            self.source_offset,
            NativeEvent {
                source_class: 0x80801F00,
                source_kind: 2,
                class: 0x80806B38,
            },
        )
    }

    /// The symbol must name a completed source cue from the audio graph converter. Returned
    /// patches remain obligations of graph publication, never permission to retain a stock cue.
    pub fn write(
        &self,
        target: &mut Payload,
        event: usize,
        cue_symbol: &str,
        inputs: &BTreeMap<u32, u32>,
        locators: &BTreeMap<u8, u8>,
    ) -> Result<ResourcePatch> {
        ensure!(
            !cue_symbol.trim().is_empty(),
            "Translated sound cue symbol is empty"
        );
        ensure!(
            target.u32(event.checked_sub(4).context("Native sound event marker")?)? == 0x80806B38
                && target.u32(event + 0x30)? == 6,
            "Native sound event class or kind differs"
        );
        let definition = target.pointer(24)?;
        let declarations = target.array(definition + 0x188, 40, Some(0x80809789))?;
        let mut parameters = Vec::new();
        for &(source, name) in &self.parameters {
            let index = *inputs
                .get(&source)
                .context("Sound input has no native mapping")?;
            let input = *declarations
                .get(index as usize)
                .context("Native sound input is undeclared")?;
            ensure!(
                target.u32(input + 0x20)? == name,
                "Sound parameter name changed during translation"
            );
            parameters.extend_from_slice(&index.to_le_bytes());
        }
        let locator = *locators
            .get(&self.locator)
            .context("Sound locator has no native mapping")?;
        ensure!(
            (locator as usize) < target.array(definition + 0x1B0, 6, Some(0x80808617))?.len(),
            "Native sound locator is undeclared"
        );
        let instance = usize::try_from(target.u64(event + 8)?)?;
        ensure!(
            target.u32(
                instance
                    .checked_sub(4)
                    .context("Native sound instance marker")?
            )? == 0x80806B37
                && target.u32(instance + 4)? == 0x80806B38
                && target.u32(event + 4)? == 0x80806B37
                && target.u32(instance)? == target.u32(event)?
                && target.u64(instance + 8)? == event as u64,
            "Native sound event pair differs"
        );
        let mut output = target.clone();
        put(&mut output.0, event + 0x38, &[0; 0x40])?;
        put(&mut output.0, event + 0x38, &0xFFu32.to_le_bytes())?;
        put(&mut output.0, event + 0x40, &u32::MAX.to_le_bytes())?;
        append(&mut output.0, event + 0x48, 0x80800007, &parameters, 4)?;
        put(&mut output.0, event + 0x70, &[locator])?;
        // Original native creation resets this playback handle before submitting the cue.
        // A checked event envelope must also be safe before its first creation callback.
        put(&mut output.0, instance + 0x40, &u32::MAX.to_le_bytes())?;
        *target = output;
        Ok(ResourcePatch {
            offset: event + 0x40,
            symbol: cue_symbol.into(),
            source: self.cue,
            source_class: 0x8080080A,
            native_class: 0x80809802,
        })
    }
}

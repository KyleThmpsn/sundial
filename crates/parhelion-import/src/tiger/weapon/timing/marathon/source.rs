//! Admit each complete source stat scope before planning private native storage.
use super::*;

const EMPTY: u32 = 0x811C9DC5;

pub(super) fn curves(
    p: &Payload,
    table: usize,
    calibration: &Calibration,
) -> Result<BTreeMap<Input, Curve>> {
    let definition = p.pointer(24)?;
    ensure!(
        p.array(definition + 0x50, 48, Some(0x80803F8A))? == [table]
            && p.u64(definition + 0x60)? == 0
            && p.u64(definition + 0x248)? == 0,
        "Source timing requires one table without shared logic or stat overrides"
    );
    let keys = p.array(definition + 0x258, 40, Some(0x80803EA7))?;
    ensure!(
        keys.len() == 1 && p.u64(keys[0])? == 0,
        "Source timing group has stat overrides"
    );
    let translations = p.array(table, 56, Some(0x80803F8E))?;
    let routes = p.array(table + 16, 96, Some(0x80803F96))?;
    for row in p.array(table + 32, 40, Some(0x80803F91))? {
        for output in p.array(row, 16, Some(0x80803F9C))? {
            ensure!(
                !(calibration.scope.accepts)((
                    p.u32(output)?,
                    p.u32(output + 4)?,
                    p.u32(output + 8)?
                )),
                "Source timing has another runtime writer"
            );
        }
    }
    calibration
        .scope
        .required
        .iter()
        .map(|&(component, input, selector)| {
            let selected = routes
                .iter()
                .copied()
                .filter(|&r| p.u32(r).ok() == Some(component) && p.u32(r + 4).ok() == Some(input))
                .collect::<Vec<_>>();
            let [row] = selected.as_slice() else {
                anyhow::bail!("Source timing input {component}.{input} has no unique route");
            };
            ensure!(
                p.u32(row + 8)? == selector
                    && p.u32(row + 12)? == EMPTY
                    && p.u32(row + 16)? == 4
                    && p.bytes::<12>(row + 20)? == [0; 12]
                    && p.u32(row + 32)? == EMPTY
                    && p.u32(row + 40)? == 0x7F7FFFFF,
                "Source timing input {component}.{input} is not a direct unnamed curve copy"
            );
            let channel = p
                .u32(row + 36)?
                .checked_sub(0x3C00)
                .context("Source timing channel")? as usize;
            let curve = *translations
                .get(channel / 32)
                .context("Source timing curve extent")?;
            let native_input = p
                .u32(curve + 32)?
                .checked_sub(1)
                .context("Source stat input")?;
            ensure!(
                calibration.inputs.contains(&native_input)
                    && p.u32(curve + 36)? == EMPTY
                    && p.u32(curve + 40)? == 0
                    && p.u32(curve + 52)? == 1,
                "Source timing curve requires the corresponding interpolated stat domain"
            );
            let low = p.f32(curve + 44)?;
            let high = p.f32(curve + 48)?;
            ensure!(high > low, "Source timing curve domain is empty");
            for recipient in p.array(curve, 16, None)? {
                ensure!(
                    p.u64(recipient)? == 0,
                    "Source timing curve has direct recipients"
                );
            }
            let samples = p
                .array(curve + 16, 16, Some(0x8080BC4D))?
                .into_iter()
                .map(|tier| {
                    let cells = p.array(tier, 4, Some(CELLS))?;
                    let cell = *cells.get(channel % 32).context("Source timing column")?;
                    // Handling includes movement penalties as well as durations. Zero is valid for a
                    // penalty or instantaneous interpolation, while a reload requires positive time.
                    let value = p.f32(cell)?;
                    ensure!(
                        (0.0..=30.).contains(&value)
                            && (calibration.name != "reload" || value >= 0.1),
                        "Source timing value is outside the checked range"
                    );
                    p.u32(cell)
                })
                .collect::<Result<Vec<_>>>()?;
            ensure!(
                !samples.is_empty() && samples.len() <= 256,
                "Source timing curve has invalid tiers"
            );
            let mut metadata = [0u8; 24];
            metadata[..4].copy_from_slice(&native_input.to_le_bytes());
            metadata[8..12].copy_from_slice(&low.to_le_bytes());
            metadata[12..16].copy_from_slice(&high.to_le_bytes());
            metadata[16] = 1;
            Ok((
                (component, input, selector),
                Curve {
                    identity: (curve, channel % 32),
                    metadata,
                    samples,
                },
            ))
        })
        .collect()
}

//! Light events naming a light record at one locator, with a stock event's parameters and neutral
//! embedded expressions.
use super::*;

/// The light event class, kind 33, and its instance class.
const EVENT: u32 = 0x8080_6f49;
/// The three embedded expressions, 0x48 bytes apart, ending at the definition's 0x150 bytes.
const EXPRESSIONS: [usize; 3] = [0x78, 0xc0, 0x108];
/// The literal-one program every inspected stock light and particle event stores in each slot.
const LITERAL_ONE: [u8; 4] = [0x34, 0, 0x3e, 0];

/// Fills light event `event`, a definition [`write`](super::write) placed in flow `payload`,
/// from the light event of stock flow `template` that names light record `record`. The record
/// field at `+0x40` is cleared for the caller's private record, `+0x44` takes locator ordinal
/// `locator`, and the stock parameters from `+0x48` to `+0x78` are copied. Stock events store
/// their locator there, 6 in the supercombine finish `80EF27F9` and 2 in the Fallen bolt flow
/// `80FF0231`, each a kind-0 point. The three embedded expressions take the literal-one program
/// with one constant `(1, 1, 1, 1)`, which the stock event is checked to hold too. Returns the
/// record field.
pub(crate) fn write(
    payload: &mut Payload,
    event: usize,
    template: &Payload,
    record: u32,
    locator: u8,
) -> Result<usize> {
    ensure!(
        payload.u32(event - 4)? == EVENT && payload.u32(event + 0x30)? == 33,
        "Light event class differs"
    );
    let root = payload.pointer(24)?;
    ensure!(
        usize::from(locator) < payload.array(root + 0x1b0, 6, Some(0x8080_8617))?.len(),
        "Light event locator is undeclared"
    );
    let definition = template.pointer(24)?;
    let mut found = Vec::new();
    for row in template.array(definition + 0x168, 24, Some(0x8080_93e6))? {
        let at = template.pointer(row + 16)?;
        if template.u32(at - 4)? == EVENT && template.u32(at + 0x40)? == record {
            found.push(at);
        }
    }
    let [stock] = found[..] else {
        anyhow::bail!("Stock flow names light record {record:08X} in no light event or several")
    };
    let one = [1f32; 4]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    ensure!(
        template.u32(stock + 0x30)? == 33 && template.u32(stock + 0x38)? == 0xff,
        "Stock light event differs"
    );
    for slot in EXPRESSIONS {
        let code = template
            .array(stock + slot, 1, Some(0x8080_0009))?
            .into_iter()
            .map(|at| template.u8(at))
            .collect::<Result<Vec<_>>>()?;
        let values = template.array(stock + slot + 16, 16, Some(0x8080_0090))?;
        ensure!(
            code == LITERAL_ONE
                && values.len() == 1
                && template.bytes::<16>(values[0])?[..] == one[..]
                && template.u64(stock + slot + 32)? == 1
                && template.u64(stock + slot + 40)? == 1,
            "Stock light event expression differs"
        );
    }
    let parameters = template.bytes::<0x30>(stock + 0x48)?;
    put(&mut payload.0, event + 0x40, &u32::MAX.to_le_bytes())?;
    put(
        &mut payload.0,
        event + 0x44,
        &u32::from(locator).to_le_bytes(),
    )?;
    put(&mut payload.0, event + 0x48, &parameters)?;
    for slot in EXPRESSIONS {
        append(&mut payload.0, event + slot, 0x8080_0009, &LITERAL_ONE, 1)?;
        append(&mut payload.0, event + slot + 16, 0x8080_0090, &one, 16)?;
        put(&mut payload.0, event + slot + 32, &1u64.to_le_bytes())?;
        put(&mut payload.0, event + slot + 40, &1u64.to_le_bytes())?;
    }
    Ok(event + 0x40)
}

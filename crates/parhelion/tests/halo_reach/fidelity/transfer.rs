//! Follow compiled transfer destinations to independently decoded source pools.
use super::*;

pub(super) fn verify(
    health: &Payload,
    native: &[usize],
    section: usize,
    source: &Value,
    sections: &[Value],
) -> Result<Value> {
    let mut expected = BTreeMap::new();
    for row in source["Section Damage Transfers@0x90"]
        .as_array()
        .context("Independent section damage transfers")?
    {
        let row = &row["fields"];
        let index = row["Damage Section Index@0xA"]
            .as_i64()
            .context("Source transfer destination")?;
        if index < 0 {
            continue;
        }
        let target = sections
            .get(usize::try_from(index)?)
            .and_then(|s| s["fields"]["Name@0x0"].as_str())
            .context("Source transfer destination pool")?;
        expected
            .entry(hash(target))
            .or_insert_with(Vec::new)
            .push(number(&row["Transfer Amount@0x4"])? * 100.);
    }
    let mut readback = Vec::new();
    for condition in health.array(section + 0x58, 32, Some(0x80804c08))? {
        for pointer in health.array(condition + 0x10, 8, Some(0x80804ae0))? {
            let at = health.pointer(pointer)?;
            ensure!(
                health.u32(at - 4)? == 0x80804acd && health.u8(at)? == 2,
                "Unexpected section transfer behavior"
            );
            let target = health.u8(at + 15)? as i8;
            let destination = *native
                .get(usize::try_from(target).context("Negative compiled transfer destination")?)
                .context("Compiled transfer destination is absent")?;
            let name = health.u32(destination + 16)?;
            let amount = health.f32(at + 8)?;
            let wanted = expected
                .get_mut(&name)
                .and_then(Vec::pop)
                .context("Runtime damage transfer selects the wrong source pool")?;
            close(amount, wanted)?;
            readback.push(json!({"destination":format!("{name:08X}"),
                "section_index":target,"percentage":amount}));
        }
    }
    ensure!(
        expected.values().all(Vec::is_empty),
        "Source damage transfer is absent from the emitted health component"
    );
    Ok(json!(readback))
}

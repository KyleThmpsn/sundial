//! Compile source pool destinations without recursive damage-transfer cycles.
use super::*;

pub(super) fn destination(row: &Value) -> Result<Option<usize>> {
    // Nonlocal routes can carry a stale section index. Keep it in the source
    // receipt, but never turn it into a native local transfer or cycle edge.
    if flags(row)? & 1 == 0 {
        return Ok(None);
    }
    let target = row["section"].as_i64().context("Source transfer section")?;
    ensure!(target >= -1, "Invalid source transfer section {target}");
    if target == -1 {
        Ok(None)
    } else {
        Ok(Some(usize::try_from(target)?))
    }
}

pub(super) fn validate(sections: &[Value]) -> Result<()> {
    let mut edges = vec![Vec::new(); sections.len()];
    let mut incoming = vec![0usize; sections.len()];
    for (index, section) in sections.iter().enumerate() {
        for row in section["damage_transfers"]
            .as_array()
            .context("Source section damage transfers")?
        {
            let Some(target) = destination(row)? else {
                continue;
            };
            i8::try_from(target)
                .context("Damage transfer destination exceeds the native signed index")?;
            let count = incoming
                .get_mut(target)
                .context("Source transfer section exceeds table")?;
            *count += 1;
            edges[index].push(target);
        }
    }
    let mut ready = incoming
        .iter()
        .enumerate()
        .filter_map(|(index, &count)| (count == 0).then_some(index))
        .collect::<Vec<_>>();
    let mut visited = 0;
    while let Some(index) = ready.pop() {
        visited += 1;
        for &target in &edges[index] {
            incoming[target] -= 1;
            if incoming[target] == 0 {
                ready.push(target);
            }
        }
    }
    ensure!(
        visited == sections.len(),
        "Source damage transfers form a cycle that native damage handling cannot execute"
    );
    Ok(())
}

pub(super) fn build(
    payload: &mut Payload,
    section: usize,
    source: &Value,
    names: &[u32],
) -> Result<Value> {
    let mut report = Vec::new();
    let mut targets = Vec::new();
    for row in source["damage_transfers"]
        .as_array()
        .context("Source section damage transfers")?
    {
        let Some(target) = destination(row)? else {
            report.push(json!({"source":row,"status":"Transfer without a local destination requires an explicit native route"}));
            continue;
        };
        let kind = row["function"]
            .as_i64()
            .context("Source transfer function")?;
        ensure!(
            kind == 0 && flags(row)? == 0x71 && row["seat"] == "",
            "Source section transfer needs a different native contract"
        );
        let name = *names
            .get(target)
            .context("Source transfer section exceeds table")?;
        let amount = scalar(row, "amount")?;
        let mut bytes = [0; 16];
        put(&mut bytes, 0, &2u32.to_le_bytes())?;
        put(&mut bytes, 4, &name.to_le_bytes())?;
        put(&mut bytes, 8, &(amount * 100.).to_le_bytes())?;
        // B899A2 sign-extends +0F into the destination passed to B851F0.
        // The name at +04 does not replace this compiled section index.
        bytes[15] = i8::try_from(target)
            .context("Damage transfer destination exceeds the native signed index")?
            as u8;
        targets.push(dynamic(payload, 0x80804acd, &bytes)?);
        report.push(json!({"source":row,"target":target,"percentage":amount*100.,"native_class":"80804ACD"}));
    }
    if !targets.is_empty() {
        let rows = array(payload, section + 0x58, 0x80804c08, 0x20, 1)?;
        pointers(payload, rows[0] + 0x10, 0x80804ae0, &targets)?;
    }
    Ok(json!(report))
}

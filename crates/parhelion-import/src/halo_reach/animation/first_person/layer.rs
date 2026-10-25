//! Keep donor action corrections out of complete translated poses.
use super::*;

// This named first-family layer holds the carrier's action-specific corrections.
// Its idle and reload choices are additive over the carrier's original clips.
const ACTION_CORRECTION: u32 = 0xFC23AA50;

pub(super) fn source_actions(
    native: &mut Reader,
    table: &Payload,
    bank: &Payload,
    first: usize,
) -> Result<(Payload, Value)> {
    let aliases = table.array(40, 2, Some(0x80800006))?;
    ensure!(
        first <= aliases.len(),
        "Translated action dispatch is outside its table"
    );
    if first == aliases.len() {
        return Ok((table.clone(), json!({"removed_routes":0})));
    }
    let layers = table.array(8, 24, Some(0x80803724))?;
    let selected = layers
        .iter()
        .enumerate()
        .filter_map(|(index, &at)| {
            (table.u32(at + 16).ok() == Some(ACTION_CORRECTION)).then_some((index, at))
        })
        .collect::<Vec<_>>();
    ensure!(
        selected.len() <= 1,
        "Repeated native action correction layer"
    );
    let Some(&(layer, at)) = selected.first() else {
        return Ok((table.clone(), json!({"removed_routes":0})));
    };
    let clips = bank.array(8, 4, Some(0x80808F48))?;
    for choice in table.array(at, 16, Some(0x8080372E))? {
        for option in table.array(choice, 64, Some(0x80803730))? {
            for weight in table.array(option, 12, Some(0x80803737))? {
                let slot = table.u32(weight)? as usize;
                let tag = bank.u32(*clips.get(slot).context("Action correction clip missing")?)?;
                let clip = native.tag(tag, Some(0x80808F49))?;
                ensure!(
                    clip.u8(0x181)? == 1,
                    "Action correction requires additive clips"
                );
            }
        }
    }
    let mut routes = table
        .array(56, 32, Some(0x808034EF))?
        .into_iter()
        .map(|at| {
            Ok([
                rows(table, at, 4, 0x808034F1)?,
                rows(table, at + 16, 4, 0x808034F1)?,
            ])
        })
        .collect::<Result<Vec<_>>>()?;
    let mut out = table.clone();
    let mut replacements = BTreeMap::new();
    let mut removed = 0;
    for &alias in &aliases[first..] {
        let original = table.u16(alias)?;
        if original == u16::MAX {
            continue;
        }
        let replacement = if let Some(&index) = replacements.get(&original) {
            index
        } else {
            let mut route = routes
                .get(usize::from(original))
                .context("Action correction dispatch is outside its routes")?
                .clone();
            let before = route[0].len();
            route[0] = route[0]
                .chunks_exact(4)
                .filter(|edge| u16::from_le_bytes([edge[2], edge[3]]) as usize != layer)
                .flatten()
                .copied()
                .collect();
            let index = if route[0].len() == before {
                original
            } else {
                removed += 1;
                let index = u16::try_from(routes.len())?;
                ensure!(
                    index <= i16::MAX as u16,
                    "Translated pose routes exceed native limits"
                );
                routes.push(route);
                index
            };
            replacements.insert(original, index);
            index
        };
        crate::presentation::put(&mut out.0, alias, &replacement.to_le_bytes())?;
    }
    append(&mut out.0, 56, 0x808034EF, &vec![0; routes.len() * 32], 32)?;
    for (at, route) in out.array(56, 32, Some(0x808034EF))?.into_iter().zip(routes) {
        for (family, edges) in route.into_iter().enumerate() {
            append(&mut out.0, at + family * 16, 0x808034F1, &edges, 4)?;
        }
    }
    Ok((
        out,
        json!({"layer":format!("{ACTION_CORRECTION:08X}"),"removed_routes":removed,
        "method":"Complete source actions omit the donor action correction layer. Other pose contributions and original action dispatch remain intact."}),
    ))
}

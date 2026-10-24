//! Literal bases for native automatic-weapon magazine calculations.
use super::*;
use std::collections::BTreeSet;

type Writes = Vec<(usize, Vec<u8>)>;
type Inputs = BTreeSet<(u32, u32)>;

pub(super) fn patches(
    owner: &[u8],
    table: usize,
    values: &[Input],
) -> AuthoringResult<(Inputs, Writes)> {
    let (count, _, rows, class) = array_at(owner, table + 32)?;
    if count != 0 && class != 0x8080397C {
        return Err(invalid("Imported stat runtime logic layout differs"));
    }
    let mut handled = BTreeSet::new();
    let mut writes = Vec::new();
    for row in (0..count).map(|i| rows + i * 40) {
        let (count, _, outputs, class) = array_at(owner, row)?;
        if count == 0 {
            continue;
        }
        if class != 0x80803987 {
            return Err(invalid("Imported stat logic outputs have another layout"));
        }
        let targets = (0..count)
            .map(|i| {
                let at = outputs + i * 12;
                Ok((
                    read_u32(owner, at)?,
                    read_u32(owner, at + 4)?,
                    read_u32(owner, at + 8)?,
                ))
            })
            .collect::<AuthoringResult<Vec<_>>>()?;
        if !targets.iter().any(|&(component, input, _)| {
            values
                .iter()
                .any(|v| v.component == component && v.input == input)
        }) {
            continue;
        }
        let target_keys: Inputs = targets
            .iter()
            .map(|&(component, input, _)| (component, input))
            .collect();
        if read_u32(owner, row + 32)? != 2
            || target_keys != BTreeSet::from([(1, 0), (1, 3)])
            || targets.len() != 2
            || targets.iter().any(|t| t.2 != u32::MAX)
        {
            return Err(invalid(
                "Native runtime logic overrides an imported input with unsupported behavior",
            ));
        }
        let capacity = values
            .iter()
            .find(|v| (v.component, v.input) == (1, 0))
            .ok_or_else(|| {
                invalid("Native magazine logic needs both capacity and refill inputs")
            })?;
        if !values
            .iter()
            .any(|v| (v.component, v.input) == (1, 3) && v.value == capacity.value)
        {
            return Err(invalid(
                "Native magazine logic needs equal capacity and refill bases",
            ));
        }
        let (count, _, operands, class) = array_at(owner, row + 16)?;
        if count != 4 || class != 0x80803986 {
            return Err(invalid("Native magazine logic operands differ"));
        }
        // Native type 2 computes round(base * rate / reference) * bullets. Whole source
        // capacities with unit rates and bullets give that base without coupling it to a
        // donor's cadence curve. Preserve the row, its two destinations and shared ordering.
        for (index, value) in [1., 1., capacity.value, 1.].into_iter().enumerate() {
            writes.push((operands + index * 20, literal(value)));
        }
        handled.extend(target_keys);
    }
    Ok((handled, writes))
}

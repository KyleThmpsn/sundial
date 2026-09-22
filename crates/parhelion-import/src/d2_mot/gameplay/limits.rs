//! Native stat-group bounds used by the package compiler.
use super::*;

#[derive(Default)]
pub(super) struct Limits {
    maximum: Option<i32>,
    minimum: BTreeMap<u16, i32>,
}

pub(super) fn read(
    r: &mut Reader,
    globals: &Payload,
    donor: u32,
    recipe: &Value,
) -> Result<Limits> {
    let index = if let Some(index) = recipe["overrides"]["stat_group_index"].as_u64() {
        usize::try_from(index)?
    } else {
        let strings = r.tag(globals.u32(16 + 33 * 16)?, None)?;
        let rows = strings.array(8, 24, Some(0x80805CDF))?;
        let row = rows
            .into_iter()
            .find(|at| strings.u32(*at).ok() == Some(donor))
            .context("native donor strings")?;
        let item = r.tag(strings.u32(row + 16)?, None)?;
        if item.u64(0x70)? == 0 {
            return Ok(Limits::default());
        }
        let resource = item.pointer(0x70)?;
        ensure!(
            resource >= 4 && item.u32(resource - 4)? == 0x80805CF1,
            "native stat-group resource differs"
        );
        let index = i32::from_le_bytes(item.bytes(resource + 0x14)?);
        if index < 0 {
            return Ok(Limits::default());
        }
        usize::try_from(index)?
    };
    let groups = r.tag(globals.u32(16 + 60 * 16)?, None)?;
    let rows = groups.array(8, 0x38, Some(0x80805D02))?;
    let row = *rows.get(index).context("native stat-group index")?;
    let maximum = i32::from_le_bytes(groups.bytes(row + 0x30)?);
    let mut minimum = BTreeMap::new();
    for scaled in groups.array(row + 0x10, 0x18, Some(0x80805D06))? {
        let points = groups.array(scaled + 8, 8, Some(0x80807D1A))?;
        let values = points
            .iter()
            .map(|at| Ok(i32::from_le_bytes(groups.bytes(*at)?)))
            .collect::<Result<Vec<_>>>()?;
        if let Some(value) = values.into_iter().min() {
            ensure!(value <= maximum, "native stat-group bounds are reversed");
            minimum.insert(u16::from(groups.u8(scaled)?), value);
        }
    }
    Ok(Limits {
        maximum: Some(maximum),
        minimum,
    })
}

impl Limits {
    pub(super) fn retain(
        &self,
        stats: Vec<Value>,
        fallbacks: &mut Vec<Value>,
    ) -> Result<Vec<Value>> {
        let mut result = Vec::new();
        for stat in stats {
            let index = u16::try_from(stat["definition_index"].as_u64().context("stat index")?)?;
            let value = i32::try_from(stat["value"].as_i64().context("stat value")?)?;
            let minimum = self.minimum.get(&index).copied();
            if minimum.is_some_and(|min| value < min) || self.maximum.is_some_and(|max| value > max)
            {
                fallbacks.push(json!({"definition_index":index,"source_value":value,"minimum":minimum,"maximum":self.maximum,"reason":"value outside target stat group, retained donor value"}));
            } else {
                result.push(stat);
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn target_bounds_preserve_supported_source_values_and_inherit_others() {
        let limits = Limits {
            maximum: Some(100),
            minimum: BTreeMap::from([(7, 10)]),
        };
        for (index, value, retained) in [
            (7, 0, false),
            (7, 10, true),
            (7, 100, true),
            (7, 101, false),
            (8, 0, true),
            (8, 101, false),
        ] {
            let mut fallback = vec![];
            let input = vec![json!({"definition_index":index,"value":value})];
            let output = limits.retain(input.clone(), &mut fallback).unwrap();
            assert_eq!(!output.is_empty(), retained);
            if retained {
                assert_eq!(output, input);
                assert!(fallback.is_empty());
            } else {
                assert_eq!(fallback[0]["source_value"], value);
            }
        }
        let mut fallback = vec![];
        assert_eq!(
            Limits::default()
                .retain(
                    vec![json!({"definition_index":7,"value":350})],
                    &mut fallback
                )
                .unwrap()
                .len(),
            1
        );
    }
}

//! Native stat-group bounds used by the package compiler.
use super::*;

/// One stat a stat group shows: its definition index, its display flags and its curve from
/// investment value to shown value.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Shown {
    pub index: u16,
    pub numeric: bool,
    pub linear: bool,
    pub display: Vec<[i32; 2]>,
}

/// The gameplay donor's stat group.
pub(super) struct Native {
    pub maximum: Option<i32>,
    pub shown: Vec<Shown>,
}

#[derive(Default)]
pub(super) struct Limits {
    maximum: Option<i32>,
    minimum: BTreeMap<u16, i32>,
}

fn shown(groups: &Payload, row: usize) -> Result<Vec<Shown>> {
    groups
        .array(row + 0x10, 0x18, Some(0x80805D06))?
        .into_iter()
        .map(|scaled| {
            let display = groups
                .array(scaled + 8, 8, Some(0x80807D1A))?
                .into_iter()
                .map(|at| {
                    Ok([
                        i32::from_le_bytes(groups.bytes(at)?),
                        i32::from_le_bytes(groups.bytes(at + 4)?),
                    ])
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Shown {
                index: u16::from(groups.u8(scaled)?),
                numeric: groups.u8(scaled + 1)? != 0,
                linear: groups.u8(scaled + 3)? != 0,
                display,
            })
        })
        .collect()
}

pub(super) fn read(
    r: &mut Reader,
    globals: &Payload,
    donor: u32,
    recipe: &Value,
) -> Result<Native> {
    let groups = r.tag(globals.u32(16 + 60 * 16)?, None)?;
    let rows = groups.array(8, 0x38, Some(0x80805D02))?;
    let none = || Native {
        maximum: None,
        shown: Vec::new(),
    };
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
            return Ok(none());
        }
        let resource = item.pointer(0x70)?;
        ensure!(
            resource >= 4 && item.u32(resource - 4)? == 0x80805CF1,
            "native stat-group resource differs"
        );
        let index = i32::from_le_bytes(item.bytes(resource + 0x14)?);
        if index < 0 {
            return Ok(none());
        }
        usize::try_from(index)?
    };
    let row = *rows.get(index).context("native stat-group index")?;
    let maximum = i32::from_le_bytes(groups.bytes(row + 0x30)?);
    let shown = shown(&groups, row)?;
    for stat in &shown {
        if let Some(value) = stat.display.iter().map(|[value, _]| *value).min() {
            ensure!(value <= maximum, "native stat-group bounds are reversed");
        }
    }
    Ok(Native {
        maximum: Some(maximum),
        shown,
    })
}

impl Limits {
    /// The bounds a group with this maximum and these stats puts on investment values.
    pub(super) fn of(maximum: Option<i32>, shown: &[Shown]) -> Self {
        Self {
            maximum,
            minimum: shown
                .iter()
                .filter_map(|stat| {
                    Some((
                        stat.index,
                        stat.display.iter().map(|[value, _]| *value).min()?,
                    ))
                })
                .collect(),
        }
    }

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

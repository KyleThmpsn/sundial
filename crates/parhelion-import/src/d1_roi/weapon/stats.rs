//! Base investment stat rows of a source weapon and the display group its strings name.
use super::*;

impl Source {
    /// The definition's own stat rows (`8080190C`, ordinal and value in the shared stat
    /// table's ordinal space) and the display group the strings record selects, with each
    /// shown stat's curve from investment value to shown value.
    pub(in super::super) fn weapon_stats(
        &self,
        item: &Weapon,
        assets: &mut BTreeMap<u32, Payload>,
    ) -> Result<Value> {
        let definition = self.read(item.definition)?;
        let holder = definition.pointer(0x50)?;
        ensure!(
            definition.u32(holder - 4)? == 0x8080178B,
            "Source stat holder type differs"
        );
        let (stats_tag, stats) = self.table(0x808017EC)?;
        let stat_rows = stats.array(8, 16, Some(0x80801870))?;
        let stat_hash = |ordinal: usize| -> Result<u32> {
            stats.u32(
                *stat_rows
                    .get(ordinal)
                    .context("Source stat ordinal exceeds the stat table")?,
            )
        };
        let mut rows = Vec::new();
        let mut seen = BTreeSet::new();
        for row in definition.array(holder, 8, Some(0x8080190C))? {
            let ordinal = usize::try_from(definition.u32(row)?)?;
            ensure!(seen.insert(ordinal), "Source weapon sets a stat twice");
            rows.push(json!({"index":ordinal,"hash":stat_hash(ordinal)?,
                "value":i32::from_le_bytes(definition.bytes::<4>(row + 4)?),
                "record":hex::encode(definition.bytes::<8>(row)?)}));
        }
        ensure!(!rows.is_empty(), "Source weapon has no base stats");
        let strings = self.read(item.strings)?;
        let (groups_tag, groups) = self.table(0x808033C3)?;
        let group = if strings.u64(0x48)? == 0 {
            Value::Null
        } else {
            let resource = strings.pointer(0x48)?;
            ensure!(
                strings.u32(resource - 4)? == 0x808034D4,
                "Source stat display resource differs"
            );
            let index = strings.u32(resource + 12)?;
            if index == u32::MAX {
                Value::Null
            } else {
                let row = *groups
                    .array(8, 0x38, Some(0x8080346B))?
                    .get(usize::try_from(index)?)
                    .context("Source stat group exceeds its table")?;
                ensure!(
                    groups.u64(row + 0x20)? == 0,
                    "Source stat group fills a list the translation does not read"
                );
                let mut shown = Vec::new();
                for scaled in groups.array(row + 0x10, 0x18, Some(0x808036C6))? {
                    ensure!(
                        groups.bytes::<6>(scaled + 2)? == [0; 6],
                        "Unknown source scaled stat flags"
                    );
                    let ordinal = usize::from(groups.u8(scaled)?);
                    let display = groups
                        .array(scaled + 8, 8, Some(0x808046EA))?
                        .into_iter()
                        .map(|at| {
                            Ok([
                                i32::from_le_bytes(groups.bytes::<4>(at)?),
                                i32::from_le_bytes(groups.bytes::<4>(at + 4)?),
                            ])
                        })
                        .collect::<Result<Vec<_>>>()?;
                    shown.push(json!({"index":ordinal,"hash":stat_hash(ordinal)?,
                        "numeric":groups.u8(scaled + 1)? != 0,"display":display}));
                }
                json!({"index":index,"hash":groups.u32(row)?,
                    "maximum":i32::from_le_bytes(groups.bytes::<4>(row + 0x30)?),
                    "resource":hex::encode(strings.bytes::<16>(resource)?),"stats":shown})
            }
        };
        assets.insert(stats_tag, stats);
        assets.insert(groups_tag, groups);
        Ok(
            json!({"holder":holder,"stat_table":stats_tag,"group_table":groups_tag,
            "rows":rows,"group":group}),
        )
    }
}

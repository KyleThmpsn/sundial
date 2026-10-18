//! D1 has no Collections set membership for these items. Use complete, unique
//! five-slot families sharing all three default dye assignments as a fallback.
use super::*;

pub(super) fn assign(source: &Source, armor: &mut [Armor]) -> Result<()> {
    let (_, dyes) = source.table(0x80803420)?;
    let rows = dyes.array(8, 4, Some(0x808033CC))?;
    let mut families: BTreeMap<(u8, u8, [u32; 3]), Vec<usize>> = BTreeMap::new();
    for (index, item) in armor.iter().enumerate() {
        let definition = source.read(item.definition)?;
        let gear = definition.pointer(0x60)?;
        if definition.u64(gear + 0x20)? != 0 || definition.u64(gear + 0x50)? != 0 {
            continue;
        }
        let defaults = definition.array(gear + 0x30, 4, Some(0x8080101C))?;
        if defaults.len() != 3 {
            continue;
        }
        let mut assignments = [None; 3];
        for row in defaults {
            let channel = usize::from(definition.u8(row)?);
            let Some(target) = assignments.get_mut(channel) else {
                continue;
            };
            ensure!(target.is_none(), "Duplicate source armor dye channel");
            let ordinal = usize::from(definition.u16(row + 2)?);
            *target = Some(dyes.u32(*rows.get(ordinal).context("Source armor dye ordinal")?)?);
        }
        let [Some(first), Some(second), Some(third)] = assignments else {
            continue;
        };
        families
            .entry((item.class_type, item.rarity, [first, second, third]))
            .or_default()
            .push(index);
    }
    for ((class, rarity, dyes), members) in families {
        if members.len() != 5
            || members
                .iter()
                .map(|&index| armor[index].slot)
                .collect::<BTreeSet<_>>()
                != BTreeSet::from([1, 2, 3, 4, 5])
        {
            continue;
        }
        let key = format!(
            "d1_roi/armor/{class}/{rarity}/{:08X}/{:08X}/{:08X}",
            dyes[0], dyes[1], dyes[2]
        );
        for index in members {
            armor[index].collection_set = Some(key.clone());
        }
    }
    Ok(())
}

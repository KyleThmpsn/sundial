//! Discover equippable source families and distinguish existing native items.
use crate::d2_mot::{
    localization,
    reader::{Reader, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug)]
pub enum ScanProgress {
    CheckingCache,
    LoadingCachedItems,
    OpeningModernPackages,
    OpeningNativePackages,
    ReadingItems {
        completed: usize,
        total: usize,
        weapons: usize,
    },
    SavingCatalog,
}

pub fn weapons(modern: &Path, native: &Path, out: &Path) -> Result<Value> {
    weapons_with_progress(modern, native, out, |_| {})
}

/// Installed identities are independent of modern labels, icons and item families.
pub(crate) fn native_hashes(native: &Path, out: &Path) -> Result<BTreeSet<u32>> {
    let mut old = Reader::discovery(native, &out.join("native"), false)?;
    let globals_tag = old
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|t| t.name == "investment_globals")
        .context("native globals")?
        .hash
        .0;
    let globals = old.tag(globals_tag, None)?;
    let root = old.tag(globals.u32(16)?, Some(0x80807D84))?;
    let items = old.tag(root.u32(8 + 48 * 16)?, None)?;
    let native_hashes = items
        .array(8, 24, Some(0x80807BE8))?
        .into_iter()
        .map(|at| items.u32(at))
        .collect::<Result<BTreeSet<_>>>()?;
    old.finish()?;
    Ok(native_hashes)
}

pub(crate) fn match_native(
    weapons: &mut [super::service::Weapon],
    native_hashes: &BTreeSet<u32>,
) -> Result<()> {
    for weapon in weapons {
        weapon.native_item = native_hashes.contains(&weapon.hash);
        weapon.present_in_native = weapon.native_item
            || native_hashes.contains(&super::service::destination_hash(weapon.hash)?);
    }
    Ok(())
}

pub fn weapons_with_progress(
    modern: &Path,
    native: &Path,
    out: &Path,
    mut progress: impl FnMut(ScanProgress),
) -> Result<Value> {
    progress(ScanProgress::OpeningModernPackages);
    let mut r = Reader::discovery(modern, out, true)?;
    progress(ScanProgress::OpeningNativePackages);
    let native_hashes = native_hashes(native, out)?;
    let tags = r.classes(0x80805499);
    ensure!(tags.len() == 1, "ambiguous source item strings");
    let table = r.tag(tags[0], None)?;
    let mut definitions = BTreeMap::new();
    for tag in r.classes(0x80807997) {
        let items = r.tag(tag, None)?;
        for row in items.array(8, 32, Some(0x8080799B))? {
            definitions.insert(items.u32(row)?, r.ref64(&items, row + 16)?);
        }
    }
    let mut types = BTreeMap::new();
    let mut labels = localization::Resolver::default();
    let mut weapons = Vec::new();
    let mut unavailable = Vec::new();
    let items = table.array(8, 32, Some(0x8080549D))?;
    let total = items.len();
    progress(ScanProgress::ReadingItems {
        completed: 0,
        total,
        weapons: 0,
    });
    let mut last_update = Instant::now();
    for (index, at) in items.into_iter().enumerate() {
        let hash = table.u32(at)?;
        let result = (|| -> Result<()> {
            let tag = r.ref64(&table, at + 16)?;
            let strings = r.tag(tag, Some(0x8080549F))?;
            let key = (strings.u32(0x8C)?, strings.u32(0x90)?);
            let kind = if let Some(name) = types.get(&key) {
                name
            } else {
                let name = labels.label(&mut r, &strings, 0x8C)?;
                types.entry(key).or_insert(name)
            };
            let Some(&definition) = definitions.get(&hash) else {
                return Ok(());
            };
            let item = r.tag(definition, Some(0x8080799D))?;
            let Some((family, bucket_hash, class_type)) =
                super::service::Family::source(&item, &strings)?
            else {
                return Ok(());
            };
            let name = labels.label(&mut r, &strings, 0x80)?;
            if !name.is_empty() {
                // Display-only definitions cannot equip. Do not infer this from duplicate names.
                let dummy = family != super::service::Family::Shader && item.u64(0x18)? == 0;
                let icon_index = strings.u32(0x78)?;
                let icon_index = (icon_index != u32::MAX).then_some(icon_index);
                let present = native_hashes.contains(&hash)
                    || native_hashes.contains(&super::service::destination_hash(hash)?);
                let native_item = native_hashes.contains(&hash);
                // Optional browse fields must not hide an otherwise discoverable item.
                let metadata = super::gameplay::browse_metadata(
                    &mut r,
                    &item,
                    &strings,
                    family == super::service::Family::Weapon,
                );
                let (rarity, ammo, damage) = match metadata {
                    Ok(metadata) => (Some(metadata.rarity), metadata.ammo, metadata.damage),
                    Err(error) => {
                        unavailable.push(json!({"hash":hash,"index":index,"scope":"browse_metadata","reason":format!("{error:#}")}));
                        (None, None, None)
                    }
                };
                weapons.push(json!({"hash":hash,"index":index,"name":name,"weapon_type":kind,"bucket_hash":bucket_hash,"class_type":class_type,"rarity":rarity,"ammo":ammo,"damage":damage,"present_in_native":present,"native_item":native_item,"dummy":dummy,"icon_index":icon_index}));
            }
            Ok(())
        })();
        if let Err(error) = result {
            unavailable.push(json!({"hash":hash,"index":index,"reason":format!("{error:#}")}));
        }
        if last_update.elapsed() >= Duration::from_millis(100) || index + 1 == total {
            progress(ScanProgress::ReadingItems {
                completed: index + 1,
                total,
                weapons: weapons.len(),
            });
            last_update = Instant::now();
        }
    }
    progress(ScanProgress::SavingCatalog);
    let result = json!({"weapons":weapons,"unavailable":unavailable,"conversion_verified":false});
    write_json(&out.join("catalog.json"), &result)?;
    r.finish()?;
    Ok(result)
}

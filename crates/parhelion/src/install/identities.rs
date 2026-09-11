//! Recover ownership from the installed native tables, not names or stale recipe manifests.
use super::*;
use crate::tag_payload::{array_at, read_u16, read_u32};
use sundial::package_authoring::{
    investment_schema::*, open_shadowkeep_package_manager, resolve_live_named_tag,
};
use tiger_pkg::TagHash;
mod placement;
mod sockets;
pub(super) use placement::slot_replacement;
pub(super) use sockets::generation_socket_defaults;

pub(super) fn installed_identities(
    target: &Path,
) -> Result<(BTreeSet<u32>, Vec<AuthoredCollectionUnlock>), String> {
    generation_identities(target, target)
}

/// Read an authored generation against the same installation's stock baseline.
/// The staged set contains complete canonical overlays, not a second stock installation.
pub(super) fn generation_identities(
    target: &Path,
    authored_directory: &Path,
) -> Result<(BTreeSet<u32>, Vec<AuthoredCollectionUnlock>), String> {
    with_generation(target, authored_directory, read_identities)
}

fn with_generation<T>(
    target: &Path,
    authored_directory: &Path,
    read: impl FnOnce(&Path) -> Result<T, String>,
) -> Result<T, String> {
    super::validation::decoder::ensure_initialized(target)?;
    if !paths_equal(target, authored_directory) {
        // Authored overlays can reference physical blocks in older stock generations.
        // Reopen the staged generation in a complete read-only view; never assume its
        // output directory alone contains those stock blocks.
        let ignored = all_authored_packages()
            .map(|p| p.file_name.to_owned())
            .collect::<Vec<_>>();
        let view = crate::workflow::FilteredPackageView::create(target, &ignored)?;
        let result = (|| {
            for profile in all_authored_packages() {
                let path = authored_directory.join(profile.file_name);
                if path.is_file() {
                    view.add_overlay(&path)?;
                }
            }
            read(view.path())
        })();
        return view.finish(result);
    }
    read(target)
}

fn read_identities(
    target: &Path,
) -> Result<(BTreeSet<u32>, Vec<AuthoredCollectionUnlock>), String> {
    let manager = open_shadowkeep_package_manager(target)?;
    let globals = resolve_live_named_tag(&manager, "investment_globals", None)?;
    // Follow each generation's own root. Do not assume authored root/table tags are unchanged.
    let tables = |authored| -> Result<(Vec<u8>, Vec<u8>), String> {
        let directory = target;
        let globals = payload(directory, globals, authored)?;
        let root = payload(
            directory,
            TagHash(investment_globals_table_tag(&globals, 0)?),
            authored,
        )?;
        Ok((
            payload(
                directory,
                TagHash(investment_root_table_tag(
                    &root,
                    ROOT_ITEM_DEFINITION_TABLE_SLOT,
                )?),
                authored,
            )?,
            payload(
                directory,
                TagHash(investment_root_table_tag(
                    &root,
                    ROOT_UNLOCK_FLAG_DEFINITION_TABLE_SLOT,
                )?),
                authored,
            )?,
        ))
    };
    let (stock_items, stock_unlocks) = tables(false)?;
    let (items, unlocks) = tables(true)?;
    identify(&stock_items, &items, &stock_unlocks, &unlocks)
}

fn payload(target: &Path, tag: TagHash, authored: bool) -> Result<Vec<u8>, String> {
    let profile = canonical_package(tag.pkg_id())
        .ok_or_else(|| format!("Ownership table {tag} is outside the recognized package chains"))?;
    let name = if authored {
        profile.authored_file_name.to_owned()
    } else {
        profile.stock_file_name(profile.stock_patch_id)
    };
    let path = target.join(name);
    reject_symlink(&path, "ownership source").map_err(|e| e.to_string())?;
    let package = PackageD2PreBL::open(path.to_str().ok_or("Package path is not UTF-8")?)
        .map_err(|e| e.to_string())?;
    package
        .read_entry(usize::from(tag.entry_index()))
        .map_err(|e| e.to_string())
}

fn rows(data: &[u8], class: u32, size: usize) -> Result<Vec<&[u8]>, String> {
    let (count, _, offset, actual) = array_at(data, 8).map_err(|e| e.to_string())?;
    if actual != class || count > usize::from(u16::MAX) {
        return Err("Unsupported ownership table shape".into());
    }
    let end = offset
        .checked_add(count.checked_mul(size).ok_or("Table overflow")?)
        .ok_or("Table overflow")?;
    Ok(data
        .get(offset..end)
        .ok_or("Truncated ownership table")?
        .chunks_exact(size)
        .collect())
}

fn identify(
    stock_items: &[u8],
    items: &[u8],
    stock_unlocks: &[u8],
    unlocks: &[u8],
) -> Result<(BTreeSet<u32>, Vec<AuthoredCollectionUnlock>), String> {
    let stock_items = rows(
        stock_items,
        ITEM_DEFINITION_INDEX_ROW_CLASS,
        ITEM_INDEX_ROW_SIZE,
    )?;
    let items = rows(items, ITEM_DEFINITION_INDEX_ROW_CLASS, ITEM_INDEX_ROW_SIZE)?;
    let stock_unlocks = rows(
        stock_unlocks,
        UNLOCK_FLAG_DEFINITION_ROW_CLASS,
        UNLOCK_FLAG_DEFINITION_ROW_SIZE,
    )?;
    let unlocks = rows(
        unlocks,
        UNLOCK_FLAG_DEFINITION_ROW_CLASS,
        UNLOCK_FLAG_DEFINITION_ROW_SIZE,
    )?;
    if items.len() < stock_items.len() || unlocks.len() < stock_unlocks.len() {
        return Err("Authored ownership tables removed stock definitions".into());
    }
    // Item indices and unlock compact addresses are append-only authoring contracts.
    for (stock, current) in stock_items.iter().zip(&items) {
        if stock != current {
            return Err("A stock item index row changed; automatic cleanup is unsafe".into());
        }
    }
    for (stock, current) in stock_unlocks.iter().zip(&unlocks) {
        if stock.get(..8) != current.get(..8) {
            return Err("A stock unlock identity changed; automatic cleanup is unsafe".into());
        }
    }
    let hash = |row: &[u8]| read_u32(row, 0).map_err(|e| e.to_string());
    let stock_hashes = stock_items
        .iter()
        .map(|row| hash(row))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let mut hashes = BTreeSet::new();
    for row in items.iter().skip(stock_items.len()) {
        let value = hash(row)?;
        if value == 0 || value == u32::MAX || stock_hashes.contains(&value) || !hashes.insert(value)
        {
            return Err("Authored item identities overlap stock or each other".into());
        }
    }
    let address = |row: &[u8]| -> Result<(u16, u16), String> {
        Ok((
            read_u16(row, 4).map_err(|e| e.to_string())?,
            read_u16(row, 6).map_err(|e| e.to_string())?,
        ))
    };
    let mut addresses = stock_unlocks
        .iter()
        .map(|row| address(row).map(|(code, slot)| (code & 0xFF, slot)))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let mut unlock_hashes = stock_unlocks
        .iter()
        .map(|row| hash(row))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let mut owned_unlocks = Vec::new();
    for (index, row) in unlocks.iter().enumerate().skip(stock_unlocks.len()) {
        let (bank, slot) = address(row)?;
        if bank != u16::from(ACCOUNT_UNLOCK_BANK)
            || usize::from(slot)
                >= sundial::package_authoring::SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY
            || !addresses.insert((bank, slot))
            || !unlock_hashes.insert(hash(row)?)
        {
            return Err("Authored unlock has an unsupported or overlapping storage address".into());
        }
        owned_unlocks.push(AuthoredCollectionUnlock {
            definition_index: index as u16,
            bank: bank as u8,
            slot,
        });
    }
    if hashes.is_empty() || owned_unlocks.is_empty() {
        return Err("The installed set has no provable authored item/unlock identities".into());
    }
    Ok((hashes, owned_unlocks))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn table(class: u32, size: usize, identities: &[(u32, u16, u16)]) -> Vec<u8> {
        use crate::tag_payload::{write_u16, write_u32, write_u64};
        let mut data = vec![0; 40 + size * identities.len()];
        write_u64(&mut data, 8, identities.len() as u64).unwrap();
        write_u64(&mut data, 16, 8).unwrap();
        write_u64(&mut data, 24, identities.len() as u64).unwrap();
        write_u32(&mut data, 32, class).unwrap();
        for (index, &(hash, bank, slot)) in identities.iter().enumerate() {
            let offset = 40 + index * size;
            write_u32(&mut data, offset, hash).unwrap();
            write_u16(&mut data, offset + 4, bank).unwrap();
            write_u16(&mut data, offset + 6, slot).unwrap();
        }
        data
    }
    #[test]
    fn ownership_is_append_only_and_rejects_stock_address_reuse() {
        let item_table =
            |rows: &[_]| table(ITEM_DEFINITION_INDEX_ROW_CLASS, ITEM_INDEX_ROW_SIZE, rows);
        let flag_table = |rows: &[_]| {
            table(
                UNLOCK_FLAG_DEFINITION_ROW_CLASS,
                UNLOCK_FLAG_DEFINITION_ROW_SIZE,
                rows,
            )
        };
        let stock_items = item_table(&[(100, 0, 0)]);
        let stock_flags = flag_table(&[(200, 1, 42)]);
        let items = item_table(&[(100, 0, 0), (101, 0, 0)]);
        let flags = flag_table(&[(200, 1, 42), (201, 1, 43)]);
        let (hashes, unlocks) = identify(&stock_items, &items, &stock_flags, &flags).unwrap();
        assert_eq!(hashes, BTreeSet::from([101]));
        assert_eq!(
            unlocks,
            vec![AuthoredCollectionUnlock {
                definition_index: 1,
                bank: 1,
                slot: 43
            }]
        );
        let overlapping = flag_table(&[(200, 1, 42), (201, 1, 42)]);
        assert!(identify(&stock_items, &items, &stock_flags, &overlapping).is_err());
        let variant_stock = flag_table(&[(200, 0x0101, 42)]);
        let variant_collision = flag_table(&[(200, 0x0101, 42), (201, 1, 42)]);
        assert!(identify(&stock_items, &items, &variant_stock, &variant_collision).is_err());
        let changed_stock = item_table(&[(999, 0, 0), (101, 0, 0)]);
        assert!(identify(&stock_items, &changed_stock, &stock_flags, &flags).is_err());
        let colliding_hash = item_table(&[(100, 0, 0), (100, 0, 0)]);
        assert!(identify(&stock_items, &colliding_hash, &stock_flags, &flags).is_err());
    }
    #[test]
    #[ignore = "requires an installed native package set; read-only"]
    fn native_account_cleanup_review_is_read_only() {
        let path = PathBuf::from(
            std::env::var("PARHELION_UNINSTALL_REVIEW_PACKAGES").expect("set native packages path"),
        );
        let before = preview_uninstall(&path).unwrap();
        let plan = preview_uninstall_with_account_cleanup(&path).unwrap();
        let cleanup = plan.account_cleanup().unwrap_or_else(|| {
            panic!(
                "{}",
                plan.account_cleanup_error().unwrap_or("missing cleanup")
            )
        });
        assert_eq!(
            fs::read(&cleanup.settings_path).unwrap(),
            cleanup.original_bytes
        );
        assert_eq!(before, preview_uninstall(&path).unwrap());
        eprintln!(
            "Read-only review: {} item instances, {} plugs, {} collection flags",
            cleanup.removed_items.values().sum::<usize>(),
            cleanup.cleared_plugs,
            cleanup.cleared_unlocks
        );
    }
}

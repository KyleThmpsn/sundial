//! The index over real packages: known uses are listed, every declared reference of a walked
//! resource is listed back, the second read comes from the shards, and a cancelled read
//! says so.
use std::{path::PathBuf, sync::atomic::AtomicBool};

use super::{CANCELLED, read};
use crate::package_runtime::references::closure;

fn clean_packages() -> PathBuf {
    PathBuf::from(
        std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
            .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages"),
    )
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to Shadowkeep packages"]
fn real_packages_list_every_declared_use_and_reuse_their_shards() {
    let manager = crate::package_authoring::open_shadowkeep_package_manager(&clean_packages())
        .expect("clean-stock manager should open");
    let quiet = |_: usize, _: usize| {};
    let first = read(&manager, &AtomicBool::new(false), quiet).unwrap();
    assert!(first.usage.resources > 0);
    // Uses established by hand on 2026-09-28: the Barricade bank is a component of the
    // Barricade ability entity, and Devour's status nest belongs to two hop-on entities.
    assert!(
        first.of(0x80BC_2BCC).contains(&0x80B8_00B0),
        "{:08X?}",
        first.of(0x80BC_2BCC)
    );
    for entity in [0x80B8_0AA0, 0x80B8_057B] {
        assert!(
            first.of(0x80BC_2F97).contains(&entity),
            "{:08X?}",
            first.of(0x80BC_2F97)
        );
    }
    // Every reference the forward walk declares from a resource is listed back to it.
    for source in [0x80B8_00B0, 0x80B8_0AA0, 0x80BC_57CB] {
        let declared = closure(&manager, [source]).unwrap();
        let direct = declared
            .iter()
            .filter(|reference| reference.parent == source && reference.tag != source)
            .map(|reference| reference.tag)
            .collect::<std::collections::BTreeSet<_>>();
        assert!(!direct.is_empty(), "{source:08X} declares nothing");
        for tag in direct {
            assert!(
                first.of(tag).contains(&source),
                "{tag:08X} does not list {source:08X}: {:08X?}",
                first.of(tag)
            );
        }
    }

    let second = read(&manager, &AtomicBool::new(false), quiet).unwrap();
    assert_eq!(second.usage.scanned_packages, 0, "{:?}", second.usage);
    assert_eq!(
        second.usage.reused_packages,
        first.usage.reused_packages + first.usage.scanned_packages
    );
    assert_eq!(second.parents, first.parents);
    assert_eq!(second.errors, first.errors);

    assert_eq!(
        read(&manager, &AtomicBool::new(true), quiet).unwrap_err(),
        CANCELLED
    );
}

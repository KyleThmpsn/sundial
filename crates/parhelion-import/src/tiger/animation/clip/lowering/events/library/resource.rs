//! Native event references outside the chosen weapon's animation bank.
use super::{Library, Resources};
use crate::tiger::reader::{Reader, package_stamp};
use anyhow::{Result, ensure};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

type Catalogs = BTreeMap<PathBuf, (String, Arc<Resources>)>;

pub(super) fn catalog(reader: &Reader) -> Result<Arc<Resources>> {
    ensure!(
        reader.is_native(),
        "Event resources require native packages"
    );
    let packages = reader.packages();
    let stamp = package_stamp(packages)?;
    static CATALOGS: OnceLock<Mutex<Catalogs>> = OnceLock::new();
    let mut catalogs = crate::cancellation::mutex(CATALOGS.get_or_init(Mutex::default))?;
    if let Some((previous, resources)) = catalogs.get(packages)
        && *previous == stamp
    {
        return Ok(Arc::clone(resources));
    }
    // Scanning every native clip takes seconds, so the resources are kept on disk for the package
    // set as well, one row per class and path.
    let rows = crate::cache::by_packages(("tiger", "event-resources", 1), packages, || {
        let mut discovery =
            Reader::discovery(packages, &reader.output.join("event-resources"), false)?;
        let clips = discovery.classes(0x80808F49);
        ensure!(
            !clips.is_empty(),
            "Native animation resource corpus is empty"
        );
        let mut library = Library::default();
        for tag in clips {
            crate::cancellation::check()?;
            // Unsupported event layouts supply no binding evidence. Unreadable
            // required package input still fails instead of caching a partial scan.
            library.add_native(discovery.tag(tag, Some(0x80808F49))?.as_ref());
        }
        ensure!(
            package_stamp(packages)? == stamp,
            "Native packages changed during event discovery"
        );
        Ok(library.resources.into_iter().collect::<Vec<_>>())
    })?;
    let resources = rows.into_iter().collect::<Resources>();
    ensure!(
        !resources.is_empty(),
        "Native clips contain no supported resource events"
    );
    let resources = Arc::new(resources);
    catalogs.insert(packages.to_owned(), (stamp, Arc::clone(&resources)));
    Ok(resources)
}

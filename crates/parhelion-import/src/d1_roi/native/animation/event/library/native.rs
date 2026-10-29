//! Complete native event witnesses, cached by mounted package identity.
use super::*;
use crate::tiger::reader::package_stamp;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

type Records = BTreeMap<Key, BTreeSet<Option<u32>>>;
type Catalogs = BTreeMap<PathBuf, (String, Arc<Records>)>;

pub(super) fn catalog(reader: &Reader) -> Result<Arc<Records>> {
    ensure!(
        reader.is_native(),
        "D1 event witnesses require native packages"
    );
    let packages = reader.packages();
    let stamp = package_stamp(packages)?;
    static CATALOGS: OnceLock<Mutex<Catalogs>> = OnceLock::new();
    let mut cache = crate::cancellation::mutex(CATALOGS.get_or_init(Mutex::default))?;
    if let Some((previous, records)) = cache.get(packages)
        && *previous == stamp
    {
        return Ok(Arc::clone(records));
    }
    // Scanning every native clip takes seconds, so the witnesses are kept on disk for the package
    // set as well, one row per key: class, hexadecimal data, path and resources.
    let rows = crate::cache::by_packages(("d1_roi", "event-witnesses", 1), packages, || {
        let mut discovery =
            Reader::discovery(packages, &reader.output.join("d1-event-witnesses"), false)?;
        let clips = discovery.classes(0x80808F49);
        ensure!(!clips.is_empty(), "Required native event corpus is empty");
        let mut records = Records::new();
        for tag in clips {
            crate::cancellation::check()?;
            let clip = discovery.tag(tag, Some(0x80808F49))?;
            let Ok(events) = read(&clip, false) else {
                continue;
            };
            for event in events {
                if let Some(resource) = event.resource {
                    discovery.tag(
                        resource,
                        Some(resource_class(
                            event.key.path.as_deref().context("Native event path")?,
                            false,
                        )),
                    )?;
                }
                records.entry(event.key).or_default().insert(event.resource);
            }
        }
        ensure!(
            !records.is_empty() && package_stamp(packages)? == stamp,
            "Native event corpus is empty or changed during discovery"
        );
        Ok(records
            .into_iter()
            .map(|(key, resources)| (key.class, hex::encode(key.data), key.path, resources))
            .collect::<Vec<_>>())
    })?;
    let mut records = Records::new();
    for (class, data, path, resources) in rows {
        let key = Key {
            class,
            data: hex::decode(data)?,
            path,
        };
        records.insert(key, resources);
    }
    ensure!(!records.is_empty(), "Native event corpus is empty");
    let records = Arc::new(records);
    cache.insert(packages.to_owned(), (stamp, Arc::clone(&records)));
    Ok(records)
}

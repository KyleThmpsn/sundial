//! Build-scoped logical records mapped to bounded native package tables.
use super::*;
use std::sync::Mutex;

#[derive(Clone, Copy)]
struct Page {
    start: usize,
    end: usize,
    package: u16,
    first: usize,
}

#[derive(Clone)]
struct Pool {
    pages: Vec<Page>,
    exposed_end: usize,
}

static POOLS: Mutex<[Option<Pool>; 2]> = Mutex::new([None, None]);

pub(crate) struct Checkpoint {
    saved: Option<([Option<Pool>; 2], crate::mod_family::build::Reservations)>,
}

pub(crate) fn checkpoint() -> Checkpoint {
    let pools = POOLS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    Checkpoint {
        saved: Some((pools, crate::mod_family::build::reservations())),
    }
}

impl Checkpoint {
    pub(crate) fn commit(mut self) {
        self.saved = None;
    }
}

impl Drop for Checkpoint {
    fn drop(&mut self) {
        if let Some((pools, reservations)) = self.saved.take() {
            *POOLS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = pools;
            crate::mod_family::build::restore_reservations(reservations);
        }
    }
}

pub(super) fn clear() {
    *POOLS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = [None, None];
}

fn with_pool<T>(
    private: bool,
    start: usize,
    f: impl FnOnce(&mut Pool) -> AuthoringResult<T>,
) -> AuthoringResult<T> {
    let mut pools = POOLS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let pool = pools[usize::from(private)].get_or_insert_with(|| {
        let host = crate::package_profile::host();
        let (package, first) = if private {
            (PRIVATE_PERK_RUNTIME_PACKAGE_ID, start)
        } else {
            (host.id, host.first)
        };
        Pool {
            pages: vec![Page {
                start: first,
                end: MAX_PACKAGE_ENTRY_COUNT.max(first),
                package,
                first,
            }],
            exposed_end: first,
        }
    });
    f(pool)
}

impl Pool {
    fn extend(&mut self, private: bool, start: usize) -> AuthoringResult<()> {
        let package = crate::mod_family::build::record_page(private, self.pages.len() - 1)
            .map_err(AuthoringError::InvalidInput)?;
        let end = start
            .checked_add(MAX_PACKAGE_ENTRY_COUNT)
            .ok_or_else(|| AuthoringError::InvalidInput("Record allocation overflowed".into()))?;
        self.pages.push(Page {
            start,
            end,
            package,
            first: 0,
        });
        Ok(())
    }

    fn page(&mut self, private: bool, index: usize) -> AuthoringResult<Page> {
        while index >= self.pages.last().unwrap().end {
            self.extend(private, self.pages.last().unwrap().end)?;
        }
        self.pages
            .iter()
            .copied()
            .find(|page| (page.start..page.end).contains(&index))
            .ok_or_else(|| AuthoringError::InvalidInput("Record precedes its allocation".into()))
    }
}

pub(super) fn tag(private: bool, start: usize, index: usize) -> AuthoringResult<TagHash> {
    with_pool(private, start, |pool| {
        let page = pool.page(private, index)?;
        pool.exposed_end = pool.exposed_end.max(index + 1);
        AppendedTagAllocator::new(page.package, page.first).assigned_tag(
            index - page.start,
            "Record destination",
            "record",
        )
    })
}

/// Reserve an owner and companion before either address is published. A short final page
/// contains no padding entries, so the logical vector remains dense across package boundaries.
pub(super) fn reserve(
    private: bool,
    start: usize,
    index: usize,
    count: usize,
) -> AuthoringResult<()> {
    if count == 0 || count > MAX_PACKAGE_ENTRY_COUNT {
        return Err(AuthoringError::InvalidInput(
            "Record group does not fit one package".into(),
        ));
    }
    let end = index
        .checked_add(count)
        .ok_or_else(|| AuthoringError::InvalidInput("Record group overflowed".into()))?;
    with_pool(private, start, |pool| {
        let page = pool.page(private, index)?;
        if end > page.end {
            if pool.exposed_end > index || pool.pages.last().unwrap().start != page.start {
                return Err(AuthoringError::InvalidInput(
                    "Record group would move an assigned address".into(),
                ));
            }
            pool.pages.last_mut().unwrap().end = index;
            pool.extend(private, index)?;
        }
        pool.exposed_end = pool.exposed_end.max(end);
        Ok(())
    })
}

pub(super) fn ordinal(private: bool, start: usize, tag: TagHash) -> AuthoringResult<usize> {
    with_pool(private, start, |pool| {
        pool.pages
            .iter()
            .find_map(|page| {
                let local = (tag.entry_index() as usize).checked_sub(page.first)?;
                (tag.pkg_id() == page.package && local < page.end - page.start)
                    .then(|| page.start + local)
                    .and_then(|index| index.checked_sub(start))
            })
            .ok_or_else(|| {
                AuthoringError::InvalidInput(format!("{tag} is outside this record allocation"))
            })
    })
}

pub(super) fn split<T>(private: bool, start: usize, tags: Vec<T>) -> AuthoringResult<Split<T>> {
    let mut first = Vec::new();
    let mut overflow: Vec<(u16, Vec<T>)> = Vec::new();
    let initial = if private {
        PRIVATE_PERK_RUNTIME_PACKAGE_ID
    } else {
        crate::package_profile::host().id
    };
    for (ordinal, value) in tags.into_iter().enumerate() {
        let id = tag(private, start, start + ordinal)?.pkg_id();
        if id == initial {
            first.push(value);
        } else {
            if overflow.last().is_none_or(|(previous, _)| *previous != id) {
                overflow.push((id, Vec::new()));
            }
            overflow.last_mut().unwrap().1.push(value);
        }
    }
    Ok((first, overflow))
}

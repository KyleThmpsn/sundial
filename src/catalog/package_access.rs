//! Coordinates on-demand inspection with the host's package-authoring pause.

use std::sync::{
    RwLock,
    atomic::{AtomicU64, Ordering},
};

/// Every generation comes from one counter for the whole process, so a new access never repeats
/// one an older access handed out, and a model cached under an older catalog never matches a
/// newer one.
static GENERATIONS: AtomicU64 = AtomicU64::new(1);

fn next_generation() -> u64 {
    GENERATIONS.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug)]
pub(crate) struct PackageInspectionAccess {
    suspended: RwLock<bool>,
    generation: AtomicU64,
}

impl Default for PackageInspectionAccess {
    fn default() -> Self {
        Self {
            suspended: RwLock::new(false),
            generation: AtomicU64::new(next_generation()),
        }
    }
}

impl PackageInspectionAccess {
    pub(crate) fn read<T>(&self, read: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let suspended = self
            .suspended
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *suspended {
            return Err("Package inspection is paused while Parhelion is open.".into());
        }
        // The read guard spans opening, reading, and dropping the package manager.
        // Suspension cannot finish until all file handles have been released.
        let result = read();
        drop(suspended);
        result
    }

    pub(super) fn suspend(&self) {
        *self
            .suspended
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.generation.store(next_generation(), Ordering::SeqCst);
    }

    pub(super) fn resume(&self) {
        *self
            .suspended
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
    }

    /// Allows reads again under a new generation, for a catalog that replaced the one earlier
    /// reads came from.
    pub(super) fn renew(&self) {
        let mut suspended = self
            .suspended
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.generation.store(next_generation(), Ordering::SeqCst);
        *suspended = false;
    }

    pub(crate) fn is_suspended(&self) -> bool {
        *self
            .suspended
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suspended_inspection_never_opens_packages_and_resume_restores_reads() {
        let access = PackageInspectionAccess::default();
        assert_eq!(access.read(|| Ok(7)), Ok(7));
        let before = access.generation();
        access.suspend();
        assert!(access.is_suspended());
        assert!(access.generation() > before);
        let blocked: Result<(), String> = access.read(|| panic!("must not touch packages"));
        assert!(blocked.unwrap_err().contains("paused"));
        access.resume();
        assert_eq!(access.read(|| Ok(9)), Ok(9));
    }
}

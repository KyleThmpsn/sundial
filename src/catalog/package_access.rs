//! Coordinates on-demand inspection with the host's package-authoring pause.

use std::sync::{
    RwLock,
    atomic::{AtomicU64, Ordering},
};

#[derive(Debug, Default)]
pub(crate) struct PackageInspectionAccess {
    suspended: RwLock<bool>,
    generation: AtomicU64,
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
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    pub(super) fn resume(&self) {
        *self
            .suspended
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
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
        access.suspend();
        assert!(access.is_suspended());
        assert_eq!(access.generation(), 1);
        let blocked: Result<(), String> = access.read(|| panic!("must not touch packages"));
        assert!(blocked.unwrap_err().contains("paused"));
        access.resume();
        assert_eq!(access.read(|| Ok(9)), Ok(9));
    }

    #[test]
    fn reader_holds_the_gate_until_package_work_finishes() {
        let access = PackageInspectionAccess::default();
        access
            .read(|| {
                assert!(access.suspended.try_write().is_err());
                Ok(())
            })
            .unwrap();
        assert!(access.suspended.try_write().is_ok());
    }
}

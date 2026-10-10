#[cfg(feature = "d2-model-importer")]
use std::path::PathBuf;
use std::{fmt, time::Duration};

#[derive(Debug)]
pub(in crate::item::build::runtime) enum Reason {
    Disabled,
    NoDirectory,
    Directory(String),
    Locked(String),
    CompilerUnavailable,
    Missing,
    CompilerChanged,
    SourceChanged,
    RecipeChanged,
    IdentityChanged,
    NativeChanged(u32),
    NativeUnavailable(u32),
    #[cfg(feature = "d2-model-importer")]
    ImportChanged(PathBuf),
    #[cfg(feature = "d2-model-importer")]
    ImportUnavailable(PathBuf),
    Damaged(String),
    UnsupportedRelocation,
    SharedAnimationChanged,
    RelocationRejected,
    #[cfg(feature = "d2-model-importer")]
    UncacheableImport,
    UntrackedReads,
    UnsupportedFragment,
    ParticleSymbols,
    TooLarge,
    Write(String),
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => write!(f, "[disabled] Disabled by PARHELION_DISABLE_RUNTIME_CACHE"),
            Self::NoDirectory => write!(f, "[no-directory] No cache directory is configured"),
            Self::Directory(error) => write!(
                f,
                "[directory-unavailable] Cache storage is unavailable: {error}"
            ),
            Self::Locked(error) => write!(
                f,
                "[cache-lock] Could not acquire the cache writer lock: {error}"
            ),
            Self::CompilerUnavailable => write!(
                f,
                "[compiler-unavailable] Could not hash the compiler executable"
            ),
            Self::Missing => write!(f, "[no-fragment] No saved fragment"),
            Self::CompilerChanged => {
                write!(f, "[compiler-changed] The compiler executable changed")
            }
            Self::SourceChanged => write!(
                f,
                "[source-changed] Native metadata, tables or source profile changed"
            ),
            Self::RecipeChanged => write!(
                f,
                "[recipe-changed] Recipe or resolved donor inputs changed"
            ),
            Self::IdentityChanged => write!(
                f,
                "[identity-changed] Compiler, native source or recipe inputs changed since this older fragment"
            ),
            Self::NativeChanged(tag) => {
                write!(f, "[native-input-changed] Native payload {tag:08X} changed")
            }
            Self::NativeUnavailable(tag) => write!(
                f,
                "[native-input-unavailable] Native payload {tag:08X} is unreadable"
            ),
            #[cfg(feature = "d2-model-importer")]
            Self::ImportChanged(path) => write!(
                f,
                "[import-changed] Imported input {} changed",
                path.display()
            ),
            #[cfg(feature = "d2-model-importer")]
            Self::ImportUnavailable(path) => write!(
                f,
                "[import-unavailable] Imported input {} is missing, unreadable or linked",
                path.display()
            ),
            Self::Damaged(detail) => write!(
                f,
                "[fragment-damaged] Saved fragment is unreadable or damaged: {detail}"
            ),
            Self::UnsupportedRelocation => write!(
                f,
                "[unsupported-relocation] Allocations moved and this fragment has an unsupported reference layout"
            ),
            Self::SharedAnimationChanged => write!(
                f,
                "[shared-animation-changed] Shared animation inputs or deduplication changed"
            ),
            Self::RelocationRejected => write!(
                f,
                "[relocation-rejected] Cached allocations or references cannot be safely replayed"
            ),
            #[cfg(feature = "d2-model-importer")]
            Self::UncacheableImport => write!(
                f,
                "[uncacheable-import] Imported inputs could not be safely fingerprinted"
            ),
            Self::UntrackedReads => write!(
                f,
                "[untracked-reads] Native dependency tracing was unavailable or incomplete"
            ),
            Self::UnsupportedFragment => write!(
                f,
                "[unsupported-fragment] Runtime changes cannot be represented as one complete fragment"
            ),
            Self::ParticleSymbols => write!(
                f,
                "[particle-symbols] Private perks require imported particle symbols that are not stored in runtime fragments"
            ),
            Self::TooLarge => write!(
                f,
                "[fragment-too-large] Fragment exceeds the 512 MiB entry limit"
            ),
            Self::Write(error) => {
                write!(f, "[write-failed] Could not persist the fragment: {error}")
            }
        }
    }
}

#[derive(Default)]
pub(in crate::item::build::runtime) struct Timings {
    pub load: Duration,
    pub inputs: Duration,
    pub relocation: Duration,
}

impl fmt::Display for Timings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Lookup {:.3}s, input checks {:.3}s, relocation {:.3}s",
            self.load.as_secs_f64(),
            self.inputs.as_secs_f64(),
            self.relocation.as_secs_f64()
        )
    }
}

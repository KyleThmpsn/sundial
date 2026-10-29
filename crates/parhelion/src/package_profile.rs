pub(crate) const SHADOWKEEP_HEADER_VERSION: u16 = 38;
pub(crate) const STOCK_PATCH_ID: u16 = 3;
pub(crate) const AUTHORED_PATCH_ID: u16 = 4;
pub(crate) const MIN_AUTHORED_STANDALONE_PACKAGE_ID: u16 = 0x0AA0;
pub(crate) const MAX_AUTHORED_STANDALONE_PACKAGE_ID: u16 = 0x0CFF;
pub(crate) const PARHELION_ASSET_PACKAGE_ID: u16 = 0x0AA0;
pub(crate) const PARHELION_ASSET_PATCH_ID: u16 = 0;
/// The family of Parhelion's own standalone packages. Like stock `w64_sandbox`, they hold the
/// entities, models, textures and runtime resources a build authors.
pub(crate) const SANDBOX_FAMILY: &str = "parhelion_sandbox";
pub(crate) const PARHELION_ASSET_FILE_NAME: &str = "w64_parhelion_sandbox_0aa0_0.pkg";
pub(crate) const PRIVATE_PERK_RUNTIME_PACKAGE_ID: u16 = 0x01BB;
pub(crate) const PRIVATE_PERK_RUNTIME_EXPECTED_ENTRY_COUNT: usize = 6_468;

/// The authored package ids one build uses, `base` through `top`. Its packages count from its
/// first id: asset packages, then map packages.
///
/// Without a mod loader the window ascends from `0x0AA0`, the names those installs have always
/// had, and private runtime and investment overflow reserve free IDs from its top. On Sunrise's
/// mod loader it descends from its top. Sunrise Studio
/// gives a mod's new packages the first ids no stock package holds, `0x0AA0` upward, without
/// reading other mods, and the loader refuses a mod whose package id another mod already adds, so
/// every package of a build sits at the other end. Record overflow, private runtime and imported
/// content take the highest ids still free as they need them, and asset and map packages count
/// past those. The loader window also skips ids an installed package or another mod already
/// holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Window {
    pub base: u16,
    pub top: u16,
    pub descending: bool,
}

impl Window {
    pub(crate) const FULL: Self = Self {
        base: PARHELION_ASSET_PACKAGE_ID,
        top: MAX_AUTHORED_STANDALONE_PACKAGE_ID,
        descending: false,
    };

    /// A descending window over the longest run of authored ids that `taken` leaves free, the
    /// highest of equal runs.
    pub(crate) fn avoiding(taken: &BTreeSet<u16>) -> Option<Self> {
        let mut best: Option<Self> = None;
        let mut base = None;
        for id in MIN_AUTHORED_STANDALONE_PACKAGE_ID..=MAX_AUTHORED_STANDALONE_PACKAGE_ID {
            if taken.contains(&id) {
                base = None;
                continue;
            }
            let base = *base.get_or_insert(id);
            if best.is_none_or(|best| id - base >= best.top - best.base) {
                best = Some(Self {
                    base,
                    top: id,
                    descending: true,
                });
            }
        }
        best
    }

    /// The window's first id, where its first asset package goes.
    pub(crate) fn first(self) -> u16 {
        self.nth(0)
            .unwrap_or(if self.descending { self.top } else { self.base })
    }

    /// The id `n` packages from the window's first, if the window holds it.
    pub(crate) fn nth(self, n: usize) -> Option<u16> {
        if self.descending {
            return (self.base..=self.top)
                .rev()
                .filter(|id| !crate::mod_family::build::reserved(*id))
                .nth(n);
        }
        (self.base..=self.top)
            .filter(|id| !crate::mod_family::build::reserved(*id))
            .nth(n)
    }

    /// The window without its first id, or `None` when that leaves nothing.
    pub(crate) fn rest(self) -> Option<Self> {
        (self.base < self.top).then(|| {
            if self.descending {
                Self {
                    top: self.top - 1,
                    ..self
                }
            } else {
                Self {
                    base: self.base + 1,
                    ..self
                }
            }
        })
    }
}

/// The window packed as `base << 16 | top`, with the high bit set when it descends. Authored ids
/// stop at `0x0CFF`, so the bit is free.
const DESCENDING: u32 = 1 << 31;

static WINDOW: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(
    ((PARHELION_ASSET_PACKAGE_ID as u32) << 16) | MAX_AUTHORED_STANDALONE_PACKAGE_ID as u32,
);

/// The window the build holding the [`Profile`] uses, [`Window::FULL`] unless it builds for a mod
/// loader install.
pub(crate) fn window() -> Window {
    let packed = WINDOW.load(std::sync::atomic::Ordering::Relaxed);
    Window {
        base: ((packed & !DESCENDING) >> 16) as u16,
        top: packed as u16,
        descending: packed & DESCENDING != 0,
    }
}

/// Sets the window for the build holding the [`Profile`]. The names an earlier build gave its
/// standalone packages no longer apply.
fn set_window(window: Window) {
    let direction = if window.descending { DESCENDING } else { 0 };
    WINDOW.store(
        direction | (u32::from(window.base) << 16) | u32::from(window.top),
        std::sync::atomic::Ordering::Relaxed,
    );
    STANDALONE_NAMES
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}
/// A stock chain's content generation: the content build and revision its last stock patch
/// carries, which an overlay must extend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Generation {
    pub content_build: u32,
    pub content_revision: u32,
}

/// The final Shadowkeep content generation, which most chains reached.
pub(crate) const FINAL_GENERATION: Generation = Generation {
    content_build: 0x0001_5281,
    content_revision: 2,
};
/// The generation before it, where some chains' last stock patch stayed: the HUD bank and
/// half the sandbox packages that hold ability banks.
pub(crate) const EARLIER_GENERATION: Generation = Generation {
    content_build: 0x0001_4B68,
    content_revision: 0,
};
pub(crate) const COLLECTION_PACKAGE_ID: u16 = 0x058C;
/// The HUD status table's package, and the packages of the header and the locale data of the
/// bank the stock statuses' names share.
pub(crate) const HUD_STATUS_TABLE_PACKAGE_ID: u16 = 0x02AF;
pub(crate) const HUD_STATUS_NAME_PACKAGE_ID: u16 = 0x06DC;
pub(crate) const HUD_STATUS_NAME_DATA_PACKAGE_ID: u16 = 0x03AB;
pub(crate) const HOST_PACKAGE_ID: u16 = 0x0914;
pub(crate) const HOST_EXPECTED_ENTRY_COUNT: usize = 5_452;

/// Where a build's new investment records go: item definitions and strings, icon containers and
/// weapon runtime. Dawn and Sunrise before 0.6 append them after the stock host package's
/// entries. Sunrise's mod loader refuses a patch whose entry count differs from its base, so a
/// build for it writes them into a standalone package of its own, from its first entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Host {
    pub id: u16,
    pub first: usize,
}

impl Host {
    pub(crate) const APPENDED: Self = Self {
        id: HOST_PACKAGE_ID,
        first: HOST_EXPECTED_ENTRY_COUNT,
    };

    /// A standalone package `id` that takes the records from its first entry.
    pub(crate) const fn standalone(id: u16) -> Self {
        Self { id, first: 0 }
    }

    pub(crate) const fn is_standalone(self) -> bool {
        self.id != HOST_PACKAGE_ID
    }
}

static HOST: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(HOST_PACKAGE_ID as u32);

/// The host the build holding the [`Profile`] writes, [`Host::APPENDED`] unless it builds for a
/// mod loader install.
pub(crate) fn host() -> Host {
    match HOST.load(std::sync::atomic::Ordering::Relaxed) as u16 {
        HOST_PACKAGE_ID => Host::APPENDED,
        id => Host::standalone(id),
    }
}

/// Sets the host alongside [`set_window`].
fn set_host(host: Host) {
    HOST.store(u32::from(host.id), std::sync::atomic::Ordering::Relaxed);
}

static NODES_SHARED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether another installed Sunrise mod changes the presentation nodes, for the build holding
/// the [`Profile`]. Automatic Collections placement then puts entries after a node's stock
/// entries, where Sunrise's merge can add them beside the other mod's.
pub(crate) fn presentation_nodes_shared() -> bool {
    NODES_SHARED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Sets what [`presentation_nodes_shared`] reads for the build holding the turn.
pub(crate) fn set_presentation_nodes_shared(shared: bool) {
    NODES_SHARED.store(shared, std::sync::atomic::Ordering::Relaxed);
}

/// A build's turn with the window, the host and the standalone names, which every authored id it
/// places reads. Builds in one process take turns, so a build for a mod loader install never
/// lends its layout to another, and the stock layout returns when the turn ends.
pub(crate) struct Profile {
    turn: Option<std::sync::MutexGuard<'static, ()>>,
}

static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

thread_local! {
    /// Whether this thread holds the turn, which the compile it runs then joins.
    static HOLDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Waits for any other build's turn to end, then sets `window` and `host` for this one.
pub(crate) fn hold(window: Window, host: Host) -> Profile {
    let turn = TURN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    HOLDING.set(true);
    set_window(window);
    set_host(host);
    crate::mod_family::build::begin_native();
    crate::appended_tags::clear();
    Profile { turn: Some(turn) }
}

/// This thread's turn when it holds one, otherwise a new turn with the stock layout.
pub(crate) fn join() -> Profile {
    if HOLDING.get() {
        Profile { turn: None }
    } else {
        hold(Window::FULL, Host::APPENDED)
    }
}

pub(crate) fn holding() -> bool {
    HOLDING.get()
}

impl Drop for Profile {
    fn drop(&mut self) {
        if self.turn.is_some() {
            crate::mod_family::build::clear();
            crate::appended_tags::clear();
            set_window(Window::FULL);
            set_host(Host::APPENDED);
            set_presentation_nodes_shared(false);
            HOLDING.set(false);
        }
    }
}

/// Whether `tag` names a stock item definition rather than one a build authored, in the stock
/// host after its stock entries or in a standalone host package.
pub(crate) fn is_stock_item_definition(tag: u32) -> bool {
    let tag = tiger_pkg::TagHash(tag);
    if is_authored_standalone_package_id(tag.pkg_id()) {
        return false;
    }
    tag.pkg_id() != HOST_PACKAGE_ID || usize::from(tag.entry_index()) < HOST_EXPECTED_ENTRY_COUNT
}
pub(crate) const ACCOUNT_UNLOCK_BANK: u8 = sundial::package_authoring::SHADOWKEEP_ACCOUNT_FLAG_BANK;
pub(crate) const LOCALIZATION_DONOR_TABLE_INDEX: usize = 2_927;

pub(crate) const VERSION_OFFSET: usize = 0;
pub(crate) const PACKAGE_ID_OFFSET: usize = 4;
pub(crate) const BUILD_SIGNATURE_OFFSET: usize = 8;
pub(crate) const PATCH_ID_OFFSET: usize = 0x20;
pub(crate) const PACKAGE_HEADER_PREFIX_SIZE: usize = PATCH_ID_OFFSET + size_of::<u16>();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackageHeaderPrefix {
    pub version: u16,
    pub package_id: u16,
    pub build_signature: u64,
    pub patch_id: u16,
}

impl PackageHeaderPrefix {
    pub(crate) fn parse(bytes: &[u8; PACKAGE_HEADER_PREFIX_SIZE]) -> Self {
        Self {
            version: read_u16(bytes, VERSION_OFFSET),
            package_id: read_u16(bytes, PACKAGE_ID_OFFSET),
            build_signature: read_u64(bytes, BUILD_SIGNATURE_OFFSET),
            patch_id: read_u16(bytes, PATCH_ID_OFFSET),
        }
    }

    #[cfg(test)]
    pub(crate) fn encode(self) -> [u8; PACKAGE_HEADER_PREFIX_SIZE] {
        let mut bytes = [0; PACKAGE_HEADER_PREFIX_SIZE];
        bytes[VERSION_OFFSET..VERSION_OFFSET + size_of::<u16>()]
            .copy_from_slice(&self.version.to_le_bytes());
        bytes[PACKAGE_ID_OFFSET..PACKAGE_ID_OFFSET + size_of::<u16>()]
            .copy_from_slice(&self.package_id.to_le_bytes());
        bytes[BUILD_SIGNATURE_OFFSET..BUILD_SIGNATURE_OFFSET + size_of::<u64>()]
            .copy_from_slice(&self.build_signature.to_le_bytes());
        bytes[PATCH_ID_OFFSET..PATCH_ID_OFFSET + size_of::<u16>()]
            .copy_from_slice(&self.patch_id.to_le_bytes());
        bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CanonicalPackage {
    pub package_id: u16,
    pub stock_file_stem: &'static str,
    pub stock_patch_id: u16,
    pub authored_patch_id: u16,
    pub authored_file_name: &'static str,
    /// The generation the last stock patch carries, which the authored patch extends.
    pub stock_generation: Generation,
    pub required_output: bool,
}

impl CanonicalPackage {
    pub(crate) fn stock_file_name(self, patch_id: u16) -> String {
        format!("{}_{patch_id}.pkg", self.stock_file_stem)
    }

    const fn authored(self) -> AuthoredPackage {
        AuthoredPackage {
            package_id: self.package_id,
            patch_id: self.authored_patch_id,
            file_name: self.authored_file_name,
            stock_overlay: true,
            required_output: self.required_output,
        }
    }
}

/// One package file emitted and installed by a complete Parhelion build.
///
/// The investment files extend stock patch chains. The asset package is a standalone patch
/// zero package and is deliberately kept out of [`CANONICAL_PACKAGES`], whose callers require a
/// stock source generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AuthoredPackage {
    pub package_id: u16,
    pub patch_id: u16,
    pub file_name: &'static str,
    pub stock_overlay: bool,
    pub required_output: bool,
}

pub(crate) const PARHELION_ASSET_PACKAGE: AuthoredPackage = AuthoredPackage {
    package_id: PARHELION_ASSET_PACKAGE_ID,
    patch_id: PARHELION_ASSET_PATCH_ID,
    file_name: PARHELION_ASSET_FILE_NAME,
    stock_overlay: false,
    required_output: true,
};

pub(crate) const CANONICAL_PACKAGES: [CanonicalPackage; 28] = [
    // The private perk runtime package.
    CanonicalPackage {
        package_id: PRIVATE_PERK_RUNTIME_PACKAGE_ID,
        stock_file_stem: "w64_sandbox_01bb",
        stock_patch_id: 6,
        authored_patch_id: 7,
        authored_file_name: "w64_sandbox_01bb_7.pkg",
        stock_generation: FINAL_GENERATION,
        required_output: false,
    },
    // Sandbox packages holding ability banks, replaced in place when a private perk needs a
    // property row a stock bank lacks.
    CanonicalPackage {
        package_id: 0x01C0,
        stock_file_stem: "w64_sandbox_01c0",
        stock_patch_id: 5,
        authored_patch_id: 6,
        authored_file_name: "w64_sandbox_01c0_6.pkg",
        stock_generation: EARLIER_GENERATION,
        required_output: false,
    },
    CanonicalPackage {
        package_id: 0x01D5,
        stock_file_stem: "w64_sandbox_01d5",
        stock_patch_id: 6,
        authored_patch_id: 7,
        authored_file_name: "w64_sandbox_01d5_7.pkg",
        stock_generation: FINAL_GENERATION,
        required_output: false,
    },
    // The human character parameter dictionary and named action selectors.
    CanonicalPackage {
        package_id: 0x01D9,
        stock_file_stem: "w64_sandbox_01d9",
        stock_patch_id: 6,
        authored_patch_id: 7,
        authored_file_name: "w64_sandbox_01d9_7.pkg",
        stock_generation: FINAL_GENERATION,
        required_output: false,
    },
    CanonicalPackage {
        package_id: 0x01DE,
        stock_file_stem: "w64_sandbox_01de",
        stock_patch_id: 6,
        authored_patch_id: 7,
        authored_file_name: "w64_sandbox_01de_7.pkg",
        stock_generation: FINAL_GENERATION,
        required_output: false,
    },
    CanonicalPackage {
        package_id: 0x01E0,
        stock_file_stem: "w64_sandbox_01e0",
        stock_patch_id: 5,
        authored_patch_id: 6,
        authored_file_name: "w64_sandbox_01e0_6.pkg",
        stock_generation: EARLIER_GENERATION,
        required_output: false,
    },
    CanonicalPackage {
        package_id: 0x01E1,
        stock_file_stem: "w64_sandbox_01e1",
        stock_patch_id: 6,
        authored_patch_id: 7,
        authored_file_name: "w64_sandbox_01e1_7.pkg",
        stock_generation: FINAL_GENERATION,
        required_output: false,
    },
    CanonicalPackage {
        package_id: 0x01E2,
        stock_file_stem: "w64_sandbox_01e2",
        stock_patch_id: 5,
        authored_patch_id: 6,
        authored_file_name: "w64_sandbox_01e2_6.pkg",
        stock_generation: EARLIER_GENERATION,
        required_output: false,
    },
    CanonicalPackage {
        package_id: 0x01E4,
        stock_file_stem: "w64_sandbox_01e4",
        stock_patch_id: 5,
        authored_patch_id: 6,
        authored_file_name: "w64_sandbox_01e4_6.pkg",
        stock_generation: EARLIER_GENERATION,
        required_output: false,
    },
    CanonicalPackage {
        package_id: 0x01FD,
        stock_file_stem: "w64_sandbox_01fd",
        stock_patch_id: 5,
        authored_patch_id: 6,
        authored_file_name: "w64_sandbox_01fd_6.pkg",
        stock_generation: EARLIER_GENERATION,
        required_output: false,
    },
    // The human character clip bank.
    CanonicalPackage {
        package_id: 0x0206,
        stock_file_stem: "w64_sandbox_0206",
        stock_patch_id: 5,
        authored_patch_id: 6,
        authored_file_name: "w64_sandbox_0206_6.pkg",
        stock_generation: EARLIER_GENERATION,
        required_output: false,
    },
    // First-person holding parameters for the native hand cannon animation family.
    CanonicalPackage {
        package_id: 0x0207,
        stock_file_stem: "w64_sandbox_0207",
        stock_patch_id: 6,
        authored_patch_id: 7,
        authored_file_name: "w64_sandbox_0207_7.pkg",
        stock_generation: FINAL_GENERATION,
        required_output: false,
    },
    // The human locomotion and rider motion tables and their selectors.
    CanonicalPackage {
        package_id: 0x037B,
        stock_file_stem: "w64_sandbox_037b",
        stock_patch_id: 5,
        authored_patch_id: 6,
        authored_file_name: "w64_sandbox_037b_6.pkg",
        stock_generation: EARLIER_GENERATION,
        required_output: false,
    },
    // Supplemental human pose dispatch for each bank descriptor.
    CanonicalPackage {
        package_id: 0x0934,
        stock_file_stem: "w64_sandbox_0934",
        stock_patch_id: 6,
        authored_patch_id: 7,
        authored_file_name: "w64_sandbox_0934_7.pkg",
        stock_generation: FINAL_GENERATION,
        required_output: false,
    },
    // The weapon crosshair table and one native subclass screen family.
    CanonicalPackage {
        package_id: 0x01A3,
        stock_file_stem: "w64_ui_01a3",
        stock_patch_id: 6,
        authored_patch_id: 7,
        authored_file_name: "w64_ui_01a3_7.pkg",
        stock_generation: FINAL_GENERATION,
        required_output: false,
    },
    // Private ability glyph rows and conditional tree/HUD color bindings.
    CanonicalPackage {
        package_id: 0x01E3,
        stock_file_stem: "w64_ui_01e3",
        stock_patch_id: 5,
        authored_patch_id: 6,
        authored_file_name: "w64_ui_01e3_6.pkg",
        stock_generation: EARLIER_GENERATION,
        required_output: false,
    },
    // The HUD status table, extended when a private perk shows a HUD status of its own.
    CanonicalPackage {
        package_id: HUD_STATUS_TABLE_PACKAGE_ID,
        stock_file_stem: "w64_ui_02af",
        stock_patch_id: 5,
        authored_patch_id: 6,
        authored_file_name: "w64_ui_02af_6.pkg",
        stock_generation: EARLIER_GENERATION,
        required_output: false,
    },
    CanonicalPackage {
        package_id: 0x0361,
        stock_file_stem: "w64_investment_0361",
        stock_patch_id: 6,
        authored_patch_id: 7,
        authored_file_name: "w64_investment_0361_7.pkg",
        stock_generation: FINAL_GENERATION,
        required_output: true,
    },
    CanonicalPackage {
        package_id: 0x0374,
        stock_file_stem: "w64_shared_manifest_0374",
        stock_patch_id: 6,
        authored_patch_id: 7,
        authored_file_name: "w64_shared_manifest_0374_7.pkg",
        stock_generation: FINAL_GENERATION,
        required_output: false,
    },
    // The companion hierarchy for the other native subclass screen family.
    CanonicalPackage {
        package_id: 0x037E,
        stock_file_stem: "w64_ui_037e",
        stock_patch_id: 5,
        authored_patch_id: 6,
        authored_file_name: "w64_ui_037e_6.pkg",
        stock_generation: EARLIER_GENERATION,
        required_output: false,
    },
    // The HUD status names' locale data.
    CanonicalPackage {
        package_id: HUD_STATUS_NAME_DATA_PACKAGE_ID,
        stock_file_stem: "w64_globals_03ab",
        stock_patch_id: 6,
        authored_patch_id: 7,
        authored_file_name: "w64_globals_03ab_7.pkg",
        stock_generation: FINAL_GENERATION,
        required_output: false,
    },
    CanonicalPackage {
        package_id: 0x0397,
        stock_file_stem: "w64_sandbox_0397",
        stock_patch_id: 5,
        authored_patch_id: 6,
        authored_file_name: "w64_sandbox_0397_6.pkg",
        stock_generation: EARLIER_GENERATION,
        required_output: false,
    },
    // The HUD status names, extended with each HUD status of the project's own.
    CanonicalPackage {
        package_id: HUD_STATUS_NAME_PACKAGE_ID,
        stock_file_stem: "w64_globals_06dc",
        stock_patch_id: 6,
        authored_patch_id: 7,
        authored_file_name: "w64_globals_06dc_7.pkg",
        stock_generation: FINAL_GENERATION,
        required_output: false,
    },
    CanonicalPackage {
        package_id: 0x058C,
        stock_file_stem: "w64_investment_globals_client_058c",
        stock_patch_id: STOCK_PATCH_ID,
        authored_patch_id: AUTHORED_PATCH_ID,
        authored_file_name: "w64_investment_globals_client_058c_4.pkg",
        stock_generation: FINAL_GENERATION,
        required_output: true,
    },
    CanonicalPackage {
        package_id: 0x0593,
        stock_file_stem: "w64_investment_globals_client_0593",
        stock_patch_id: STOCK_PATCH_ID,
        authored_patch_id: AUTHORED_PATCH_ID,
        authored_file_name: "w64_investment_globals_client_0593_4.pkg",
        stock_generation: FINAL_GENERATION,
        required_output: true,
    },
    CanonicalPackage {
        package_id: 0x0709,
        stock_file_stem: "w64_investment_globals_client_0709",
        stock_patch_id: STOCK_PATCH_ID,
        authored_patch_id: AUTHORED_PATCH_ID,
        authored_file_name: "w64_investment_globals_client_0709_4.pkg",
        stock_generation: FINAL_GENERATION,
        required_output: true,
    },
    CanonicalPackage {
        package_id: 0x0913,
        stock_file_stem: "w64_investment_globals_client_0913",
        stock_patch_id: STOCK_PATCH_ID,
        authored_patch_id: AUTHORED_PATCH_ID,
        authored_file_name: "w64_investment_globals_client_0913_4.pkg",
        stock_generation: FINAL_GENERATION,
        required_output: true,
    },
    CanonicalPackage {
        package_id: 0x0914,
        stock_file_stem: "w64_investment_globals_client_0914",
        stock_patch_id: STOCK_PATCH_ID,
        authored_patch_id: AUTHORED_PATCH_ID,
        authored_file_name: "w64_investment_globals_client_0914_4.pkg",
        stock_generation: FINAL_GENERATION,
        required_output: true,
    },
];

/// Dependency order: the standalone resource package is installed before overlays that root it.
pub(crate) const AUTHORED_PACKAGES: [AuthoredPackage; CANONICAL_PACKAGES.len() + 1] =
    authored_packages();

/// Sorted output names used by staged-manifest validation.
#[cfg(test)]
pub(crate) const CANONICAL_ARTIFACT_FILE_NAMES: [&str; CANONICAL_PACKAGES.len() + 1] =
    canonical_artifact_file_names();
pub(crate) const CANONICAL_PACKAGE_IDS: [u16; CANONICAL_PACKAGES.len()] = canonical_package_ids();

pub(crate) const fn is_authored_standalone_package_id(package_id: u16) -> bool {
    package_id >= MIN_AUTHORED_STANDALONE_PACKAGE_ID
        && package_id <= MAX_AUTHORED_STANDALONE_PACKAGE_ID
}

pub(crate) fn canonical_package(package_id: u16) -> Option<CanonicalPackage> {
    CANONICAL_PACKAGES
        .iter()
        .copied()
        .find(|profile| profile.package_id == package_id)
}

pub(crate) fn authored_package(package_id: u16) -> Option<AuthoredPackage> {
    if is_authored_standalone_package_id(package_id) {
        return Some(asset_package(package_id));
    }
    AUTHORED_PACKAGES
        .iter()
        .copied()
        .find(|profile| profile.package_id == package_id)
}

/// The profile of authored package file `file_name`. A standalone package may carry any family
/// in its name, as a map's packages carry the map's, so every `w64_<family>_<id>_0.pkg` with an
/// authored id is one. Installation checks the header of each such file for Parhelion's build
/// signature before it adopts, replaces or retires it.
pub(crate) fn authored_package_for_file_name(file_name: &str) -> Option<AuthoredPackage> {
    if let Some(id) = standalone_id(file_name) {
        return Some(standalone_package(id, intern(file_name)));
    }
    AUTHORED_PACKAGES
        .iter()
        .copied()
        .find(|profile| profile.file_name == file_name)
}

/// The id of standalone package file `file_name`: `w64_<family>_<id>_0.pkg` with a family of
/// lowercase letters, digits and underscores, and an authored id in four lowercase hex digits.
fn standalone_id(file_name: &str) -> Option<u16> {
    let stem = file_name.strip_prefix("w64_")?.strip_suffix("_0.pkg")?;
    let (family, encoded) = stem.rsplit_once('_')?;
    let family_valid = !family.is_empty()
        && family
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    let encoded_valid = encoded.len() == 4
        && encoded
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !family_valid || !encoded_valid {
        return None;
    }
    u16::from_str_radix(encoded, 16)
        .ok()
        .filter(|&id| is_authored_standalone_package_id(id))
}

/// The family of the package holding a build's investment records on Sunrise's mod loader, which
/// refuses a patch that appends them to a stock package as Dawn and earlier Sunrise builds do.
pub(crate) const RECORDS_FAMILY: &str = "parhelion_investment";

/// The managed records family is also an installation-layout marker. Legacy overflow uses a
/// different family so a valid appended build is not mistaken for a mod-loader-only build.
pub(crate) const LEGACY_RECORDS_FAMILY: &str = "parhelion_records";

/// Whether `file_name` is a build's records package, which only builds for the mod loader have.
pub(crate) fn is_records_package(file_name: &str) -> bool {
    standalone_id(file_name).is_some()
        && file_name
            .strip_prefix("w64_")
            .and_then(|rest| rest.strip_prefix(RECORDS_FAMILY))
            .and_then(|rest| rest.strip_prefix('_'))
            .and_then(|rest| rest.strip_suffix("_0.pkg"))
            .is_some_and(|id| id.len() == 4)
}

/// Standalone packages the current build names after something other than Parhelion's assets,
/// by id. Each map's packages are named after the map, as stock maps' are:
/// `w64_<map>_<id>_0.pkg` holds its world and `w64_<map>_activities_<id>_0.pkg` its activity.
static STANDALONE_NAMES: std::sync::RwLock<BTreeMap<u16, &'static str>> =
    std::sync::RwLock::new(BTreeMap::new());

/// Names standalone package `id` `w64_<family>_<id>_0.pkg` for the current build.
pub(crate) fn name_standalone_package(id: u16, family: &str) -> Result<(), String> {
    let name = format!("w64_{family}_{id:04x}_0.pkg");
    if standalone_id(&name) != Some(id) {
        return Err(format!(
            "{name:?} is not a standalone package name in the authored window"
        ));
    }
    STANDALONE_NAMES
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(id, intern(&name));
    Ok(())
}

/// One lasting copy of each standalone file name, which profiles borrow.
fn intern(name: &str) -> &'static str {
    static NAMES: std::sync::Mutex<BTreeSet<&'static str>> = std::sync::Mutex::new(BTreeSet::new());
    let mut names = NAMES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(&name) = names.get(name) {
        return name;
    }
    let name: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.insert(name);
    name
}

/// Resolves a recipe-selected artifact set into deterministic installation order: standalone
/// packages by id, then the stock overlays in profile order.
///
/// Every core package must be present, optional runtime hosts may be present when the compiler
/// needs them, and no unrecognized package may enter the install transaction. Each name resolves
/// itself, so a map's packages resolve in any process, not only in the build that named them.
pub(crate) fn authored_packages_for_file_names<'a>(
    file_names: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<AuthoredPackage>, String> {
    let mut selected = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut profiles = Vec::new();
    let mut assets = false;
    for file_name in file_names {
        let Some(profile) = authored_package_for_file_name(file_name) else {
            return Err(format!("Unrecognized authored package {file_name:?}"));
        };
        if !selected.insert(profile.file_name) {
            return Err(format!("Duplicate authored package {file_name:?}"));
        }
        if !ids.insert(profile.package_id) {
            return Err(format!(
                "Authored package {file_name:?} shares package id {:04X} with another",
                profile.package_id
            ));
        }
        assets |= is_authored_standalone_package_id(profile.package_id);
        profiles.push(profile);
    }
    // The first asset package sits where the build's window begins, which is not always 0AA0.
    let missing = AUTHORED_PACKAGES
        .iter()
        .filter(|profile| profile.required_output && !selected.contains(profile.file_name))
        .filter(|profile| !(assets && is_authored_standalone_package_id(profile.package_id)))
        .map(|profile| profile.file_name)
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "Missing required authored packages: {}",
            missing.join(", ")
        ));
    }
    profiles.sort_by_key(|profile| {
        (
            !is_authored_standalone_package_id(profile.package_id),
            CANONICAL_PACKAGES
                .iter()
                .position(|canonical| canonical.package_id == profile.package_id),
            profile.package_id,
        )
    });
    Ok(profiles)
}

fn standalone_package(package_id: u16, file_name: &'static str) -> AuthoredPackage {
    AuthoredPackage {
        package_id,
        patch_id: 0,
        file_name,
        stock_overlay: false,
        required_output: package_id == PARHELION_ASSET_PACKAGE_ID,
    }
}

/// Standalone package `package_id` under Parhelion's sandbox name.
fn default_asset_package(package_id: u16) -> AuthoredPackage {
    static NAMES: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(|| {
        (MIN_AUTHORED_STANDALONE_PACKAGE_ID..=MAX_AUTHORED_STANDALONE_PACKAGE_ID)
            .map(|id| format!("w64_{SANDBOX_FAMILY}_{id:04x}_0.pkg"))
            .collect()
    });
    standalone_package(
        package_id,
        &NAMES[usize::from(package_id - MIN_AUTHORED_STANDALONE_PACKAGE_ID)],
    )
}

/// Standalone package `package_id` under the name the current build gives it.
fn asset_package(package_id: u16) -> AuthoredPackage {
    let named = STANDALONE_NAMES
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&package_id)
        .copied();
    named.map_or_else(
        || default_asset_package(package_id),
        |name| standalone_package(package_id, name),
    )
}

/// Includes optional spill packages for ownership checks, retirement, and recovery, under
/// Parhelion's sandbox names and the names the current build gives them.
pub(crate) fn all_authored_packages() -> impl Iterator<Item = AuthoredPackage> {
    let named = STANDALONE_NAMES
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .map(|(&id, &name)| standalone_package(id, name))
        .collect::<Vec<_>>();
    (MIN_AUTHORED_STANDALONE_PACKAGE_ID..=MAX_AUTHORED_STANDALONE_PACKAGE_ID)
        .map(default_asset_package)
        .chain(named)
        .chain(
            CANONICAL_PACKAGES
                .into_iter()
                .map(CanonicalPackage::authored),
        )
}

/// [`all_authored_packages`] and every standalone package file in `directories`, such as a map's
/// package an earlier build named after its map.
pub(crate) fn all_authored_packages_in(directories: &[&std::path::Path]) -> Vec<AuthoredPackage> {
    let mut packages = all_authored_packages().collect::<Vec<_>>();
    for directory in directories {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if let Some(profile) = name.to_str().and_then(authored_package_for_file_name)
                && standalone_id(profile.file_name).is_some()
                && !packages
                    .iter()
                    .any(|known| known.file_name == profile.file_name)
            {
                packages.push(profile);
            }
        }
    }
    packages
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        bytes[offset..offset + size_of::<u16>()]
            .try_into()
            .expect("fixed package header field is in bounds"),
    )
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + size_of::<u64>()]
            .try_into()
            .expect("fixed package header field is in bounds"),
    )
}

const fn authored_packages() -> [AuthoredPackage; CANONICAL_PACKAGES.len() + 1] {
    let mut packages = [PARHELION_ASSET_PACKAGE; CANONICAL_PACKAGES.len() + 1];
    let mut index = 0;
    while index < CANONICAL_PACKAGES.len() {
        packages[index + 1] = CANONICAL_PACKAGES[index].authored();
        index += 1;
    }
    packages
}

#[cfg(test)]
const fn canonical_artifact_file_names() -> [&'static str; CANONICAL_PACKAGES.len() + 1] {
    let mut names = [""; CANONICAL_PACKAGES.len() + 1];
    let mut index = 0;
    while index < CANONICAL_PACKAGES.len() {
        names[index] = CANONICAL_PACKAGES[index].authored_file_name;
        index += 1;
    }
    names[CANONICAL_PACKAGES.len()] = PARHELION_ASSET_FILE_NAME;
    names
}

const fn canonical_package_ids() -> [u16; CANONICAL_PACKAGES.len()] {
    let mut ids = [0; CANONICAL_PACKAGES.len()];
    let mut index = 0;
    while index < CANONICAL_PACKAGES.len() {
        ids[index] = CANONICAL_PACKAGES[index].package_id;
        index += 1;
    }
    ids
}
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_artifact_selection_requires_core_and_accepts_optional_runtime_hosts() {
        let required = [
            "w64_parhelion_sandbox_0aa0_0.pkg",
            "w64_investment_0361_7.pkg",
            "w64_investment_globals_client_058c_4.pkg",
            "w64_investment_globals_client_0593_4.pkg",
            "w64_investment_globals_client_0709_4.pkg",
            "w64_investment_globals_client_0913_4.pkg",
            "w64_investment_globals_client_0914_4.pkg",
        ];
        let selected = authored_packages_for_file_names(required).unwrap();
        assert_eq!(
            selected
                .iter()
                .map(|profile| profile.file_name)
                .collect::<BTreeSet<_>>(),
            required.into_iter().collect()
        );
        for missing in required {
            assert!(
                authored_packages_for_file_names(
                    required.into_iter().filter(|name| *name != missing)
                )
                .is_err()
            );
        }

        let all = authored_packages_for_file_names(CANONICAL_ARTIFACT_FILE_NAMES).unwrap();
        assert_eq!(
            all.iter()
                .map(|profile| profile.file_name)
                .collect::<BTreeSet<_>>(),
            AUTHORED_PACKAGES
                .iter()
                .map(|profile| profile.file_name)
                .collect()
        );
        assert!(
            all.iter()
                .any(|profile| profile.package_id == PRIVATE_PERK_RUNTIME_PACKAGE_ID)
        );
    }

    #[test]
    fn spill_profiles_are_optional_canonical_and_installed_before_roots() {
        let spill = authored_package(0x0AA1).unwrap();
        assert_eq!(spill.file_name, "w64_parhelion_sandbox_0aa1_0.pkg");
        assert!(!spill.required_output);
        assert!(!spill.stock_overlay);
        let mut names = CANONICAL_ARTIFACT_FILE_NAMES.to_vec();
        names.push(spill.file_name);
        let selected = authored_packages_for_file_names(names).unwrap();
        let first_root = selected
            .iter()
            .position(|profile| profile.stock_overlay)
            .unwrap();
        for resource in [PARHELION_ASSET_PACKAGE, spill] {
            assert!(
                selected
                    .iter()
                    .position(|profile| *profile == resource)
                    .unwrap()
                    < first_root
            );
        }
        assert!(
            selected[first_root..]
                .iter()
                .all(|profile| profile.stock_overlay)
        );
        for name in [
            "w64_parhelion_sandbox_0AA1_0.pkg",
            "w64_parhelion_sandbox_0aa1_1.pkg",
            "w64_parhelion_sandbox_0d00_0.pkg",
        ] {
            assert!(authored_package_for_file_name(name).is_none());
        }
    }

    /// Round-tripping through `encode` proved nothing on its own: both sides read the same four
    /// constants, so a wrong offset moved the writer and the reader together and the test still
    /// passed. The offsets are the game's, so they are spelled out here as literals instead.
    #[test]
    fn a_package_header_prefix_reads_each_field_from_its_own_offset() {
        let mut bytes = [0_u8; PACKAGE_HEADER_PREFIX_SIZE];
        bytes[0..2].copy_from_slice(&SHADOWKEEP_HEADER_VERSION.to_le_bytes());
        bytes[4..6].copy_from_slice(&0x0914_u16.to_le_bytes());
        bytes[8..16].copy_from_slice(&0x1234_5678_9ABC_DEF0_u64.to_le_bytes());
        bytes[0x20..0x22].copy_from_slice(&AUTHORED_PATCH_ID.to_le_bytes());
        let header = PackageHeaderPrefix {
            version: SHADOWKEEP_HEADER_VERSION,
            package_id: 0x0914,
            build_signature: 0x1234_5678_9ABC_DEF0,
            patch_id: AUTHORED_PATCH_ID,
        };

        assert_eq!(PackageHeaderPrefix::parse(&bytes), header);
        // `encode` is the inverse, and is what the install and workflow fixtures build headers
        // with, so it is pinned to the same literal bytes rather than to its own reader.
        assert_eq!(header.encode(), bytes);
    }
}

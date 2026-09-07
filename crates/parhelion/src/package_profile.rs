pub(crate) const SHADOWKEEP_HEADER_VERSION: u16 = 38;
pub(crate) const STOCK_PATCH_ID: u16 = 3;
pub(crate) const AUTHORED_PATCH_ID: u16 = 4;
pub(crate) const MIN_AUTHORED_STANDALONE_PACKAGE_ID: u16 = 0x0AA0;
pub(crate) const MAX_AUTHORED_STANDALONE_PACKAGE_ID: u16 = 0x0CFF;
pub(crate) const PARHELION_ASSET_PACKAGE_ID: u16 = 0x0AA0;
pub(crate) const PARHELION_ASSET_PATCH_ID: u16 = 0;
pub(crate) const PARHELION_ASSET_FILE_NAME: &str = "w64_parhelion_assets_0aa0_0.pkg";
pub(crate) const PRIVATE_PERK_RUNTIME_PACKAGE_ID: u16 = 0x01BB;
pub(crate) const PRIVATE_PERK_RUNTIME_EXPECTED_ENTRY_COUNT: usize = 6_468;
pub(crate) const COLLECTION_PACKAGE_ID: u16 = 0x058C;
pub(crate) const HOST_PACKAGE_ID: u16 = 0x0914;
pub(crate) const HOST_EXPECTED_ENTRY_COUNT: usize = 5_452;
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

pub(crate) const CANONICAL_PACKAGES: [CanonicalPackage; 9] = [
    CanonicalPackage {
        package_id: PRIVATE_PERK_RUNTIME_PACKAGE_ID,
        stock_file_stem: "w64_sandbox_01bb",
        stock_patch_id: 6,
        authored_patch_id: 7,
        authored_file_name: "w64_sandbox_01bb_7.pkg",
        required_output: false,
    },
    CanonicalPackage {
        package_id: 0x0361,
        stock_file_stem: "w64_investment_0361",
        stock_patch_id: 6,
        authored_patch_id: 7,
        authored_file_name: "w64_investment_0361_7.pkg",
        required_output: true,
    },
    CanonicalPackage {
        package_id: 0x0374,
        stock_file_stem: "w64_shared_manifest_0374",
        stock_patch_id: 6,
        authored_patch_id: 7,
        authored_file_name: "w64_shared_manifest_0374_7.pkg",
        required_output: false,
    },
    CanonicalPackage {
        package_id: 0x037E,
        stock_file_stem: "w64_ui_037e",
        stock_patch_id: 5,
        authored_patch_id: 6,
        authored_file_name: "w64_ui_037e_6.pkg",
        required_output: false,
    },
    CanonicalPackage {
        package_id: 0x058C,
        stock_file_stem: "w64_investment_globals_client_058c",
        stock_patch_id: STOCK_PATCH_ID,
        authored_patch_id: AUTHORED_PATCH_ID,
        authored_file_name: "w64_investment_globals_client_058c_4.pkg",
        required_output: true,
    },
    CanonicalPackage {
        package_id: 0x0593,
        stock_file_stem: "w64_investment_globals_client_0593",
        stock_patch_id: STOCK_PATCH_ID,
        authored_patch_id: AUTHORED_PATCH_ID,
        authored_file_name: "w64_investment_globals_client_0593_4.pkg",
        required_output: true,
    },
    CanonicalPackage {
        package_id: 0x0709,
        stock_file_stem: "w64_investment_globals_client_0709",
        stock_patch_id: STOCK_PATCH_ID,
        authored_patch_id: AUTHORED_PATCH_ID,
        authored_file_name: "w64_investment_globals_client_0709_4.pkg",
        required_output: true,
    },
    CanonicalPackage {
        package_id: 0x0913,
        stock_file_stem: "w64_investment_globals_client_0913",
        stock_patch_id: STOCK_PATCH_ID,
        authored_patch_id: AUTHORED_PATCH_ID,
        authored_file_name: "w64_investment_globals_client_0913_4.pkg",
        required_output: true,
    },
    CanonicalPackage {
        package_id: 0x0914,
        stock_file_stem: "w64_investment_globals_client_0914",
        stock_patch_id: STOCK_PATCH_ID,
        authored_patch_id: AUTHORED_PATCH_ID,
        authored_file_name: "w64_investment_globals_client_0914_4.pkg",
        required_output: true,
    },
];

/// Dependency order: the standalone resource package is installed before overlays that root it.
pub(crate) const AUTHORED_PACKAGES: [AuthoredPackage; CANONICAL_PACKAGES.len() + 1] =
    authored_packages();

/// Sorted output names used by staged-manifest validation.
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
    AUTHORED_PACKAGES
        .iter()
        .copied()
        .find(|profile| profile.package_id == package_id)
}

pub(crate) fn authored_package_for_file_name(file_name: &str) -> Option<AuthoredPackage> {
    AUTHORED_PACKAGES
        .iter()
        .copied()
        .find(|profile| profile.file_name == file_name)
}

/// Resolves a recipe-selected artifact set into deterministic installation order.
///
/// Every core package must be present, optional runtime hosts may be present when the compiler
/// needs them, and no unrecognized package may enter the install transaction.
pub(crate) fn authored_packages_for_file_names<'a>(
    file_names: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<AuthoredPackage>, String> {
    let mut selected = BTreeSet::new();
    for file_name in file_names {
        let Some(profile) = authored_package_for_file_name(file_name) else {
            return Err(format!("Unrecognized authored package {file_name:?}"));
        };
        if !selected.insert(profile.file_name) {
            return Err(format!("Duplicate authored package {file_name:?}"));
        }
    }
    let missing = AUTHORED_PACKAGES
        .iter()
        .filter(|profile| profile.required_output && !selected.contains(profile.file_name))
        .map(|profile| profile.file_name)
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "Missing required authored packages: {}",
            missing.join(", ")
        ));
    }
    Ok(AUTHORED_PACKAGES
        .iter()
        .copied()
        .filter(|profile| selected.contains(profile.file_name))
        .collect())
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
use std::collections::BTreeSet;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_arrays_are_derived_from_the_profile() {
        assert!(is_authored_standalone_package_id(
            PARHELION_ASSET_PACKAGE_ID
        ));
        assert_eq!(
            CANONICAL_PACKAGE_IDS,
            CANONICAL_PACKAGES.map(|profile| profile.package_id)
        );
        assert_eq!(
            &CANONICAL_ARTIFACT_FILE_NAMES[..CANONICAL_PACKAGES.len()],
            &CANONICAL_PACKAGES.map(|profile| profile.authored_file_name)
        );
        assert_eq!(
            CANONICAL_ARTIFACT_FILE_NAMES[CANONICAL_PACKAGES.len()],
            PARHELION_ASSET_FILE_NAME
        );
    }

    #[test]
    fn authored_package_profiles_are_coherent() {
        assert_eq!(AUTHORED_PACKAGES[0], PARHELION_ASSET_PACKAGE);
        assert_eq!(AUTHORED_PACKAGES.len(), 10);
        assert_eq!(
            AUTHORED_PACKAGES
                .iter()
                .filter(|profile| profile.required_output)
                .count(),
            7
        );
        assert_eq!(PARHELION_ASSET_PACKAGE.patch_id, 0);
        assert_eq!(
            AUTHORED_PACKAGES
                .iter()
                .filter(|profile| profile.stock_overlay)
                .count(),
            AUTHORED_PACKAGES.len() - 1
        );
        assert!(
            AUTHORED_PACKAGES[1..]
                .iter()
                .all(|profile| profile.stock_overlay)
        );
        assert_eq!(
            AUTHORED_PACKAGES
                .iter()
                .map(|profile| profile.package_id)
                .collect::<BTreeSet<_>>()
                .len(),
            AUTHORED_PACKAGES.len()
        );
        assert_eq!(
            AUTHORED_PACKAGES
                .iter()
                .map(|profile| profile.file_name)
                .collect::<BTreeSet<_>>()
                .len(),
            AUTHORED_PACKAGES.len()
        );
        for profile in CANONICAL_PACKAGES {
            assert_eq!(
                profile.authored_file_name,
                profile.stock_file_name(profile.authored_patch_id)
            );
        }
        for profile in AUTHORED_PACKAGES {
            assert_eq!(authored_package(profile.package_id), Some(profile));
            assert_eq!(
                authored_package_for_file_name(profile.file_name),
                Some(profile)
            );
        }
    }

    #[test]
    fn authored_artifact_selection_requires_core_and_accepts_optional_runtime_hosts() {
        let required = AUTHORED_PACKAGES
            .iter()
            .filter(|profile| profile.required_output)
            .map(|profile| profile.file_name)
            .collect::<Vec<_>>();
        let selected = authored_packages_for_file_names(required.iter().copied()).unwrap();
        assert_eq!(selected.len(), 7);
        assert!(selected.iter().all(|profile| profile.required_output));

        let all = authored_packages_for_file_names(CANONICAL_ARTIFACT_FILE_NAMES).unwrap();
        assert_eq!(all, AUTHORED_PACKAGES);
        assert!(
            all.iter()
                .any(|profile| profile.package_id == PRIVATE_PERK_RUNTIME_PACKAGE_ID)
        );
    }

    #[test]
    fn package_header_prefix_round_trips() {
        let header = PackageHeaderPrefix {
            version: SHADOWKEEP_HEADER_VERSION,
            package_id: 0x0914,
            build_signature: 0x1234_5678_9ABC_DEF0,
            patch_id: AUTHORED_PATCH_ID,
        };
        assert_eq!(PackageHeaderPrefix::parse(&header.encode()), header);
    }
}

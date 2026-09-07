use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};

use crate::package_runtime::sunrise_module_path;

const RUNTIME_STATE_FILE_NAME: &str = "runtime-state.bin";
const RUNTIME_STATE_MAGIC: u32 = 0x5352_5354;
const RUNTIME_STATE_MODULE_MARKER: &[u8] = b"r\0u\0n\0t\0i\0m\0e\0-\0s\0t\0a\0t\0e\0.\0b\0i\0n\0";

pub(super) const WARNING_MESSAGE: &str = "This Sunrise installation appears to use the separate runtime-state.bin persistence found in Sunrise AIO Karisma. It can override account data loaded from settings.json, so changes made in Sundial may not appear in game.";

#[derive(Clone, Debug, PartialEq, Eq)]
enum Evidence {
    Match,
    Missing,
    NoMatch,
    Inaccessible(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PersistenceCompatibility {
    runtime_state_path: PathBuf,
    runtime_state_evidence: Evidence,
    runtime_state_version: Option<u32>,
    module_path: PathBuf,
    module_evidence: Evidence,
}

impl PersistenceCompatibility {
    pub(super) fn inspect(install: &Path) -> Self {
        let runtime_state_path = install
            .join("bin")
            .join("x64")
            .join("Sunrise")
            .join(RUNTIME_STATE_FILE_NAME);
        let (runtime_state_evidence, runtime_state_version) =
            inspect_runtime_state_file(&runtime_state_path);
        let module_path = sunrise_module_path(install);
        let module_evidence = inspect_sunrise_module(&module_path);
        Self {
            runtime_state_path,
            runtime_state_evidence,
            runtime_state_version,
            module_path,
            module_evidence,
        }
    }

    pub(super) fn detected(&self) -> bool {
        self.runtime_state_evidence == Evidence::Match || self.module_evidence == Evidence::Match
    }

    pub(super) fn detection_evidence(&self) -> &'static str {
        match (
            self.runtime_state_evidence == Evidence::Match,
            self.module_evidence == Evidence::Match,
        ) {
            (true, true) => "runtime_state_header_and_sunrise_module_marker",
            (true, false) => "runtime_state_header",
            (false, true) => "sunrise_module_marker",
            (false, false) => "none",
        }
    }

    pub(super) fn runtime_state_path(&self) -> &Path {
        &self.runtime_state_path
    }

    pub(super) fn runtime_state_status(&self) -> String {
        match &self.runtime_state_evidence {
            Evidence::Match => format!(
                "recognized | format_version={}",
                self.runtime_state_version
                    .map_or_else(|| "unavailable".to_owned(), |version| version.to_string())
            ),
            Evidence::Missing => "missing".to_owned(),
            Evidence::NoMatch => "present_but_unrecognized".to_owned(),
            Evidence::Inaccessible(error) => {
                format!("inaccessible | error={}", single_line(error))
            }
        }
    }

    pub(super) fn module_path(&self) -> &Path {
        &self.module_path
    }

    pub(super) fn module_status(&self) -> String {
        match &self.module_evidence {
            Evidence::Match => "runtime_state_filename_marker_present".to_owned(),
            Evidence::Missing => "missing".to_owned(),
            Evidence::NoMatch => "runtime_state_filename_marker_absent".to_owned(),
            Evidence::Inaccessible(error) => {
                format!("inaccessible | error={}", single_line(error))
            }
        }
    }
}

fn inspect_runtime_state_file(path: &Path) -> (Evidence, Option<u32>) {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return (Evidence::Missing, None);
        }
        Err(error) => return (Evidence::Inaccessible(error.to_string()), None),
    };
    let mut header = [0u8; 8];
    if let Err(error) = file.read_exact(&mut header) {
        return if error.kind() == io::ErrorKind::UnexpectedEof {
            (Evidence::NoMatch, None)
        } else {
            (Evidence::Inaccessible(error.to_string()), None)
        };
    }
    let magic = u32::from_le_bytes(header[..4].try_into().expect("four-byte magic"));
    let version = u32::from_le_bytes(header[4..].try_into().expect("four-byte version"));
    if magic == RUNTIME_STATE_MAGIC {
        (Evidence::Match, Some(version))
    } else {
        (Evidence::NoMatch, None)
    }
}

fn inspect_sunrise_module(path: &Path) -> Evidence {
    match fs::read(path) {
        Ok(bytes) if contains_bytes(&bytes, RUNTIME_STATE_MODULE_MARKER) => Evidence::Match,
        Ok(_) => Evidence::NoMatch,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Evidence::Missing,
        Err(error) => Evidence::Inaccessible(error.to_string()),
    }
}

fn contains_bytes(bytes: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && bytes.windows(needle.len()).any(|window| window == needle)
}

fn single_line(value: &str) -> &str {
    value.split(['\r', '\n']).next().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDirectory;

    #[test]
    fn recognizes_the_runtime_state_header() {
        let directory = TestDirectory::new("karisma-runtime-state-header");
        let runtime_directory = directory.0.join("bin").join("x64").join("Sunrise");
        fs::create_dir_all(&runtime_directory).unwrap();
        let mut header = Vec::from(RUNTIME_STATE_MAGIC.to_le_bytes());
        header.extend_from_slice(&7u32.to_le_bytes());
        fs::write(runtime_directory.join(RUNTIME_STATE_FILE_NAME), header).unwrap();

        let inspection = PersistenceCompatibility::inspect(&directory.0);

        assert!(inspection.detected());
        assert_eq!(inspection.detection_evidence(), "runtime_state_header");
        assert_eq!(
            inspection.runtime_state_status(),
            "recognized | format_version=7"
        );
    }

    #[test]
    fn recognizes_the_installed_module_before_a_runtime_state_file_exists() {
        let directory = TestDirectory::new("karisma-runtime-module-marker");
        let module = sunrise_module_path(&directory.0);
        fs::create_dir_all(module.parent().unwrap()).unwrap();
        let mut bytes = b"unrelated-prefix".to_vec();
        bytes.extend_from_slice(RUNTIME_STATE_MODULE_MARKER);
        fs::write(module, bytes).unwrap();

        let inspection = PersistenceCompatibility::inspect(&directory.0);

        assert!(inspection.detected());
        assert_eq!(inspection.detection_evidence(), "sunrise_module_marker");
        assert_eq!(
            inspection.module_status(),
            "runtime_state_filename_marker_present"
        );
    }

    #[test]
    fn ignores_unrecognized_files_and_ascii_only_module_text() {
        let directory = TestDirectory::new("karisma-runtime-no-match");
        let runtime_directory = directory.0.join("bin").join("x64").join("Sunrise");
        fs::create_dir_all(&runtime_directory).unwrap();
        fs::write(
            runtime_directory.join(RUNTIME_STATE_FILE_NAME),
            b"not a Sunrise runtime state",
        )
        .unwrap();
        let module = sunrise_module_path(&directory.0);
        fs::create_dir_all(module.parent().unwrap()).unwrap();
        fs::write(module, b"runtime-state.bin").unwrap();

        let inspection = PersistenceCompatibility::inspect(&directory.0);

        assert!(!inspection.detected());
        assert_eq!(inspection.detection_evidence(), "none");
        assert_eq!(
            inspection.runtime_state_status(),
            "present_but_unrecognized"
        );
        assert_eq!(
            inspection.module_status(),
            "runtime_state_filename_marker_absent"
        );
    }
}

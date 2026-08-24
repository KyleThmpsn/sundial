use std::path::PathBuf;

use super::settings;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum GeneratedFileSaveAction {
    Save,
    SaveAndExit,
    ResetDefaults,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum GeneratedFileDecision {
    Ask,
    Replace,
    KeepExisting,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum GeneratedFileKind {
    OrbitMap,
}

impl GeneratedFileKind {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::OrbitMap => "Orbit map",
        }
    }

    pub(super) const fn file_name(self) -> &'static str {
        match self {
            Self::OrbitMap => "orbit_map.txt",
        }
    }
}

pub(super) struct PendingGeneratedFile {
    pub(super) kind: GeneratedFileKind,
    pub(super) path: PathBuf,
    pub(super) existing: String,
    pub(super) generated: String,
    pub(super) diff: String,
    pub(super) action: GeneratedFileSaveAction,
}

pub(super) enum GeneratedFilePlan {
    Current(GeneratedFileKind, PathBuf),
    Write(GeneratedFileKind, String),
    KeepExisting(GeneratedFileKind, PathBuf),
}

pub(super) fn settings_size_note(result: &settings::SaveJsonResult) -> String {
    let limit = settings_size_label(result.size_limit_bytes);
    if result.exceeds_size_limit {
        format!(
            " Warning: the compacted file is {} bytes, above this Sunrise schema's {limit} settings limit, and may not load.",
            result.encoded_bytes,
        )
    } else if result.compacted {
        format!(
            " Sunrise-style formatting exceeded this schema's {limit} limit, so Sundial compacted the file to {} bytes.",
            result.encoded_bytes,
        )
    } else {
        String::new()
    }
}

pub(super) fn normalized_generated_document(document: &str) -> String {
    document
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim_end_matches('\n')
        .to_owned()
}

pub(super) fn generated_file_diff(file_name: &str, existing: &str, generated: &str) -> String {
    let existing = normalized_generated_document(existing);
    let generated = normalized_generated_document(generated);
    let before = existing.lines().collect::<Vec<_>>();
    let after = generated.lines().collect::<Vec<_>>();
    let mut common = vec![vec![0_usize; after.len() + 1]; before.len() + 1];
    for before_index in (0..before.len()).rev() {
        for after_index in (0..after.len()).rev() {
            common[before_index][after_index] = if before[before_index] == after[after_index] {
                common[before_index + 1][after_index + 1] + 1
            } else {
                common[before_index + 1][after_index].max(common[before_index][after_index + 1])
            };
        }
    }

    let mut diff = format!("--- Existing {file_name}\n+++ Package-generated {file_name}\n");
    let (mut before_index, mut after_index) = (0, 0);
    while before_index < before.len() || after_index < after.len() {
        if before_index < before.len()
            && after_index < after.len()
            && before[before_index] == after[after_index]
        {
            diff.push_str("  ");
            diff.push_str(before[before_index]);
            before_index += 1;
            after_index += 1;
        } else if after_index == after.len()
            || (before_index < before.len()
                && common[before_index + 1][after_index] >= common[before_index][after_index + 1])
        {
            diff.push_str("- ");
            diff.push_str(before[before_index]);
            before_index += 1;
        } else {
            diff.push_str("+ ");
            diff.push_str(after[after_index]);
            after_index += 1;
        }
        diff.push('\n');
    }
    diff
}

pub(super) fn settings_size_label(bytes: usize) -> String {
    const KIB: usize = 1024;
    const MIB: usize = 1024 * KIB;
    if bytes % MIB == 0 {
        format!("{} MiB", bytes / MIB)
    } else {
        format!("{} KiB", bytes / KIB)
    }
}

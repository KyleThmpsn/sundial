use super::settings;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SaveAction {
    Save,
    SaveAndExit,
}

pub(super) fn settings_save_note(result: &settings::SaveJsonResult) -> String {
    let limit = settings_size_label(result.size_limit_bytes);
    let mut note = if result.compacted {
        format!(
            " Sunrise-style formatting exceeded this schema's {limit} limit, so Sundial compacted the file to {} bytes.",
            result.encoded_bytes,
        )
    } else {
        String::new()
    };
    if let Some(warning) = &result.durability_warning {
        note.push(' ');
        note.push_str(warning);
        note.push('.');
    }
    note
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

use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

const MAX_ACTIVITY_BYTES: u64 = 256 * 1024;

pub(super) fn append_parhelion_log(report: &mut String) {
    if let Some(path) = crate::activity_log::Product::Parhelion.log_path() {
        append_at(report, &path);
    }
}

fn append_at(report: &mut String, path: &Path) {
    match read_tail(path) {
        Ok((text, truncated)) if !text.trim().is_empty() => {
            report.push_str("Recent Parhelion Activity (Oldest First)\n---------------------------------------\n");
            report.push_str(&format!("source = {}\n", path.display()));
            if truncated {
                report.push_str("Earlier entries omitted. Showing the latest 256 KiB.\n");
            }
            report.push_str(&text);
            report.push_str("\n\n");
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            report.push_str(&format!("Parhelion Activity Unavailable\n-----------------------------\nsource = {}\nerror = {error}\n\n", path.display()));
        }
    }
}

fn read_tail(path: &Path) -> io::Result<(String, bool)> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let start = length.saturating_sub(MAX_ACTIVITY_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(MAX_ACTIVITY_BYTES).read_to_end(&mut bytes)?;
    // Drop the partial first record, including any split UTF-8 character.
    let start_of_record = if start > 0 {
        bytes
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(bytes.len(), |index| index + 1)
    } else {
        0
    };
    Ok((
        String::from_utf8_lossy(&bytes[start_of_record..]).into_owned(),
        start > 0,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDirectory;

    #[test]
    fn omits_absent_and_empty_logs_and_includes_available_activity() {
        let directory = TestDirectory::new("diagnostic-parhelion-activity");
        let path = directory.0.join("parhelion.log");
        let mut report = "Sundial report\n".to_owned();
        append_at(&mut report, &path);
        std::fs::write(&path, "").unwrap();
        append_at(&mut report, &path);
        assert_eq!(report, "Sundial report\n");
        std::fs::write(&path, "2026-09-09 [Info] Build complete: 星\n").unwrap();
        append_at(&mut report, &path);
        assert!(report.contains("Recent Parhelion Activity"));
        assert!(report.contains("Build complete: 星"));
        assert!(report.contains(&path.display().to_string()));
    }

    #[test]
    fn bounds_large_logs_and_preserves_recent_complete_records() {
        let directory = TestDirectory::new("diagnostic-parhelion-tail");
        let path = directory.0.join("parhelion.log");
        let entries = format!(
            "{}\nLatest: 星\n",
            "古い\n".repeat(MAX_ACTIVITY_BYTES as usize / 4)
        );
        std::fs::write(&path, entries).unwrap();
        let (text, truncated) = read_tail(&path).unwrap();
        assert!(truncated);
        assert!(text.len() <= MAX_ACTIVITY_BYTES as usize);
        assert!(!text.contains('\u{fffd}'));
        assert!(text.ends_with("Latest: 星\n"));
    }

    #[test]
    fn unreadable_activity_does_not_discard_the_report() {
        let directory = TestDirectory::new("diagnostic-parhelion-read-error");
        let mut report = "Sundial report\n".to_owned();
        append_at(&mut report, &directory.0);
        assert!(report.starts_with("Sundial report\n"));
        assert!(report.contains("Parhelion Activity Unavailable"));
    }
}

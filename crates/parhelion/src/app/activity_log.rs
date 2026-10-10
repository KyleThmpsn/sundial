//! The in-app activity log: a bounded ring of entries that can also go to the log file.
use super::*;

pub(super) const ACTIVITY_LOG_CAPACITY: usize = 30;

#[derive(Default)]
pub(super) struct ActivityLog {
    pub(super) entries: VecDeque<LogEntry>,
    pub(super) notice: Option<String>,
    pub(super) file: sundial::activity_log::FileLog,
}

impl ActivityLog {
    pub(super) fn enable_file(&mut self) {
        self.file.enable(sundial::activity_log::Product::Parhelion);
        for entry in &self.entries {
            self.file.append(entry);
        }
    }

    pub(super) fn new(entry: LogEntry) -> Self {
        let notice = entry.error.then(|| entry.text.clone());
        Self {
            entries: VecDeque::from([entry]),
            notice,
            file: Default::default(),
        }
    }

    pub(super) fn push(&mut self, entry: LogEntry) {
        self.file.append(&entry);
        if entry.error {
            self.notice = Some(entry.text.clone());
        }
        if self.entries.len() >= ACTIVITY_LOG_CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    pub(super) fn iter(&self) -> impl DoubleEndedIterator<Item = &LogEntry> {
        self.entries.iter()
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }
}

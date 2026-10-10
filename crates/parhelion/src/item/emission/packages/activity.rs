//! A worker timestamps its own stages before the bounded channel can delay delivery.
use super::*;

pub(super) struct Reporter<'a> {
    send: &'a mpsc::SyncSender<Event>,
    index: usize,
    id: u16,
    step: u32,
    label: String,
    started: Instant,
    admitted: Instant,
    queued: std::time::Duration,
    reporting: std::time::Duration,
}

impl<'a> Reporter<'a> {
    pub(super) fn new(
        send: &'a mpsc::SyncSender<Event>,
        index: usize,
        id: u16,
        queued: Instant,
    ) -> Self {
        let now = Instant::now();
        let file = crate::package_profile::authored_package(id)
            .expect("planned package")
            .file_name;
        crate::block_codec::Timings::take();
        let mut result = Self {
            send,
            index,
            id,
            step: 0,
            label: format!("Preparing Package: {file}"),
            started: now,
            admitted: now,
            queued: now.saturating_duration_since(queued),
            reporting: std::time::Duration::ZERO,
        };
        result.event(now, OperationStatus::Started, false);
        result.started = Instant::now();
        result
    }

    fn event(&mut self, timestamp: Instant, status: OperationStatus, completed: bool) {
        let reporting = Instant::now();
        let _ = self.send.send(Event::Activity {
            index: self.index,
            label: self.label.clone(),
            timestamp,
            completed,
            activity: BuildActivity::Package {
                id: self.id,
                step: self.step,
                status,
                duration: timestamp.saturating_duration_since(self.started),
            },
        });
        self.reporting += reporting.elapsed();
    }

    pub(super) fn start(&mut self, label: &str) {
        let now = Instant::now();
        self.event(now, OperationStatus::Finished, false);
        self.step += 1;
        self.started = Instant::now();
        self.label = label.to_owned();
        self.event(self.started, OperationStatus::Started, false);
        self.started = Instant::now();
    }

    pub(super) fn finish(mut self, failed: bool) {
        let now = Instant::now();
        self.event(
            now,
            if failed {
                OperationStatus::Failed
            } else {
                OperationStatus::Finished
            },
            !failed,
        );
        let timings = crate::block_codec::Timings::take();
        let file = crate::package_profile::authored_package(self.id)
            .expect("planned package")
            .file_name;
        let _ = self.send.send(Event::Activity {
            index: self.index, label: String::new(), timestamp: now, completed: false,
            activity: BuildActivity::Diagnostic(format!(
                "Package timings: {file}. Queue {:.3}s, elapsed {:.3}s, progress wait {:.3}s, codec wait {:.3}s, compression {:.3}s, codec verification {:.3}s, {} blocks.",
                self.queued.as_secs_f64(), now.saturating_duration_since(self.admitted).as_secs_f64(),
                self.reporting.as_secs_f64(),
                timings.wait.as_secs_f64(), timings.compression.as_secs_f64(), timings.verification.as_secs_f64(), timings.blocks,
            )),
        });
    }
}

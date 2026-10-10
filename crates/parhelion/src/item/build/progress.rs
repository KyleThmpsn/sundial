//! Count completed compiler work without estimating elapsed time.

use super::AuthoringResult;
use crate::workflow::BuildActivity;
use std::time::Instant;

#[derive(Clone, Copy)]
pub(crate) enum Phase {
    Authoring,
    Payloads,
}

pub(crate) struct Event<'a> {
    pub phase: Phase,
    pub label: &'a str,
    pub completed: usize,
    pub total: usize,
    pub timestamp: Instant,
    pub activity: Option<BuildActivity>,
}

pub(in crate::item) struct Progress<'a> {
    phase: Phase,
    completed: usize,
    total: usize,
    report: &'a mut dyn FnMut(Event<'_>),
}

impl<'a> Progress<'a> {
    pub(in crate::item) fn new(total: usize, report: &'a mut dyn FnMut(Event<'_>)) -> Self {
        Self {
            phase: Phase::Authoring,
            completed: 0,
            total,
            report,
        }
    }

    pub(in crate::item) fn payloads(&mut self, total: usize) {
        self.phase = Phase::Payloads;
        self.completed = 0;
        self.total = total;
    }

    pub(in crate::item) fn start(&mut self, label: &str) {
        self.emit(label, Instant::now(), None);
    }

    pub(in crate::item) fn finish(&mut self, label: &str) {
        self.completed += 1;
        self.emit(label, Instant::now(), None);
    }

    fn emit(&mut self, label: &str, timestamp: Instant, activity: Option<BuildActivity>) {
        (self.report)(Event {
            phase: self.phase,
            label,
            completed: self.completed,
            total: self.total,
            timestamp,
            activity,
        });
    }

    pub(in crate::item) fn activity(
        &mut self,
        label: &str,
        timestamp: Instant,
        activity: BuildActivity,
        completed_package: bool,
    ) {
        self.completed += usize::from(completed_package);
        self.emit(label, timestamp, Some(activity));
    }

    pub(in crate::item) fn diagnostic(&mut self, message: String) {
        self.emit("", Instant::now(), Some(BuildActivity::Diagnostic(message)));
    }

    pub(in crate::item) fn step<T>(
        &mut self,
        label: &str,
        operation: impl FnOnce() -> AuthoringResult<T>,
    ) -> AuthoringResult<T> {
        self.start(label);
        let result = operation().map_err(|error| error.context(label))?;
        self.finish(label);
        Ok(result)
    }
}

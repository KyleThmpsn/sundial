//! Count completed compiler work without estimating elapsed time.

use super::AuthoringResult;

#[derive(Clone, Copy)]
pub(crate) enum Phase {
    Authoring,
    Payloads,
}

pub(in crate::weapon) struct Progress<'a> {
    phase: Phase,
    completed: usize,
    total: usize,
    report: &'a mut dyn FnMut(Phase, &str, usize, usize),
}

impl<'a> Progress<'a> {
    pub(in crate::weapon) fn new(
        total: usize,
        report: &'a mut dyn FnMut(Phase, &str, usize, usize),
    ) -> Self {
        Self {
            phase: Phase::Authoring,
            completed: 0,
            total,
            report,
        }
    }

    pub(in crate::weapon) fn payloads(&mut self, total: usize) {
        self.phase = Phase::Payloads;
        self.completed = 0;
        self.total = total;
    }

    pub(in crate::weapon) fn start(&mut self, label: &str) {
        (self.report)(self.phase, label, self.completed, self.total);
    }

    pub(in crate::weapon) fn finish(&mut self, label: &str) {
        self.completed += 1;
        (self.report)(self.phase, label, self.completed, self.total);
    }

    pub(in crate::weapon) fn step<T>(
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

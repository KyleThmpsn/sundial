//! Navigation state for the progression metadata inspector.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::app) enum MetadataSelection {
    FlagDefinition(usize),
    ValueDefinition(usize),
    FlagOverride(usize),
    ValueOverride(usize),
}

impl Default for MetadataSelection {
    fn default() -> Self {
        Self::FlagDefinition(0)
    }
}

impl MetadataSelection {
    pub(in crate::app) const fn definition_index(self) -> usize {
        match self {
            Self::FlagDefinition(index)
            | Self::ValueDefinition(index)
            | Self::FlagOverride(index)
            | Self::ValueOverride(index) => index,
        }
    }

    pub(in crate::app) const fn is_value(self) -> bool {
        matches!(self, Self::ValueDefinition(_) | Self::ValueOverride(_))
    }
}

#[derive(Debug, Default)]
pub(in crate::app) struct ProgressionInspectorState {
    selection: Option<MetadataSelection>,
    history: Vec<MetadataSelection>,
    forward: Vec<MetadataSelection>,
    pub(super) full_width: bool,
    reveal_request: Option<MetadataSelection>,
}

impl ProgressionInspectorState {
    pub(in crate::app) const fn is_open(&self) -> bool {
        self.selection.is_some()
    }

    pub(super) const fn selection(&self) -> Option<MetadataSelection> {
        self.selection
    }

    pub(super) const fn can_go_back(&self) -> bool {
        !self.history.is_empty()
    }

    pub(super) fn previous(&self) -> Option<MetadataSelection> {
        self.history.last().copied()
    }

    pub(super) fn next(&self) -> Option<MetadataSelection> {
        self.forward.last().copied()
    }

    pub(in crate::app) fn open(&mut self, selection: MetadataSelection) {
        if self.selection == Some(selection) {
            return;
        }
        if let Some(current) = self.selection {
            self.history.push(current);
            trim_navigation_stack(&mut self.history);
        }
        self.selection = Some(selection);
        self.forward.clear();
    }

    pub(super) fn back(&mut self) {
        if let Some(previous) = self.history.pop()
            && let Some(current) = self.selection.replace(previous)
        {
            self.forward.push(current);
            trim_navigation_stack(&mut self.forward);
        }
    }

    pub(super) fn forward(&mut self) {
        if let Some(next) = self.forward.pop()
            && let Some(current) = self.selection.replace(next)
        {
            self.history.push(current);
            trim_navigation_stack(&mut self.history);
        }
    }

    pub(super) fn request_reveal(&mut self) {
        let selection = self.selection;
        self.close();
        self.reveal_request = selection;
    }

    pub(in crate::app) fn take_reveal_request(&mut self) -> Option<MetadataSelection> {
        self.reveal_request.take()
    }

    pub(super) fn close(&mut self) {
        self.selection = None;
        self.history.clear();
        self.forward.clear();
        self.full_width = false;
        self.reveal_request = None;
    }

    pub(in crate::app) fn reset(&mut self) {
        self.close();
    }
}

fn trim_navigation_stack(stack: &mut Vec<MetadataSelection>) {
    const LIMIT: usize = 32;
    let overflow = stack.len().saturating_sub(LIMIT);
    if overflow > 0 {
        stack.drain(0..overflow);
    }
}

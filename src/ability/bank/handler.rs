//! A handler index is local to its bank and cannot stand in for an ability target.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct HandlerIndex(u32);

impl HandlerIndex {
    pub const fn new(index: u32) -> Self {
        Self(index)
    }
    pub const fn get(self) -> u32 {
        self.0
    }
}

//! The serialized index selects a bank-local modifier priority byte. Native compatibility and
//! application use separate typed callbacks. The public name is retained for saved callers.
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

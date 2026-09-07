//! Dependency-free random selection used by the equipment randomizer.

use super::*;

/// A dependency-free xorshift generator. UI variety does not require a
/// cryptographic random source.
pub(super) struct Rng(u64);

impl Rng {
    pub(super) fn from_clock() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| {
                since
                    .as_secs()
                    .wrapping_mul(1_000_000_000)
                    .wrapping_add(u64::from(since.subsec_nanos()))
            });
        Self::from_seed(seed)
    }

    pub(super) fn from_seed(seed: u64) -> Self {
        Self(seed | 1)
    }

    pub(super) fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub(super) fn pick<'a, T>(&mut self, options: &'a [T]) -> Option<&'a T> {
        let count = u64::try_from(options.len())
            .ok()
            .filter(|count| *count > 0)?;
        let index = usize::try_from(self.next() % count).ok()?;
        options.get(index)
    }

    pub(super) fn pick_valid_hash(&mut self, options: &[u64]) -> Option<u64> {
        let count = options.len();
        let count_u64 = u64::try_from(count).ok().filter(|count| *count > 0)?;
        let start = usize::try_from(self.next() % count_u64).ok()?;
        (0..count).find_map(|offset| {
            let hash = options[(start + offset) % count];
            valid_definition_hash(hash).then_some(hash)
        })
    }
}

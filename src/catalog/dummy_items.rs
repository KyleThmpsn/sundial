//! Display-only item definitions present in Destiny 2 build 86657.20.08.23.
//!
//! Bungie's public manifest assigns these hashes to its hidden "Dummies" category. `hashes.json`
//! holds the complete category membership from the cached public manifest for this exact game
//! build, public manifest version 86657.20.08.23.1800-9.
use std::sync::OnceLock;

use serde::Deserialize;

use crate::hash::parse_hash_hex;

const DATABASE_JSON: &str = include_str!("dummy_items/hashes.json");

#[derive(Deserialize)]
struct Database {
    hashes: Vec<String>,
}

/// The sorted membership list, parsed once.
fn hashes() -> &'static [u64] {
    static HASHES: OnceLock<Vec<u64>> = OnceLock::new();
    HASHES.get_or_init(|| {
        let database: Database = serde_json::from_str(DATABASE_JSON)
            .expect("the bundled dummy item list must be valid JSON");
        let hashes = database
            .hashes
            .iter()
            .map(|text| {
                parse_hash_hex(text).expect("dummy item hashes are 0x-prefixed hexadecimal")
            })
            .collect::<Vec<_>>();
        assert!(
            hashes.windows(2).all(|pair| pair[0] < pair[1]),
            "dummy item hashes must be sorted and unique"
        );
        hashes
    })
}

pub(crate) fn contains(hash: u64) -> bool {
    hashes().binary_search(&hash).is_ok()
}

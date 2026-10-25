//! Gameplay observations supplement native identity without changing native assets.
//! These are observations on test weapons, not guarantees for new combinations.
use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Observation {
    pub graph: u32,
    pub name: String,
    pub summary: String,
    pub aliases: String,
    pub source: String,
}

fn observations() -> &'static [Observation] {
    static DATA: OnceLock<Vec<Observation>> = OnceLock::new();
    DATA.get_or_init(|| {
        let mut entries: Vec<Observation> = serde_json::from_str(include_str!("knowledge.json"))
            .expect("bundled gameplay observations");
        entries.sort_by_key(|entry| entry.graph);
        entries
    })
}

pub fn get(graph: u32) -> Option<&'static Observation> {
    let entries = observations();
    entries
        .binary_search_by_key(&graph, |entry| entry.graph)
        .ok()
        .map(|index| &entries[index])
}

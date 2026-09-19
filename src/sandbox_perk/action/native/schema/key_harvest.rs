//! Scratch probe: collect every name hash the stock perk data actually stores, so a live
//! client scan can try to resolve the ones the workbench shows as bare hexadecimal.
//!
//! Property keys, event keys and predicate named keys are all FNV-1 name hashes. The
//! workbench renders them as `0x........` because nothing in the packages names them. They
//! are the best remaining target for a client memory scan: unlike the schema's member names,
//! these are values the running game looks up by name, so the strings have a reason to be
//! resident.
//!
//! Run with `cargo test --release -p sundial key_harvest -- --ignored --nocapture`.

use std::collections::BTreeMap;

use super::field_map::{action_tag, is_synthetic, survey_dir};
use crate::sandbox_perk::action::native::{Graph, fields, fields::Format};
use crate::sandbox_perk::action::{ACTION_ROOT_CLASS, decode};

/// Per key hash: how many stock nodes store it, and the class and offset sites it sits at.
type KeyUses = BTreeMap<u32, (usize, BTreeMap<(u32, usize), usize>)>;

/// Hashes this project already resolves, so the harvest reports only what is still unnamed.
fn already_named(hash: u32) -> bool {
    hash == crate::sandbox_perk::program::EMPTY_KEY
        || hash == 0
        || hash == u32::MAX
        || crate::package_runtime::labels::name(hash).is_some()
}

#[test]
#[ignore = "scratch harvest, needs the captured survey, run with --ignored --nocapture"]
fn scratch_harvest_stock_key_hashes() {
    let Some(dir) = survey_dir() else {
        println!("no survey");
        return;
    };
    // Per hash: how many stock nodes store it, and which classes and offsets it sits at.
    let mut keys = KeyUses::new();
    for path in std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()) {
        if path.extension().is_none_or(|e| e != "bin") {
            continue;
        }
        let Some(tag) = action_tag(&path) else {
            continue;
        };
        if is_synthetic(tag) {
            continue;
        }
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        if decode(&data).is_err() {
            continue;
        }
        let Ok(graph) = Graph::read(&data, 0, ACTION_ROOT_CLASS) else {
            continue;
        };
        for block in &graph.blocks {
            let Ok(described) = fields::describe(block.class) else {
                continue;
            };
            let stride = match super::record(block.class) {
                Ok(record) if record.size > 0 => record.size,
                _ => continue,
            };
            for row in 0..block.count.unwrap_or(1) {
                for field in described.iter().filter(|f| f.format == Format::Key) {
                    let at = row * stride + field.offset;
                    let Some(bytes) = block.bytes.get(at..at + 4) else {
                        continue;
                    };
                    let hash = u32::from_le_bytes(bytes.try_into().unwrap());
                    if already_named(hash) {
                        continue;
                    }
                    let entry = keys.entry(hash).or_default();
                    entry.0 += 1;
                    *entry.1.entry((block.class, field.offset)).or_default() += 1;
                }
            }
        }
    }
    let mut ranked: Vec<_> = keys.iter().collect();
    ranked.sort_by(|a, b| b.1.0.cmp(&a.1.0));
    println!("unnamed key hashes in stock data: {}", ranked.len());
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/stock-key-hashes.txt");
    let mut out = String::new();
    for (hash, (uses, sites)) in &ranked {
        let where_at = sites
            .keys()
            .take(2)
            .map(|(class, offset)| format!("{}+0x{offset:X}", fields::name(*class)))
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&format!("{hash:08X} key x{uses} {where_at}\n"));
    }
    let _ = std::fs::write(&path, &out);
    for line in out.lines().take(15) {
        println!("  {line}");
    }
    println!("wrote {}", path.display());
}

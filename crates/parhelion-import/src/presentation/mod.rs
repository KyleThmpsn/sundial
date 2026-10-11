//! Shared native presentation graph and geometry emission.
pub(crate) mod channel;
pub(crate) mod geometry;
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::path::PathBuf;
pub(crate) fn put(b: &mut [u8], o: usize, v: &[u8]) -> Result<()> {
    b.get_mut(o..o + v.len())
        .context("native write outside payload")?
        .copy_from_slice(v);
    Ok(())
}
pub(crate) fn append(
    b: &mut Vec<u8>,
    o: usize,
    class: u64,
    rows: &[u8],
    stride: usize,
) -> Result<()> {
    ensure!(
        stride > 0 && rows.len().is_multiple_of(stride),
        "array stride mismatch"
    );
    if rows.is_empty() {
        return put(b, o, &[0; 16]);
    }
    let h = (b.len() + 19) & !15;
    b.resize(h - 4, 0);
    b.extend(0x80809fbdu32.to_le_bytes());
    b.extend((rows.len() as u64 / stride as u64).to_le_bytes());
    b.extend(class.to_le_bytes());
    b.extend(rows);
    put(b, o, &(rows.len() as u64 / stride as u64).to_le_bytes())?;
    put(b, o + 8, &(h as i64 - o as i64 - 8).to_le_bytes())?;
    let len = b.len() as u64;
    put(b, 0, &len.to_le_bytes())
}
pub(crate) struct Graph {
    pub root: PathBuf,
    pub nodes: Vec<Value>,
}
impl Graph {
    pub fn seal_instances(&mut self) -> Result<Vec<Value>> {
        let mut changes = Vec::new();
        for layout in crate::tiger::instance::LAYOUTS {
            let Ok(node) = self.node(layout.symbol) else {
                continue;
            };
            let mut patches = node["patches"]
                .as_array()
                .context("Presentation component patches")?
                .clone();
            let mut payload = self.read(layout.symbol)?;
            if let Some(evidence) =
                crate::tiger::instance::seal(&mut payload, &mut patches, layout)?
            {
                self.replace(layout.symbol, &payload.0, patches)?;
                changes.push(evidence);
            }
        }
        Ok(changes)
    }

    pub fn node(&self, symbol: &str) -> Result<&Value> {
        self.nodes
            .iter()
            .find(|node| node["symbol"] == symbol)
            .with_context(|| format!("Missing presentation node {symbol}"))
    }
    pub fn read(&self, symbol: &str) -> Result<crate::tiger::payload::Payload> {
        Ok(crate::tiger::payload::Payload(std::fs::read(
            self.root.join(
                self.node(symbol)?["file"]
                    .as_str()
                    .context("Presentation node path")?,
            ),
        )?))
    }
    pub fn replace(&mut self, symbol: &str, bytes: &[u8], patches: Vec<Value>) -> Result<()> {
        std::fs::write(
            self.root.join(
                self.node(symbol)?["file"]
                    .as_str()
                    .context("Presentation node path")?,
            ),
            bytes,
        )?;
        self.nodes
            .iter_mut()
            .find(|node| node["symbol"] == symbol)
            .context("Presentation node")?["patches"] = serde_json::json!(patches);
        Ok(())
    }
    pub fn add(
        &mut self,
        symbol: &str,
        template: u32,
        data: &[u8],
        reference: Option<&str>,
        patches: Vec<Value>,
    ) -> Result<()> {
        ensure!(
            !self.nodes.iter().any(|n| n["symbol"] == symbol),
            "duplicate graph symbol"
        );
        crate::graph::add(
            &self.root,
            &mut self.nodes,
            symbol,
            template,
            data,
            reference,
            patches,
        )
    }
}

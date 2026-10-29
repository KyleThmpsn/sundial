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
    /// The position of each node's symbol, once [`Self::index_symbols`] asks for it.
    symbols: std::sync::Mutex<Option<Symbols>>,
}
/// The first node holding each symbol among the first `indexed` nodes.
#[derive(Default)]
struct Symbols {
    at: std::collections::HashMap<String, usize>,
    indexed: usize,
}
impl Graph {
    pub fn new(root: PathBuf, nodes: Vec<Value>) -> Self {
        Self {
            root,
            nodes,
            symbols: std::sync::Mutex::new(None),
        }
    }

    /// Finds nodes by symbol through an index rather than by visiting every node. Each visit
    /// follows a node's own allocations, and a campaign level's 11,616 nodes took 18 seconds of
    /// duplicate checks that way. Only a graph whose nodes are appended, and whose symbols never
    /// change in place, may take the index: a symbol it has not seen is taken to be absent.
    pub fn index_symbols(&mut self) {
        *self.symbols.get_mut().unwrap_or_else(|e| e.into_inner()) = Some(Symbols::default());
    }

    /// The first node holding `symbol`.
    fn position(&self, symbol: &str) -> Option<usize> {
        let scan = || self.nodes.iter().position(|node| node["symbol"] == symbol);
        let mut guard = self.symbols.lock().unwrap_or_else(|e| e.into_inner());
        let Some(index) = guard.as_mut() else {
            return scan();
        };
        if self.nodes.len() < index.indexed {
            *index = Symbols::default();
        }
        for (at, node) in self.nodes.iter().enumerate().skip(index.indexed) {
            if let Some(name) = node["symbol"].as_str() {
                index.at.entry(name.to_owned()).or_insert(at);
            }
        }
        index.indexed = self.nodes.len();
        match index.at.get(symbol) {
            None => None,
            Some(&at) if self.nodes[at]["symbol"] == symbol => Some(at),
            // A node changed in place since it was indexed.
            Some(_) => {
                *index = Symbols::default();
                scan()
            }
        }
    }

    /// Whether a node holds `symbol`.
    pub fn contains(&self, symbol: &str) -> bool {
        self.position(symbol).is_some()
    }

    /// Join a graph written beneath its own directory and symbol namespace.
    /// Symbolic patches stay symbolic until the complete graph is linked.
    pub fn extend(&mut self, graph: Self, prefix: &str) -> Result<()> {
        ensure!(
            !prefix.is_empty()
                && prefix
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_')),
            "Invalid presentation namespace"
        );
        ensure!(
            graph.root == self.root.join(prefix),
            "Nested presentation directory differs"
        );
        let symbols = graph
            .nodes
            .iter()
            .map(|node| node["symbol"].as_str().context("Nested graph symbol"))
            .collect::<Result<std::collections::BTreeSet<_>>>()?;
        ensure!(
            symbols.len() == graph.nodes.len(),
            "Nested graph symbols collide"
        );
        let rename = |name: &str| -> Result<String> {
            ensure!(
                symbols.contains(name),
                "Nested graph reference leaves its namespace"
            );
            Ok(format!("{prefix}-{name}"))
        };
        let mut joined = Vec::new();
        for source in &graph.nodes {
            let mut node = source.clone();
            let symbol = rename(source["symbol"].as_str().context("Nested graph symbol")?)?;
            ensure!(!self.contains(&symbol), "Presentation namespace collides");
            node["symbol"] = Value::String(symbol);
            let file = source["file"].as_str().context("Nested graph file")?;
            ensure!(
                std::path::Path::new(file)
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_))),
                "Nested presentation file leaves its directory"
            );
            node["file"] = Value::String(format!("{prefix}/{file}"));
            for field in ["reference", "shared_owner"] {
                if let Some(value) = source[field].as_str() {
                    node[field] = Value::String(rename(value)?);
                }
            }
            for patch in node["patches"]
                .as_array_mut()
                .context("Nested graph patches")?
            {
                patch["symbol"] = Value::String(rename(
                    patch["symbol"].as_str().context("Nested graph patch")?,
                )?);
            }
            joined.push(node);
        }
        self.nodes.extend(joined);
        Ok(())
    }

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
        self.position(symbol)
            .map(|at| &self.nodes[at])
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
        ensure!(!self.contains(symbol), "duplicate graph symbol");
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

    /// Places node `symbol` as [`Self::add`] does, before its payload exists. [`Self::write`]
    /// then writes the payload.
    pub fn reserve(
        &mut self,
        symbol: &str,
        template: u32,
        reference: Option<&str>,
        patches: Vec<Value>,
    ) -> Result<()> {
        ensure!(!self.contains(symbol), "duplicate graph symbol");
        self.nodes.push(serde_json::json!({"symbol":symbol,"template":template,"file":format!("{symbol}.bin"),"reference":reference,"patches":patches}));
        Ok(())
    }

    /// Writes the payload of node `symbol`, placed by [`Self::reserve`].
    pub fn write(&self, symbol: &str, data: &[u8]) -> Result<()> {
        Ok(std::fs::write(self.path(symbol), data)?)
    }

    /// The file of node `symbol`'s payload, as [`Self::add`] and [`Self::reserve`] name it.
    pub fn path(&self, symbol: &str) -> PathBuf {
        self.root.join(format!("{symbol}.bin"))
    }
}

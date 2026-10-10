//! Private graph nodes use temporary identities only while translating. Every occurrence
//! is replaced by a symbol before the graph reaches Parhelion's package allocator.
use super::*;

#[derive(Clone)]
pub(crate) struct Node {
    pub id: u32,
    pub symbol: String,
    pub template: u32,
    pub payload: Payload,
    pub reference: Option<u32>,
    storage: Option<&'static str>,
    patches: Vec<(usize, String)>,
}

#[derive(Clone, Default)]
pub(crate) struct Assets {
    pub names: BTreeMap<u32, String>,
    pub nodes: Vec<Node>,
}

impl Assets {
    pub fn reserve(&mut self, native: &Reader, symbol: String) -> Result<u32> {
        let id = 0x81FF0000u32
            .checked_add(u32::try_from(self.names.len() + 1)?)
            .context("projectile relocation capacity")?;
        ensure!(
            id <= 0x81FFFFFF && native.reference(id).is_err(),
            "projectile temporary identity collides with native content"
        );
        ensure!(
            !self.names.values().any(|name| name == &symbol),
            "duplicate projectile symbol"
        );
        self.names.insert(id, symbol);
        Ok(id)
    }

    pub fn external(&mut self, native: &Reader, symbol: &str) -> Result<u32> {
        if let Some((&id, _)) = self.names.iter().find(|(_, name)| name.as_str() == symbol) {
            return Ok(id);
        }
        self.reserve(native, symbol.into())
    }

    pub fn push(
        &mut self,
        id: u32,
        template: u32,
        payload: Payload,
        reference: Option<u32>,
    ) -> Result<()> {
        ensure!(
            !self.nodes.iter().any(|node| node.id == id),
            "projectile node emitted twice"
        );
        self.nodes.push(Node {
            id,
            symbol: self
                .names
                .get(&id)
                .context("unreserved projectile node")?
                .clone(),
            template,
            payload,
            reference,
            storage: None,
            patches: Vec::new(),
        });
        Ok(())
    }

    pub fn audio(
        &mut self,
        id: u32,
        template: u32,
        payload: Vec<u8>,
        bank: bool,
        patches: Vec<(usize, String)>,
    ) -> Result<()> {
        self.push(id, template, Payload(payload), None)?;
        let node = self.nodes.last_mut().context("missing audio node")?;
        node.storage = Some(if bank { "audio_bank" } else { "audio_media" });
        for (offset, _) in &patches {
            ensure!(
                node.payload.0.get(*offset..offset + 4) == Some(&u32::MAX.to_le_bytes()[..]),
                "audio media reference is not a placeholder"
            );
        }
        node.patches = patches;
        Ok(())
    }

    pub fn write(self, graph: &Path, root: u32, report: Value) -> Result<Value> {
        let directory = graph.join("projectile");
        ensure!(!directory.exists(), "projectile output already exists");
        fs::create_dir_all(&directory)?;
        let mut nodes = Vec::new();
        for mut node in self.nodes {
            let mut patches = node
                .patches
                .iter()
                .map(|(offset, symbol)| json!({"offset":offset,"symbol":symbol}))
                .collect::<Vec<_>>();
            // PCM samples and Wwise IDs are not package tags. Bank patches came from
            // the checked bank parser, whose fields need not be four-byte aligned.
            for at in (0..if node.storage.is_none() {
                node.payload.0.len().saturating_sub(3)
            } else {
                0
            })
                .step_by(4)
            {
                if let Some(symbol) = self.names.get(&node.payload.u32(at)?) {
                    patches.push(json!({"offset":at,"symbol":symbol}));
                    put(&mut node.payload, at, &u32::MAX.to_le_bytes())?;
                }
            }
            let file = format!("projectile/{}.bin", node.symbol);
            fs::write(graph.join(&file), &node.payload.0)?;
            nodes.push(json!({"symbol":node.symbol,"template":node.template,"file":file,
                "reference":node.reference.map(|id| self.names.get(&id).cloned().context("unreserved projectile reference")).transpose()?,
                "storage":node.storage,
                "patches":patches}));
        }
        Ok(
            json!({"installable":true,"root":self.names.get(&root).context("missing projectile root")?,
            "nodes":nodes,"conversion":report}),
        )
    }
}

pub(crate) fn metadata(
    reader: &mut Reader,
    payloads: &[&Payload],
    output: &mut BTreeMap<u32, Payload>,
) -> Result<()> {
    let mut tags = BTreeSet::new();
    for payload in payloads {
        // Network owners use package-defined root schemas. Their converters check
        // the schema's base type, size and name in addition to provider metadata.
        for field in [16, 24] {
            let class = payload.u32(payload.pointer(field)? + 4)?;
            if (0x80810000..=0x81FFFFFF).contains(&class) {
                reader.reference(class)?;
                tags.insert(class);
            }
        }
        for at in (0..payload.0.len().saturating_sub(3)).step_by(4) {
            let tag = payload.u32(at)?;
            if (0x80800001..=0x81FFFFFF).contains(&tag)
                && matches!(reader.reference(tag), Ok(0x80809B23 | 0x80809C54))
            {
                tags.insert(tag);
            }
        }
    }
    for tag in tags {
        let payload = reader.tag(tag, None)?;
        if let Some(previous) = output.insert(tag, (*payload).clone()) {
            ensure!(
                previous.0 == payload.0,
                "cross-version metadata identity collision"
            );
        }
    }
    Ok(())
}

pub(crate) fn retag(payload: &Payload, tag: u32) -> Result<Payload> {
    let mut result = payload.clone();
    let previous = payload.u32(payload.pointer(16)?)?;
    for at in (0..payload.0.len().saturating_sub(15)).step_by(4) {
        if payload.u32(at)? == previous {
            ensure!(
                payload.u32(at + 4)? & 0xFFFF0000 == 0x80800000
                    && payload.u64(at + 8)? < payload.0.len() as u64,
                "untyped component owner occurrence"
            );
            put(&mut result, at, &tag.to_le_bytes())?;
        }
    }
    Ok(result)
}

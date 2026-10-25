//! Bind graph audio after the asset group receives its final private media tags.
use super::*;
use parhelion_import::d2_mot::{AudioNamespace, ConvertedAudioBank};
use serde::Deserialize;

#[derive(Default, Deserialize)]
pub(in crate::item) struct Metadata {
    #[serde(default, rename = "audio_bank")]
    bank: Option<ConvertedAudioBank>,
    #[serde(default, rename = "audio_media")]
    media: Vec<Medium>,
    #[serde(default, rename = "audio_events")]
    events: Vec<Event>,
}

#[derive(Deserialize)]
struct Medium {
    id: u32,
    symbol: String,
}

#[derive(Deserialize)]
struct Event {
    offset: usize,
    id: u32,
}

pub(in crate::item) struct Node<'a> {
    pub symbol: &'a str,
    pub payload: &'a [u8],
    pub storage: crate::NewTagStorageMode,
    pub patched: bool,
    pub metadata: &'a Metadata,
}

impl<'a> From<&'a super::Node<Metadata>> for Node<'a> {
    fn from(node: &'a super::Node<Metadata>) -> Self {
        Self {
            symbol: &node.symbol,
            payload: &node.payload,
            storage: node.storage,
            patched: !node.patches.is_empty(),
            metadata: &node.metadata,
        }
    }
}

pub(in crate::item) fn bind(
    nodes: &[Node<'_>],
    symbols: &BTreeMap<String, TagHash>,
    namespace: &str,
) -> AuthoringResult<BTreeMap<String, Vec<u8>>> {
    let banks = nodes
        .iter()
        .filter_map(|node| node.metadata.bank.as_ref().map(|bank| (node, bank)))
        .collect::<Vec<_>>();
    if banks.is_empty() {
        if nodes
            .iter()
            .any(|node| !node.metadata.events.is_empty() || !node.metadata.media.is_empty())
        {
            return Err(invalid("Graph audio events have no declared bank"));
        }
        return Ok(BTreeMap::new());
    }
    let namespace = AudioNamespace::new(
        namespace,
        &banks
            .iter()
            .map(|(_, bank)| (*bank).clone())
            .collect::<Vec<_>>(),
    )
    .map_err(|error| invalid(format!("Graph audio namespace: {error:#}")))?;
    let mut bound = BTreeMap::new();
    for (node, bank) in banks {
        if node.storage != crate::NewTagStorageMode::AudioBank
            || node.payload != bank.bytes.as_slice()
            || node.patched
        {
            return Err(invalid("Graph audio bank payload or storage differs"));
        }
        let mut media = BTreeMap::new();
        let mut sizes = BTreeMap::new();
        for row in &node.metadata.media {
            let medium = nodes
                .iter()
                .find(|node| node.symbol == row.symbol)
                .ok_or_else(|| invalid("Graph audio medium is absent"))?;
            if medium.storage != crate::NewTagStorageMode::AudioMedia
                || media
                    .insert(
                        row.id,
                        symbols
                            .get(&row.symbol)
                            .ok_or_else(|| invalid("Graph audio medium was not allocated"))?
                            .0,
                    )
                    .is_some()
            {
                return Err(invalid("Graph audio medium storage or identity differs"));
            }
            sizes.insert(
                row.id,
                u32::try_from(medium.payload.len())
                    .map_err(|_| invalid("Graph audio medium too large"))?,
            );
        }
        bound.insert(
            node.symbol.to_owned(),
            namespace
                .instantiate(bank, &media, &sizes)
                .map_err(|error| invalid(format!("Graph audio linking: {error:#}")))?,
        );
    }
    for node in nodes.iter().filter(|node| !node.metadata.events.is_empty()) {
        let payload = bound
            .entry(node.symbol.to_owned())
            .or_insert_with(|| node.payload.to_vec());
        for event in &node.metadata.events {
            if read_u32(payload, event.offset)? != event.id {
                return Err(invalid("Graph audio cue event changed"));
            }
            write_u32(
                payload,
                event.offset,
                namespace
                    .object(event.id)
                    .map_err(|error| invalid(format!("Graph audio event: {error:#}")))?,
            )?;
        }
    }
    Ok(bound)
}

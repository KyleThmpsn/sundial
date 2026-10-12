//! Bind graph audio after the runtime group receives its final private media tags.
use super::*;
use parhelion_import::d2_mot::{AudioNamespace, ConvertedAudioBank};
use serde::Deserialize;

#[derive(Default, Deserialize)]
pub(super) struct Metadata {
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

pub(super) fn bind(
    nodes: &[Node],
    symbols: &BTreeMap<String, TagHash>,
    item: u32,
) -> AuthoringResult<BTreeMap<String, Vec<u8>>> {
    let banks = nodes
        .iter()
        .filter_map(|node| node.audio.bank.as_ref().map(|bank| (node, bank)))
        .collect::<Vec<_>>();
    if banks.is_empty() {
        if nodes.iter().any(|node| !node.audio.events.is_empty()) {
            return Err(invalid("Graph audio events have no declared bank"));
        }
        return Ok(BTreeMap::new());
    }
    let namespace = AudioNamespace::new(
        &format!("parhelion/{item:08x}/graph-audio"),
        &banks
            .iter()
            .map(|(_, bank)| (*bank).clone())
            .collect::<Vec<_>>(),
    )
    .map_err(|error| invalid(format!("Graph audio namespace: {error:#}")))?;
    let mut bound = BTreeMap::new();
    for (node, bank) in banks {
        if node.storage != crate::NewTagStorageMode::AudioBank
            || node.payload != bank.bytes
            || !node.patches.is_empty()
        {
            return Err(invalid("Graph audio bank payload or storage differs"));
        }
        let mut media = BTreeMap::new();
        let mut sizes = BTreeMap::new();
        for row in &node.audio.media {
            let medium = nodes
                .iter()
                .find(|node| node.symbol == row.symbol)
                .ok_or_else(|| invalid("Graph audio medium is absent"))?;
            if medium.storage != crate::NewTagStorageMode::AudioMedia
                || media.insert(row.id, symbols[&row.symbol].0).is_some()
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
            node.symbol.clone(),
            namespace
                .instantiate(bank, &media, &sizes)
                .map_err(|error| invalid(format!("Graph audio linking: {error:#}")))?,
        );
    }
    for node in nodes.iter().filter(|node| !node.audio.events.is_empty()) {
        let payload = bound
            .entry(node.symbol.clone())
            .or_insert_with(|| node.payload.clone());
        for event in &node.audio.events {
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

//! Exact texture-pair sharing within one world. Allocated entries keep their ordinals, while
//! symbolic references and loading roots name the first identical header and data pair.
use super::*;
use sha2::{Digest, Sha256};

pub(in crate::map) type Textures = BTreeMap<String, [TagHash; 2]>;

/// Both templates and complete payloads participate in identity. A header with additional
/// fixups, streamed backing or nonreciprocal data ownership is deliberately not shared.
pub(super) fn identify(
    nodes: &mut [Value],
    folder: &Path,
    manager: &PackageManager,
) -> AuthoringResult<()> {
    let symbols = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            node["symbol"]
                .as_str()
                .map(|symbol| (symbol.to_owned(), index))
                .ok_or_else(|| invalid("Map texture node has no symbol"))
        })
        .collect::<AuthoringResult<BTreeMap<_, _>>>()?;
    for index in 0..nodes.len() {
        parhelion_import::cancellation::check().map_err(|e| invalid(e.to_string()))?;
        let template = nodes[index]["template"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| invalid("Map texture node has no template"))?;
        let header = manager
            .get_entry(TagHash(template))
            .ok_or_else(|| invalid("Map texture template disappeared"))?;
        if (header.file_type, header.file_subtype) != (32, 1) {
            continue;
        }
        let Some(data_index) = nodes[index]["reference"]
            .as_str()
            .and_then(|symbol| symbols.get(symbol))
            .copied()
        else {
            continue;
        };
        let pair = [index, data_index];
        if nodes[data_index]["reference"] != nodes[index]["symbol"]
            || pair.iter().any(|&at| {
                !nodes[at]["storage"].is_null()
                    || nodes[at]["patches"]
                        .as_array()
                        .is_some_and(|patches| !patches.is_empty())
            })
        {
            continue;
        }
        let data_template = nodes[data_index]["template"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| invalid("Map texture data has no template"))?;
        let data = manager
            .get_entry(TagHash(data_template))
            .ok_or_else(|| invalid("Map texture data template disappeared"))?;
        if (data.file_type, data.file_subtype) != (40, 1) {
            continue;
        }
        let header_file = nodes[index]["file"]
            .as_str()
            .ok_or_else(|| invalid("Map texture has no header file"))?;
        let header_bytes =
            std::fs::read(folder.join(header_file)).map_err(|e| invalid(e.to_string()))?;
        if header_bytes.len() != 40 || read_u32(&header_bytes, 36)? != u32::MAX {
            continue;
        }
        let mut digest = Sha256::new();
        digest.update(b"parhelion/map-texture-pair/1");
        for at in pair {
            let file = nodes[at]["file"]
                .as_str()
                .ok_or_else(|| invalid("Map texture has no payload"))?;
            let payload = crate::artifact::digest_file(&folder.join(file))
                .map_err(|e| invalid(e.to_string()))?;
            if at == data_index && u64::from(read_u32(&header_bytes, 0)?) != payload.byte_length {
                return Err(invalid("Map texture header and data length disagree"));
            }
            digest.update(nodes[at]["template"].as_u64().unwrap().to_le_bytes());
            digest.update(payload.byte_length.to_le_bytes());
            digest.update(payload.sha256.as_bytes());
        }
        let key = hex::encode(digest.finalize());
        for (role, at) in pair.into_iter().enumerate() {
            nodes[at]["texture_pair"] = serde_json::json!({"key": key, "role": role});
        }
    }
    Ok(())
}

pub(super) fn resolve(
    nodes: &[Value],
    entry: &impl Fn(usize) -> AuthoringResult<TagHash>,
    textures: &mut Textures,
) -> AuthoringResult<BTreeMap<usize, TagHash>> {
    let mut pairs = BTreeMap::<String, [Option<usize>; 2]>::new();
    for (index, node) in nodes.iter().enumerate() {
        let pair = &node["texture_pair"];
        if pair.is_null() {
            continue;
        }
        let key = pair["key"]
            .as_str()
            .filter(|key| key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| invalid("Map texture identity is malformed"))?;
        let role = pair["role"]
            .as_u64()
            .filter(|&role| role < 2)
            .ok_or_else(|| invalid("Map texture role is malformed"))? as usize;
        // A conversion may itself contain multiple identical pairs. Their reciprocal symbols
        // identify each local pair, while the shared key resolves all of them to one resource.
        let local = node["symbol"]
            .as_str()
            .ok_or_else(|| invalid("Map texture has no symbol"))?;
        let peer = node["reference"]
            .as_str()
            .ok_or_else(|| invalid("Map texture has no peer"))?;
        let local_key = format!("{key}/{}", if role == 0 { local } else { peer });
        let row = pairs.entry(local_key).or_default();
        if row[role].replace(index).is_some() {
            return Err(invalid("Map texture repeats a pair role"));
        }
    }
    let mut aliases = BTreeMap::new();
    // Resolve in allocated-entry order, so the first complete pair owns each shared identity.
    let mut pairs = pairs
        .into_iter()
        .map(|(key, pair)| match pair {
            [Some(header), Some(data)] => Ok((header, data, key[..64].to_owned())),
            _ => Err(invalid("Map texture pair is incomplete")),
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    pairs.sort_by_key(|(header, _, _)| *header);
    for (header, data, key) in pairs {
        let own = [entry(header)?, entry(data)?];
        let canonical = *textures.entry(key).or_insert(own);
        aliases.insert(header, canonical[0]);
        aliases.insert(data, canonical[1]);
    }
    Ok(aliases)
}

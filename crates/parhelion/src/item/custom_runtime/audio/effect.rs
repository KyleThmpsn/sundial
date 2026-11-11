//! The prepared firing flow that replaces a donor presentation's muzzle particles.
//!
//! The Reach importer rebuilds the donor's slot-0 effect flow around the source firing particles
//! and records it with its private instance allocation. Its particle system fields name nodes of
//! the imported asset group, so the flow is resolved once that group has its tags. The firing
//! presentation then binds it in its private copy of the slot entity, beside any private sounds.
//! A flow that gained a scalar input also names the slot entity's connection rows it was prepared
//! against and the rows it needs, and the slot's channel bank it reads may be a prepared owner of
//! its own.
use super::*;

/// One prepared owner, the slot entity that binds the stock owner it replaces, and the private
/// allocation its rebuilt records need.
pub(super) struct Prepared {
    pub slot: usize,
    pub entity: u32,
    pub owner: u32,
    payload: Vec<u8>,
    patches: Vec<(usize, String)>,
    pub allocation: (u32, Vec<u8>),
    /// The slot entity's stock connection rows and the rows that replace them.
    pub connections: Option<(Vec<u8>, Vec<u8>)>,
}

pub(in crate::item) struct FiringEffect {
    pub(super) presentation: Value,
    pub(super) owners: Vec<Prepared>,
}

fn node<'a>(section: &'a Value, symbol: &Value) -> AuthoringResult<&'a Value> {
    let symbol = symbol
        .as_str()
        .ok_or_else(|| invalid("Imported firing effect names no node"))?;
    section
        .as_array()
        .into_iter()
        .flatten()
        .find(|node| node["symbol"] == symbol)
        .ok_or_else(|| invalid(format!("Imported firing effect lacks node {symbol}")))
}

fn template(node: &Value) -> AuthoringResult<u32> {
    node["template"]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| invalid("Imported firing effect node lacks a template"))
}

/// The graph's prepared firing flows, or `None` when its firing effect installs nothing.
pub(in crate::item) fn load(graph: &Inputs) -> AuthoringResult<Option<FiringEffect>> {
    let effect = &graph.value()["firing_effect"];
    let Some(replacements) = effect.get("replacements") else {
        return Ok(None);
    };
    let presentation = effect["native_presentation"].clone();
    if !presentation.is_object() {
        return Err(invalid(
            "Imported firing effect lacks its native presentation",
        ));
    }
    let read = |node: &Value| -> AuthoringResult<Vec<u8>> {
        graph.read(
            node["file"]
                .as_str()
                .ok_or_else(|| invalid("Imported firing effect node lacks a file"))?,
        )
    };
    let mut owners = Vec::new();
    for row in replacements
        .as_array()
        .ok_or_else(|| invalid("Imported firing effect replacements are not an array"))?
    {
        let slot = row["slot"]
            .as_u64()
            .filter(|slot| *slot < 6)
            .ok_or_else(|| invalid("Imported firing effect slot is invalid"))?
            as usize;
        let entity = hex(&row["entity"], "firing effect entity")?;
        let owner = hex(&row["component_owner"], "firing effect owner")?;
        let flow = node(&effect["owners"], &row["owner"])?;
        let allocation = node(&effect["allocations"], &row["allocation"])?;
        if template(flow)? != owner {
            return Err(invalid("Imported firing flow replaces another owner"));
        }
        let mut patches = Vec::new();
        for patch in flow["patches"].as_array().into_iter().flatten() {
            let offset = patch["offset"]
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| invalid("Imported firing flow patch lacks an offset"))?;
            let symbol = patch["symbol"]
                .as_str()
                .ok_or_else(|| invalid("Imported firing flow patch lacks a symbol"))?;
            patches.push((offset, symbol.to_owned()));
        }
        let connections = row
            .get("connections")
            .map(|rows| -> AuthoringResult<(Vec<u8>, Vec<u8>)> {
                // A slot entity whose only rows fed the replaced flow keeps none afterward.
                let decode = |field: &str, empty: bool| {
                    rows[field]
                        .as_str()
                        .and_then(|text| ::hex::decode(text).ok())
                        .filter(|bytes| {
                            (empty || !bytes.is_empty()) && bytes.len().is_multiple_of(72)
                        })
                        .ok_or_else(|| {
                            invalid("Imported firing entity connection rows are invalid")
                        })
                };
                Ok((decode("before", false)?, decode("after", true)?))
            })
            .transpose()?;
        owners.push(Prepared {
            slot,
            entity,
            owner,
            payload: read(flow)?,
            patches,
            allocation: (template(allocation)?, read(allocation)?),
            connections,
        });
    }
    Ok((!owners.is_empty()).then_some(FiringEffect {
        presentation,
        owners,
    }))
}

impl Prepared {
    /// The rebuilt flow with each particle system field naming its allocated asset.
    pub(super) fn payload(&self, symbols: &BTreeMap<String, TagHash>) -> AuthoringResult<Vec<u8>> {
        let mut payload = self.payload.clone();
        for (offset, symbol) in &self.patches {
            let tag = symbols.get(symbol).ok_or_else(|| {
                invalid(format!(
                    "Imported firing particle {symbol} was not allocated"
                ))
            })?;
            if word(&payload, *offset)? != u32::MAX {
                return Err(invalid("Imported firing particle field is already bound"));
            }
            put(&mut payload, *offset, tag.0)?;
        }
        Ok(payload)
    }
}

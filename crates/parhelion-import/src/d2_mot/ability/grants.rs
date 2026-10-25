//! Modern investment ability grants and their source entity roots.
//!
//! The paired catalogs establish source row identity. Grant flags, conditions and property
//! consumers remain source evidence, not a native ability slot or an equipping decision.

use super::source::tag;
use crate::d2_mot::{gameplay, gameplay::perks, payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize)]
pub struct Catalog {
    pub definition_tag: String,
    pub identity_tag: String,
    pub identities: Vec<u32>,
    /// Complete 28-byte definition rows, preserving fields beyond identity and assignment key.
    pub rows: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Assignment {
    pub key: u32,
    pub target: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Evidence {
    pub source_offset: u64,
    /// Complete 40-byte grant row. Relative fields still address the retained item payload.
    pub source_record: String,
    pub assignment: Option<Assignment>,
    pub entity: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct Plan {
    pub source_index: u32,
    pub source_identity: u32,
    pub assignment_key: u32,
    pub property_key: u32,
    pub slot: u8,
    /// Observed separately from the slot byte. Its runtime meaning remains unresolved.
    pub flag_byte: u8,
    pub unresolved: Option<String>,
    pub evidence: Evidence,
}

fn record(value: &str, size: usize) -> Result<Payload> {
    let bytes = hex::decode(value).context("Source ability record encoding")?;
    ensure!(bytes.len() == size, "Source ability record size differs");
    Ok(Payload(bytes))
}

impl Catalog {
    pub(crate) fn validate(&self) -> Result<()> {
        tag(&self.definition_tag)?;
        tag(&self.identity_tag)?;
        ensure!(
            !self.rows.is_empty() && self.rows.len() == self.identities.len(),
            "Source ability catalogs have empty or unequal populations"
        );
        for (row, identity) in self.rows.iter().zip(&self.identities) {
            ensure!(
                record(row, 28)?.u32(0)? == *identity,
                "Source ability catalog identity differs"
            );
        }
        Ok(())
    }

    fn definition(&self, index: u32) -> Result<Payload> {
        record(
            self.rows
                .get(index as usize)
                .context("Ability grant index outside catalog")?,
            28,
        )
    }
}

pub(crate) fn plan(trace: &Value, catalog: Option<&Catalog>) -> Result<Option<Vec<Plan>>> {
    let Some(values) = trace.get("ability_grants") else {
        // Older captures did not inspect this resource. Absence does not mean no grants.
        return Ok(None);
    };
    let catalog = catalog.context("Ability grants have no paired source catalogs")?;
    let mut result = Vec::new();
    for value in values.as_array().context("Source ability grants")? {
        let evidence: Evidence = serde_json::from_value(value.clone())?;
        let source = record(&evidence.source_record, 40)?;
        let index = source.u32(0)?;
        let definition = catalog.definition(index)?;
        let key = definition.u32(4)?;
        let unresolved = match (&evidence.assignment, &evidence.entity) {
            (Some(assignment), Some(entity)) => {
                ensure!(
                    assignment.key == key,
                    "Ability assignment key differs from its definition"
                );
                ensure!(
                    tag(&assignment.target)?
                        == tag(entity["tag"].as_str().context("Ability entity tag")?)?,
                    "Ability entity differs from its assignment"
                );
                entity["components"]
                    .as_array()
                    .context("Ability entity components")?;
                None
            }
            (None, None) => Some("No standalone runtime assignment".to_owned()),
            _ => anyhow::bail!("Ability assignment and entity must be captured together"),
        };
        result.push(Plan {
            source_index: index,
            source_identity: definition.u32(0)?,
            assignment_key: key,
            property_key: source.u32(4)?,
            slot: source.u8(8)?,
            flag_byte: source.u8(9)?,
            unresolved,
            evidence,
        });
    }
    Ok(Some(result))
}

pub(crate) fn catalog(reader: &mut Reader) -> Result<Catalog> {
    let definitions = reader.classes(0x80805439);
    let identities = reader.classes(0x808076AF);
    ensure!(
        definitions.len() == 1 && identities.len() == 1,
        "Missing or ambiguous source ability catalogs"
    );
    let definition = reader.tag(definitions[0], Some(0x80805439))?;
    let identity = reader.tag(identities[0], Some(0x808076AF))?;
    let result = Catalog {
        definition_tag: format!("{:08X}", definitions[0]),
        identity_tag: format!("{:08X}", identities[0]),
        identities: identity
            .array(8, 4, Some(0x808076B4))?
            .into_iter()
            .map(|at| identity.u32(at))
            .collect::<Result<_>>()?,
        rows: definition
            .array(8, 28, Some(0x8080543D))?
            .into_iter()
            .map(|at| Ok(hex::encode(definition.bytes::<28>(at)?)))
            .collect::<Result<_>>()?,
    };
    result.validate()?;
    Ok(result)
}

pub(crate) fn capture(
    reader: &mut Reader,
    item: &Payload,
    catalog: &Catalog,
) -> Result<Vec<Evidence>> {
    if item.u64(0x68)? == 0 {
        return Ok(Vec::new());
    }
    let block = gameplay::resource(item, 0x68, 0x80807381)?;
    let mut result = Vec::new();
    for at in item.array(block + 0x20, 40, Some(0x80807385))? {
        crate::cancellation::check()?;
        let definition = catalog.definition(item.u32(at)?)?;
        let key = definition.u32(4)?;
        let (assignment, entity) = match perks::action(reader, key)? {
            Some(target) => (
                Some(Assignment {
                    key,
                    target: format!("{target:08X}"),
                }),
                Some(perks::entity(reader, target)?),
            ),
            None => (None, None),
        };
        result.push(Evidence {
            source_offset: at as u64,
            source_record: hex::encode(item.bytes::<40>(at)?),
            assignment,
            entity,
        });
    }
    Ok(result)
}

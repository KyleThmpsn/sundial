//! Asset profiles deliberately exclude enemy behavior and never install content.
use crate::d2_mot::{
    profile::hash,
    reader::{Reader, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};
pub(crate) mod item;
mod model;
mod shader;

pub fn validate(p: &Value) -> Result<&str> {
    let object = p.as_object().context("asset profile must be an object")?;
    for key in object.keys() {
        ensure!(
            [
                "kind",
                "source_item",
                "source_entity",
                "source_model",
                "native_item",
                "dye_key_base"
            ]
            .contains(&key.as_str()),
            "unknown asset field {key}"
        );
    }
    let kind = p["kind"].as_str().context("kind is required")?;
    ensure!(
        [
            "weapon", "ornament", "prop", "ghost", "ship", "sparrow", "armor", "shader"
        ]
        .contains(&kind),
        "unsupported kind {kind}; enemies are out of scope"
    );
    let selectors = ["source_item", "source_entity", "source_model"]
        .into_iter()
        .filter(|k| p.get(*k).is_some())
        .collect::<Vec<_>>();
    ensure!(
        selectors.len() == 1,
        "exactly one source selector is required"
    );
    hash(p, selectors[0])?;
    if kind == "shader" {
        ensure!(selectors[0] == "source_item", "shader requires source_item");
        hash(p, "native_item")?;
        hash(p, "dye_key_base")?;
    } else {
        ensure!(
            p.get("native_item").is_none() && p.get("dye_key_base").is_none(),
            "native dye fields require shader kind"
        );
    }
    Ok(kind)
}
pub fn export(profile: &Path, modern: &Path, native: &Path, out: &Path) -> Result<Value> {
    let p: Value = serde_json::from_slice(&fs::read(profile)?)?;
    let kind = validate(&p)?.to_owned();
    let out = crate::d2_mot::reader::outside(
        &crate::d2_mot::reader::outside(out, modern.parent().context("modern parent")?)?,
        native.parent().context("native parent")?,
    )?;
    if kind == "shader" {
        return shader::export(&p, modern, native, &out);
    }
    let mut r = Reader::new(modern, &out, true)?;
    let result = (|| -> Result<Value> {
        let mut presentation = Value::Null;
        let mut entities = Vec::new();
        let mut direct = None;
        if p.get("source_item").is_some() {
            presentation = item::presentation(&mut r, hash(&p, "source_item")?)?;
            entities = presentation["entities"]
                .as_array()
                .context("entities")?
                .iter()
                .map(|v| v.as_u64().map(|v| v as u32).context("entity"))
                .collect::<Result<_>>()?;
        } else if p.get("source_entity").is_some() {
            entities.push(hash(&p, "source_entity")?);
        } else {
            direct = Some(hash(&p, "source_model")?);
        }
        let content = model::export(&mut r, &entities, direct)?;
        Ok(
            json!({"schema":1,"kind":kind,"classification":"profile supplied; not an inventory type assertion","profile":p,"presentation":presentation,"content":content,"installed":false,"shadowkeep_ready":false,"remaining":["native material and skeleton conversion for this asset family","family-specific native registration and attachment mapping","in-game validation"]}),
        )
    })();
    r.finish()?;
    match result {
        Ok(report) => {
            write_json(&out.join("asset.json"), &report)?;
            Ok(report)
        }
        Err(error) => {
            write_json(
                &out.join("asset.json"),
                &json!({"kind":kind,"status":"blocked","error":format!("{error:#}"),"installed":false,"shadowkeep_ready":false}),
            )?;
            Err(error)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_enemies_ambiguous_sources_and_unknown_fields() {
        assert!(validate(&json!({"kind":"enemy","source_model":"80C00001"})).is_err());
        assert!(
            validate(&json!({"kind":"prop","source_model":"80C00001","source_item":1})).is_err()
        );
        assert!(validate(&json!({"kind":"armor","source_item":1,"install":true})).is_err());
        assert!(validate(&json!({"kind":"shader","source_item":1})).is_err());
        for kind in [
            "weapon", "ornament", "prop", "ghost", "ship", "sparrow", "armor",
        ] {
            assert!(validate(&json!({"kind":kind,"source_item":1})).is_ok());
        }
    }
}

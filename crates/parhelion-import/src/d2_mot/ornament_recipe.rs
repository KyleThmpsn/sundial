//! Add a declared ornament to a copy of an existing authored weapon recipe.
use crate::d2_mot::{
    profile::hash,
    reader::{outside, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};
pub fn prepare(
    source: &Path,
    profile: &Path,
    output: &Path,
    modern: Option<&Path>,
) -> Result<Value> {
    let output = outside(output, source.parent().context("recipe parent")?)?;
    let output = if let Some(packages) = modern {
        outside(
            &output,
            packages.parent().context("modern packages parent")?,
        )?
    } else {
        output
    };
    let p: Value = serde_json::from_slice(&fs::read(profile)?)?;
    let link = p.get("ornament").context("profile is not an ornament")?;
    let localized = if let Some(packages) = modern {
        let mut r =
            crate::d2_mot::reader::Reader::new(packages, &output.join("localization"), true)?;
        let source_hash = hash(&p, "source_item")?;
        let (index, _) = crate::d2_mot::assets::item::find(&mut r, source_hash)?;
        let result = crate::d2_mot::localization::item_name(&mut r, source_hash, index, 0)?;
        write_json(&r.output.join("name.json"), &result)?;
        r.finish()?;
        Some(result)
    } else {
        None
    };
    let name = localized
        .as_ref()
        .and_then(|v| v["name"].as_str())
        .or_else(|| link["name"].as_str())
        .context(
            "provide --modern-packages to resolve the ornament name, or an explicit ornament.name",
        )?;
    ensure!(!name.trim().is_empty(), "ornament name is empty");
    let mut recipe: Value = serde_json::from_slice(&fs::read(source)?)?;
    ensure!(
        hash(&recipe["identity"], "item_hash")? == hash(link, "target_weapon")?,
        "ornament owner does not match recipe"
    );
    let socket = link["target_socket"].as_u64().context("target socket")? as usize;
    let donor = hash(link, "native_plug")?;
    let native_type = link["native_socket_type"]
        .as_u64()
        .context("native socket type")?;
    let namespace = recipe["namespace"]
        .as_str()
        .context("recipe namespace")?
        .to_owned();
    let columns = recipe["overrides"]["socket_columns"]
        .as_array_mut()
        .context("recipe socket columns")?;
    let column = columns
        .get_mut(socket)
        .context("target socket outside recipe")?;
    let choices = column["choices"]
        .as_array_mut()
        .context("ornament socket must be explicitly configured")?;
    ensure!(
        choices.first().is_some_and(|v| v == "0xAEBAE371"),
        "ornament socket must start with default appearance reset"
    );
    ensure!(
        !choices
            .iter()
            .any(|v| v == &json!(format!("0x{donor:08X}"))),
        "native plug already selected; provide another native ornament donor for each choice"
    );
    let choice = choices.len();
    let expected = format!("parhelion/{namespace}/socket/{socket}/choice/{choice}/private-plug")
        .bytes()
        .fold(0x811C9DC5u32, |h, b| {
            h.wrapping_mul(16777619) ^ u32::from(b)
        });
    ensure!(
        expected == hash(&p, "target_item")?,
        "profile target must match private choice identity {expected:08X}"
    );
    choices.push(json!(format!("0x{donor:08X}")));
    column["socket_type"] = json!(native_type);
    column["reusable_plug_set_index"] = Value::Null;
    column["randomized_plug_set_index"] = Value::Null;
    column["choice_weight_bits"] = json!([]);
    column["choice_conditions"] = json!([]);
    column["randomized_selection_program"] = json!([]);
    let variants = recipe["overrides"]["socket_plug_variants"]
        .as_array_mut()
        .context("recipe private variants")?;
    ensure!(
        !variants
            .iter()
            .any(|v| v["socket_index"] == socket && v["choice_index"] == choice),
        "ornament variant position is occupied"
    );
    variants.push(json!({"socket_index":socket,"choice_index":choice,"source_plug_hash":format!("0x{donor:08X}"),
        "name":name,"description":"Changes this weapon's appearance. Select Default Ornament to restore the original appearance.","sandbox_perks":[]}));
    fs::create_dir_all(&output)?;
    let path = output.join(source.file_name().context("recipe filename")?);
    write_json(&path, &recipe)?;
    Ok(
        json!({"recipe":path,"ornament":expected,"name":name,"localization":localized,"socket":socket,"choice":choice,"installed":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authoring_preserves_source_and_rejects_cross_weapon_links() {
        let dir = tempfile::tempdir().unwrap();
        let source_dir = dir.path().join("source");
        fs::create_dir(&source_dir).unwrap();
        let source = source_dir.join("weapon.json");
        let recipe = json!({"namespace":"parhelion.ergo-same","identity":{"item_hash":"0x50EE7278"},
            "overrides":{"socket_columns":[null,null,null,null,null,null,null,{"choices":["0xAEBAE371"]}],"socket_plug_variants":[]}});
        write_json(&source, &recipe).unwrap();
        let original = fs::read(&source).unwrap();
        let profile = dir.path().join("profile.json");
        let mut p = json!({"target_item":3848808219u32,"ornament":{"target_weapon":1357804152u32,"target_socket":7,"native_plug":2117927723u32,"native_socket_type":500,"name":"Test ornament"}});
        write_json(&profile, &p).unwrap();
        assert!(prepare(&source, &profile, &source_dir.join("output"), None).is_err());
        assert!(!source_dir.join("output").exists());
        prepare(&source, &profile, &dir.path().join("valid"), None).unwrap();
        assert_eq!(fs::read(&source).unwrap(), original);
        let result: Value =
            serde_json::from_slice(&fs::read(dir.path().join("valid/weapon.json")).unwrap())
                .unwrap();
        assert_eq!(
            result["overrides"]["socket_columns"][7]["choices"],
            json!(["0xAEBAE371", "0x7E3D032B"])
        );
        p["ornament"]["target_weapon"] = json!(42);
        write_json(&profile, &p).unwrap();
        assert!(prepare(&source, &profile, &dir.path().join("wrong-owner"), None).is_err());
        assert!(!dir.path().join("wrong-owner").exists());
    }
}

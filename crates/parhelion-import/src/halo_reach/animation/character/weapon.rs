//! Qualified Reach melee classes share the native sword character controller.
use super::*;
use crate::halo_reach::Scene;

pub(in crate::halo_reach) fn defaults(
    scene: &Scene,
    native: &mut Reader,
    donor: u32,
) -> Result<Option<Options>> {
    if scene.object.tag.group != "weap" {
        return Ok(None);
    }
    let Some(class @ ("sword" | "hammer")) = scene.object.gameplay["weapon_class"].as_str() else {
        return Ok(None);
    };
    let item = crate::tiger::shadowkeep::extract(native, donor)?;
    let item_tag = u32::from_str_radix(item["item_tag"].as_str().context("Native item tag")?, 16)?;
    let report = crate::tiger::rig::inspect(native, item_tag, false)?;
    let runtime = u32::from_str_radix(
        report["runtime_entity"]
            .as_str()
            .context("Weapon runtime")?,
        16,
    )?;
    let content = u32::from_str_radix(
        report["content_key"].as_str().context("Weapon content")?,
        16,
    )?;
    let entity = native.tag(runtime, Some(0x80809C0F))?;
    let bindings =
        sundial::package_authoring::entity::weapon_component_bindings(&entity.0, 0xD3A5500E)
            .map_err(anyhow::Error::msg)?;
    let [binding] = bindings.as_slice() else {
        anyhow::bail!("Melee import requires one native weapon attachment")
    };
    let owner = native.tag(binding.owner_tag, Some(0x80809C36))?;
    let definition = owner.pointer(24)?;
    ensure!(
        owner.u32(definition - 4)? == 0x80804221,
        "Unknown melee attachment layout"
    );
    let rows = crate::tiger::rig::attachment_rows(&owner, definition, content, false)?;
    for row in rows {
        ensure!(
            owner.u32(row + 0x68)? == 0x19EC2C8B && owner.u32(row + 0x6C)? == name("sword"),
            "Reach melee poses require a native sword attachment profile"
        );
    }
    let clip = |mode: &str, action: &str| Action::Clip(format!("{mode}:{class}:{action}"));
    let mut actions = BTreeMap::from([
        ("ready".into(), clip("combat", "ready")),
        ("put_away".into(), clip("combat", "put_away")),
    ]);
    // The native sword's aerial and grounded profile lists name these states.
    // Their human bank routes corroborate the first air and ground light roles.
    for (state, action) in [
        ("0x054A16BE", "melee_lunge_airborne"),
        ("0x06B623AD", "melee_lunge_airborne"),
        ("0xC5C3F7A0", "melee_strike_1"),
        ("0xC5C3F7A3", "melee_strike_2"),
        ("0xCE9A511C", "melee_strike_2"),
        ("0xCE9A511D", "melee_strike_1"),
        ("0xCE9A511F", "melee_strike_1"),
        ("0xE9D650B6", "melee_lunge"),
    ] {
        actions.insert(state.into(), clip("combat", action));
    }
    Ok(Some(Options {
        graph: "objects\\characters\\spartans\\spartans".into(),
        model: "objects\\characters\\spartans\\spartans".into(),
        reference: format!("combat:{class}:idle"),
        actions,
        stance: BTreeMap::from([
            ("stand".into(), clip("combat", "idle")),
            ("crouch".into(), clip("crouch", "idle")),
        ]),
        rider: None,
        bone_aliases: BTreeMap::new(),
        native: Controller {
            skeleton: 0x80C22410,
            controls: 0x80BFCB38,
            lookup: 0x80EF62E5,
            pose: 0x8161D5F7,
        },
    }))
}

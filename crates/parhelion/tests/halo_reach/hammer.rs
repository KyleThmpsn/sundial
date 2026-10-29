//! Follow emitted sword contact and timed-effect references against a source witness.
mod audio;
use super::*;
use std::collections::BTreeSet;
use sundial::package_authoring::entity::weapon_component_bindings;

fn number(value: &Value) -> Result<f32> {
    let result = value.as_f64().context("Hammer source witness value")? as f32;
    ensure!(result.is_finite(), "Nonfinite Hammer witness");
    Ok(result)
}

fn close(actual: f32, expected: f32) -> Result<()> {
    ensure!(
        (actual - expected).abs() <= 0.0001 * expected.abs().max(1.),
        "Hammer value {actual} differs from source witness {expected}"
    );
    Ok(())
}

fn banks(owner: &Payload) -> Result<Vec<usize>> {
    let mut rows = BTreeSet::new();
    for descriptor in (0..owner.0.len().saturating_sub(16)).step_by(8) {
        if owner.u64(descriptor)? == 0 || owner.u64(descriptor)? > 128 {
            continue;
        }
        let Ok(header) = owner.pointer(descriptor + 8) else {
            continue;
        };
        if header < 4
            || owner.u32(header - 4).ok() != Some(0x80809fbd)
            || owner.u64(header + 8).ok() != Some(0x80802d7b)
        {
            continue;
        }
        rows.extend(owner.array(descriptor, 0x220, Some(0x80802d7b))?);
    }
    ensure!(!rows.is_empty(), "Sword has no attack profiles");
    Ok(rows.into_iter().collect())
}

fn amount(reader: &mut Reader, stock: &Reader, tag: u32, expected: f32) -> Result<Payload> {
    ensure!(
        stock.reference(tag).is_err(),
        "Hammer modifies a stock damage profile"
    );
    let p = reader.tag(tag, Some(0x80809c36))?;
    let d = p.pointer(24)?;
    ensure!(
        p.u32(d - 4)? == 0x80804b02 && p.u8(d + 0x10)? == 0,
        "Hammer damage profile is not absolute"
    );
    close(p.f32(d + 0x14)?, expected)?;
    close(p.f32(d + 0x18)?, expected)?;
    close(p.f32(d + 0x1c)?, 0.)?;
    Ok((*p).clone())
}

pub(super) fn verify(
    staged: &mut Reader,
    stock: &mut Reader,
    runtime: &Value,
    graph: &Value,
    witness: &Value,
    directory: &Path,
    output: &Path,
) -> Result<()> {
    if graph.get("sword").is_none() && witness.is_null() {
        return Ok(());
    }
    ensure!(
        !witness.is_null() && graph["sword"]["installable"] == true,
        "Hammer requires both an independent source witness and emitted mechanics"
    );
    let tag = u32::from_str_radix(
        runtime["runtime_entity"]
            .as_str()
            .context("Hammer runtime")?,
        16,
    )?;
    let entity = staged.tag(tag, Some(0x80809c0f))?;
    let binding = weapon_component_bindings(&entity.0, 0xcd2bceac).map_err(anyhow::Error::msg)?;
    let [binding] = binding.as_slice() else {
        anyhow::bail!("Hammer sword binding is ambiguous")
    };
    ensure!(
        stock.reference(binding.owner_tag).is_err(),
        "Hammer uses a stock sword owner"
    );
    let owner = staged.tag(binding.owner_tag, Some(0x80809c36))?;
    let original_tag = u32::try_from(
        graph["sword"]["source_owner"]
            .as_u64()
            .context("Source owner")?,
    )?;
    let original = stock.tag(original_tag, Some(0x80809c36))?;
    ensure!(
        staged.tag(original_tag, Some(0x80809c36))?.0 == original.0,
        "Hammer installation changed its stock donor"
    );
    let mut effects = BTreeSet::new();
    let mut profiles = Vec::new();
    let mut powered = 0;
    let mut depleted = 0;
    for row in banks(&original)? {
        let gates = original.array(row + 0x18, 8, Some(0x80802d6e))?;
        let uses_ammo = gates.iter().any(|&g| {
            original
                .pointer(g)
                .ok()
                .and_then(|p| original.u32(p.saturating_sub(4)).ok())
                == Some(0x80802d70)
        });
        ensure!(
            owner.u64(row + 0x18)? == original.u64(row + 0x18)?,
            "Hammer changes attack eligibility"
        );
        for gate in gates {
            let p = original.pointer(gate)?;
            ensure!(
                owner.bytes::<12>(p)? == original.bytes::<12>(p)?,
                "Hammer changes a native attack gate"
            );
        }
        let contact = owner.pointer(row + 0x1a8)?;
        ensure!(
            owner.u32(contact - 4)? == 0x80804455,
            "Hammer contact type differs"
        );
        let damage = owner.u32(contact + 0x28)?;
        amount(
            staged,
            stock,
            damage,
            number(
                &witness[if uses_ammo {
                    "powered_contact"
                } else {
                    "depleted_contact"
                }],
            )?,
        )?;
        let effect = owner.u32(row + 0x198)?;
        if uses_ammo {
            powered += 1;
            ensure!(
                stock.reference(effect).is_err(),
                "Powered Hammer has no private timed blast"
            );
            effects.insert(effect);
        } else {
            depleted += 1;
            ensure!(
                effect == u32::MAX,
                "Depleted Hammer retains a powered blast"
            );
        }
        profiles.push(
            json!({"action":format!("{:08X}",owner.u32(row)?),"powered":uses_ammo,
            "contact":format!("{damage:08X}"),"effect":format!("{effect:08X}")}),
        );
    }
    ensure!(
        powered > 0 && depleted > 0 && effects.len() == 1,
        "Hammer lacks powered/depleted coverage"
    );
    let blast_tag = *effects.first().unwrap();
    let blast = staged.tag(blast_tag, Some(0x80809c0f))?;
    let mut queries = Vec::new();
    let mut sounds = Vec::new();
    for component in blast.array(16, 12, Some(0x80809c04))? {
        let p = staged.tag(blast.u32(component)?, Some(0x80809c36))?;
        let d = p.pointer(24)?;
        if p.u32(d - 4)? != 0x808084e9 {
            continue;
        }
        ensure!(
            p.u8(d + 0x2e0)? & 2 != 0,
            "Hammer effect does not retire at root completion"
        );
        for row in p.array(d + 0x168, 24, Some(0x808093e6))? {
            let q = p.pointer(row + 16)?;
            if p.u32(q - 4)? == 0x80806b38 && witness.get("impact_audio").is_some() {
                sounds.push(audio::verify(
                    staged,
                    stock,
                    p.u32(q + 0x40)?,
                    directory,
                    &witness["impact_audio"],
                )?);
            }
            if p.u32(q - 4)? != 0x80804ab9 {
                continue;
            }
            let shape = p.pointer(q + 0x48)?;
            ensure!(
                p.u32(shape - 4)? == 0x808092b1 && p.u32(shape)? == 2,
                "Hammer query is not spherical"
            );
            let actions = p.array(q + 0x178, 0x58, Some(0x80804abf))?;
            ensure!(
                actions.len() == 1,
                "Hammer radial query has ambiguous actions"
            );
            let a = p.pointer(actions[0] + 0x50)?;
            ensure!(
                p.u32(a - 4)? == 0x80804ac9,
                "Hammer query has another action type"
            );
            let profile_tag = p.u32(a + 8)?;
            let damage = staged.tag(profile_tag, Some(0x80809c36))?;
            let dd = damage.pointer(24)?;
            let is_push = damage.f32(dd + 0x14)? == 0.;
            amount(
                staged,
                stock,
                profile_tag,
                if is_push {
                    0.
                } else {
                    number(&witness["blast_damage"])?
                },
            )?;
            close(
                p.f32(shape + 4)?,
                number(
                    &witness[if is_push {
                        "push_radius"
                    } else {
                        "damage_radius"
                    }],
                )?,
            )?;
            let responses = damage.array(dd + 0x78, 8, None)?;
            let push = responses
                .into_iter()
                .filter_map(|r| damage.pointer(r).ok())
                .filter(|&r| damage.u32(r.saturating_sub(4)).ok() == Some(0x80804b57))
                .collect::<Vec<_>>();
            ensure!(
                push.len() == usize::from(is_push),
                "Damage and push responses are not separated"
            );
            if let Some(&r) = push.first() {
                close(
                    damage.f32(r + 4)?,
                    number(&witness["native_push_strength"])?,
                )?;
                close(
                    damage.f32(r + 8)?,
                    number(&witness["native_push_strength"])?,
                )?;
            }
            queries.push(json!({"push":is_push,"radius":p.f32(shape+4)?,"profile":format!("{profile_tag:08X}")}));
        }
    }
    ensure!(
        queries.len() == 2 && queries.iter().filter(|q| q["push"] == true).count() == 1,
        "Hammer is missing either radial damage or recipient push"
    );
    if witness.get("impact_audio").is_some() {
        ensure!(
            sounds.len() == 1,
            "Hammer lacks one timed source impact sound"
        );
    }
    fs::write(
        output,
        serde_json::to_vec_pretty(&json!({"witness":witness,"profiles":profiles,
        "queries":queries,"sounds":sounds,"effect":format!("{blast_tag:08X}"),"gameplay_verified":false}))?,
    )?;
    Ok(())
}

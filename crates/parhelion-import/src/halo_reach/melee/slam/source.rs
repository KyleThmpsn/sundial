//! Qualify distinct source contact, radial health and radial motion profiles.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

fn number(value: &Value) -> Result<f32> {
    let number = value.as_f64().context("Melee source scalar")? as f32;
    ensure!(number.is_finite(), "Nonfinite melee source scalar");
    Ok(number)
}

fn linked<'a>(behavior: &'a Value, owner: &Value, role: &str, group: &str) -> Result<&'a Value> {
    let edges = behavior["edges"].as_array().context("Melee source edges")?;
    let matching = edges
        .iter()
        .filter(|e| e["owner"] == *owner && e["role"] == role && e["tag"]["group"] == group)
        .collect::<Vec<_>>();
    let [edge] = matching.as_slice() else {
        anyhow::bail!("Melee needs one {role} {group} reference")
    };
    let id = edge["tag"]["datum"]
        .as_u64()
        .context("Melee source identity")?;
    let node = &behavior["nodes"][format!("{id:08X}")];
    ensure!(
        node["tag"] == edge["tag"],
        "Melee source reference is unresolved"
    );
    Ok(node)
}

fn constant(node: &Value) -> Result<f32> {
    let d = &node["data"];
    let amount = number(&d["lower_bound"])?;
    ensure!(
        amount > 0.
            && d["radius"] == json!([0., 0.])
            && d["upper_bound"].as_array().is_some_and(|v| v.len() == 2
                && v.iter()
                    .all(|n| n.as_f64().is_some_and(|n| n as f32 == amount))),
        "Melee contact requires a positive constant source damage profile"
    );
    Ok(amount)
}

#[derive(Clone, Copy)]
pub(super) struct Radial<'a> {
    pub(super) source: &'a Value,
    pub(super) radius: [f32; 2],
    pub(super) amount: f32,
    pub(super) acceleration: f32,
}

impl<'a> Radial<'a> {
    fn read(source: &'a Value) -> Result<Self> {
        let d = &source["data"];
        let radius = [number(&d["radius"][0])?, number(&d["radius"][1])?];
        let amount = number(&d["upper_bound"][0])?;
        let acceleration = number(&d["instantaneous_acceleration"])?;
        ensure!(
            radius[0] >= 0.
                && radius[1] > radius[0]
                && amount >= 0.
                && acceleration >= 0.
                && number(&d["lower_bound"])? == 0.
                && number(&d["upper_bound"][1])? == amount,
            "Radial melee needs deterministic source bounds"
        );
        Ok(Self {
            source,
            radius,
            amount,
            acceleration,
        })
    }
}

pub(super) struct Source<'a> {
    pub powered: &'a Value,
    pub depleted: &'a Value,
    pub effect: &'a Value,
    pub blast: Radial<'a>,
    pub push: Radial<'a>,
    pub powered_amount: f32,
    pub depleted_amount: f32,
    pub sound: Option<&'a Value>,
}

impl<'a> Source<'a> {
    pub fn read(behavior: &'a Value, source_audio: bool) -> Result<Self> {
        let root_id = &behavior["root"]["datum"];
        let contact = linked(behavior, root_id, "lunge_damage", "jpt!")?;
        let empty = linked(behavior, root_id, "empty_melee_damage", "jpt!")?;
        let effect = linked(behavior, root_id, "lunge_explosive_damage", "effe")?;
        let mut parts = Vec::new();
        for edge in behavior["edges"].as_array().context("Melee effect links")? {
            if edge["owner"] == effect["tag"]["datum"]
                && edge["role"] == "effect_part"
                && edge["tag"]["group"] == "jpt!"
            {
                let id = edge["tag"]["datum"]
                    .as_u64()
                    .context("Radial source identity")?;
                parts.push(Radial::read(&behavior["nodes"][format!("{id:08X}")])?);
            }
        }
        ensure!(
            parts.len() == 2,
            "Melee blast needs separate damage and impulse parts"
        );
        let blast = *parts
            .iter()
            .find(|p| p.amount > 0. && p.acceleration == 0.)
            .context("Source radial health part")?;
        let push = *parts
            .iter()
            .find(|p| p.amount == 0. && p.acceleration > 0.)
            .context("Source radial impulse part")?;
        Ok(Self {
            powered: contact,
            depleted: empty,
            effect,
            blast,
            push,
            powered_amount: constant(contact)?,
            depleted_amount: constant(empty)?,
            sound: source_audio
                .then(|| super::audio::source(behavior, effect))
                .transpose()?,
        })
    }
}

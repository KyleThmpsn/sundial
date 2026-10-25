//! Translate source trajectory scalars through the checked native paired fields.
use crate::tiger::payload::Payload;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use sundial::package_authoring::{
    entity::weapon_component_bindings,
    sandbox_perk::entity::projectile::parameters::{Kind, Stored},
};

pub(in crate::halo_reach) fn apply(
    entity: &Payload,
    owners: &mut BTreeMap<u32, Payload>,
    source: &Value,
    metres: f32,
    source_trajectory: bool,
) -> Result<Value> {
    let scalar = |name: &str| -> Result<f32> {
        let value = source[name]
            .as_f64()
            .with_context(|| format!("Source projectile {name}"))? as f32;
        ensure!(
            value.is_finite() && value >= 0.,
            "Invalid source projectile {name}"
        );
        Ok(value)
    };
    ensure!(
        metres.is_finite() && metres > 0.,
        "Invalid projectile distance scale"
    );
    let gravity = scalar("air_gravity_scale")?;
    let distance = scalar("maximum_range")? * metres;
    let bindings = weapon_component_bindings(&entity.0, 0x0437756d).map_err(anyhow::Error::msg)?;
    let mut changes = Vec::new();
    for binding in bindings {
        if binding.concrete_class != 0x80803b73 {
            continue;
        }
        let payload = owners
            .get_mut(&binding.owner_tag)
            .context("Projectile movement owner is absent")?;
        let instance = usize::try_from(binding.resource_offset)?;
        for (kind, value) in [(Kind::Gravity, gravity), (Kind::TravelDistance, distance)] {
            let stored = Stored::read(kind, &payload.0, binding.owner_tag, instance)
                .map_err(anyhow::Error::msg)?;
            let original = stored.original();
            stored
                .write(&mut payload.0, value)
                .map_err(anyhow::Error::msg)?;
            changes.push(json!({"owner":binding.owner_tag,"instance":instance,
                "kind":kind,"original":original,"value":value}));
        }
        if source_trajectory {
            let speed = scalar("initial_velocity")? * metres;
            Stored::read(Kind::Speed, &payload.0, binding.owner_tag, instance)
                .map_err(anyhow::Error::msg)?
                .write(&mut payload.0, speed)
                .map_err(anyhow::Error::msg)?;
            let range = source["acceleration_range"]
                .as_array()
                .context("Source acceleration interval")?;
            ensure!(range.len() == 2, "Source acceleration interval differs");
            let start = range[0].as_f64().context("Acceleration start")? as f32 * metres;
            let end = range[1].as_f64().context("Acceleration end")? as f32 * metres;
            let final_speed = scalar("final_velocity")? * metres;
            let inverse = sundial::package_authoring::sandbox_perk::entity::projectile::parameters::set_stored_curve(
                &mut payload.0, binding.owner_tag, instance, [final_speed, gravity, start, end],
            ).map_err(anyhow::Error::msg)?;
            let wanted = scalar("acceleration_inverse_width")? / metres;
            let rates = source["guided_angular_velocity"]
                .as_array()
                .context("Guidance bounds")?;
            let varying =
                rates[0] != rates[1] || rates[0] != source["guided_angular_velocity_at_rest"];
            ensure!(
                (speed == final_speed && !varying)
                    || (inverse - wanted).abs() <= 1e-5 * wanted.abs().max(1.),
                "Source acceleration inverse is not representable by the native distance curve"
            );
            let definition = usize::try_from(payload.u64(instance + 8)?)?;
            crate::presentation::put(&mut payload.0, definition + 0x8c, &0f32.to_le_bytes())?;
            changes.push(json!({"owner":binding.owner_tag,"initial_speed":speed,"final_speed":final_speed,"curve_start":start,"curve_end":end,"inverse":inverse}));
        }
    }
    ensure!(
        !changes.is_empty(),
        "Source projectile has no supported native movement component"
    );
    Ok(json!({"fields":changes,"length_unit_metres":metres,
        "source_initial_metres_per_second":scalar("initial_velocity")?*metres,
        "source_final_metres_per_second":scalar("final_velocity")?*metres,
        "source_trajectory":source_trajectory,
        "limits":["Range retirement does not reproduce Reach detonation policy. Native global and external perk modifiers remain active. Source speed curves require the declared private player launch policy."]}))
}

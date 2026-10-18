//! MCC effect timing, locations and emitter properties retain their source units.
use crate::halo_reach::{
    cache::{Cache, Tag},
    material, vehicle,
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};

fn scalar(cache: &Cache, at: usize) -> Result<f32> {
    let value = cache.f32(at)?;
    ensure!(value.is_finite(), "Nonfinite source effect scalar");
    Ok(value)
}

fn vector<const N: usize>(cache: &Cache, at: usize) -> Result<[f32; N]> {
    let values = cache.floats::<N>(at)?;
    ensure!(
        values.iter().all(|v| v.is_finite()),
        "Nonfinite source effect vector"
    );
    Ok(values)
}

fn range(cache: &Cache, at: usize, nonnegative: bool) -> Result<[f32; 2]> {
    let values = vector(cache, at)?;
    // Authored random endpoints may descend. MCC debris includes both orders.
    // Preserve their order because interpolation still spans the same interval.
    ensure!(
        !nonnegative || values.iter().all(|value| *value >= 0.),
        "Invalid source effect interval"
    );
    Ok(values)
}

fn fraction(cache: &Cache, at: usize) -> Result<f32> {
    let value = scalar(cache, at)?;
    ensure!(
        (0.0..=1.0).contains(&value),
        "Invalid source effect probability"
    );
    Ok(value)
}

fn property(cache: &Cache, at: usize) -> Result<Value> {
    Ok(
        json!({"inputs":cache.bytes(at,4)?,"function":vehicle::function(cache,at+4)?,
        "compiled_constant":scalar(cache,at+0x18)?,"compiled_flags":cache.u16(at+0x1c)?,
        "force_flags":cache.u8(at+0x1e)?}),
    )
}

fn emitter(cache: &Cache, at: usize) -> Result<Value> {
    let mut properties = serde_json::Map::new();
    for (name, offset) in [
        ("offset", 0x48),
        ("direction", 0x80),
        ("radius", 0xb8),
        ("angle", 0xd8),
        ("axis_angle", 0xf8),
        ("starting_count", 0x118),
        ("maximum_count", 0x138),
        ("rate", 0x158),
        ("life", 0x178),
        ("acceleration", 0x1c8),
        ("velocity", 0x200),
        ("rotation", 0x220),
        ("rotation_rate", 0x240),
        ("size", 0x260),
        ("scale", 0x280),
        ("tint", 0x2a0),
        ("alpha", 0x2c0),
        ("black_point", 0x2e0),
    ] {
        properties.insert(name.into(), property(cache, at + offset)?);
    }
    let movements = cache
        .block(at + 0x1ac, 0x18)?
        .into_iter()
        .map(|row| {
            let parameters = cache
                .block(row + 4, 0x24)?
                .into_iter()
                .map(|parameter| {
                    Ok(json!({"id":cache.u32(parameter)?,"value":property(cache,parameter+4)?}))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(json!({"kind":cache.i16(row)?,"flags":cache.u8(row+2)?,"parameters":parameters}))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"offset":at,"name":cache.string_id(cache.u32(at)?)?,"flags":cache.u8(at+5)?,
        "shape":cache.u8(at+4)?,"additional_flags":cache.u8(at+6)?,
        "model_axis":cache.u8(at+7)?,"reference_axis":cache.u8(at+8)?,
        "custom_shape":cache.reference(at+0xc,None)?,
        "boat_hull":cache.reference(at+0x1c,None)?,"axis_scale":vector::<3>(cache,at+0x34)?,
        "uv_scroll":vector::<2>(cache,at+0x40)?,"properties":properties,
        "offset_bounds":[vector::<3>(cache,at+0x68)?,vector::<3>(cache,at+0x74)?],
        "direction_bounds":[vector::<3>(cache,at+0xa0)?,vector::<3>(cache,at+0xac)?],
        "acceleration_bounds":[vector::<3>(cache,at+0x1e8)?,vector::<3>(cache,at+0x1f4)?],
        "movement_template":cache.reference(at+0x198,None)?,"movement_flags":cache.u16(at+0x1a8)?,
        "movements":movements,"turbulence_texture":cache.reference(at+0x1b8,None)?}),
    )
}

pub(in crate::halo_reach) fn read(cache: &Cache, at: usize) -> Result<Value> {
    cache.meta(at, 0x60)?;
    let locations = cache
        .block(at + 0x20, 8)?
        .into_iter()
        .map(|row| Ok(json!({"name":cache.string_id(cache.u32(row)?)?,"flags":cache.u16(row+4)?})))
        .collect::<Result<Vec<_>>>()?;
    let location = |index: i16| -> Result<i16> {
        ensure!(
            index == -1 || index >= 0 && (index as usize) < locations.len(),
            "Source effect location exceeds table"
        );
        Ok(index)
    };
    let mut events = Vec::new();
    for row in cache.block(at + 0x2c, 0x40)? {
        let parts = cache.block(row+0x1c,0x64)?.into_iter().map(|part| {
            Ok(json!({"offset":part,"tag":cache.reference(part+0x14,None)?,"raw":hex::encode(cache.meta(part,0x64)?),
                "environment":cache.i16(part)?,"violence":cache.i16(part+2)?,
                "location":location(cache.i16(part+4)?)?,"secondary_location":location(cache.i16(part+6)?)?,
                "flags":cache.u16(part+8)?,"camera":cache.u8(part+0xb)?,
                "velocity_source":range(cache,part+0x24,false)?,"velocity_orientation_radians":vector::<2>(cache,part+0x2c)?,
                "velocity_cone_radians":scalar(cache,part+0x34)?,
                "angular_velocity_radians":range(cache,part+0x38,false)?,"radius_modifier":range(cache,part+0x40,true)?,
                "relative_offset_source":vector::<3>(cache,part+0x48)?,"relative_orientation_radians":vector::<2>(cache,part+0x54)?,
                "scale_a":cache.u32(part+0x5c)?,"scale_b":cache.u32(part+0x60)?}))
        }).collect::<Result<Vec<_>>>()?;
        let particles = cache
            .block(row + 0x34, 0x70)?
            .into_iter()
            .map(|system| {
                let emitters = cache
                    .block(system + 0x5c, 0x31c)?
                    .into_iter()
                    .map(|at| emitter(cache, at))
                    .collect::<Result<Vec<_>>>()?;
                Ok(
                    json!({"offset":system,"tag":cache.reference(system+4,Some("prt3"))?,
                "location":location(cache.i16(system+0x16)?)?,"coordinates":cache.i16(system+0x18)?,
                "environment":cache.i16(system+0x1a)?,"disposition":cache.i16(system+0x1c)?,
                "camera":cache.i16(system+0x1e)?,"flags":cache.u32(system+0x24)?,
                "near_fade_source":[scalar(cache,system+0x34)?,scalar(cache,system+0x38)?],
                "maximum_life":scalar(cache,system+0x68)?,"emitters":emitters}),
                )
            })
            .collect::<Result<Vec<_>>>()?;
        events.push(
            json!({"offset":row,"name":cache.string_id(cache.u32(row)?)?,"flags":cache.u16(row+4)?,
            "skip_fraction":fraction(cache,row+8)?,"delay_seconds":range(cache,row+0xc,true)?,
            "duration_seconds":range(cache,row+0x14,true)?,"parts":parts,"particles":particles,
            "raw":hex::encode(cache.meta(row,0x40)?)}),
        );
    }
    let loop_start = cache.i16(at + 0x14)?;
    ensure!(
        loop_start == -1 || loop_start >= 0 && (loop_start as usize) < events.len(),
        "Source effect loop exceeds event table"
    );
    let sounds = cache.block(at+0x38,0x14)?.into_iter().map(|row| {
        let event = cache.i16(row+0x12)?;
        ensure!(event == -1 || event >= 0 && (event as usize) < events.len(), "Source looping sound event exceeds table");
        Ok(json!({"tag":cache.reference(row,Some("lsnd"))?,"location":location(cache.i16(row+0x10)?)?,
            "event":event}))
    }).collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"flags":cache.u32(at)?,"seed":cache.u32(at+4)?,"death_delay_seconds":scalar(cache,at+0x10)?,
        "loop_start":loop_start,"locations":locations,"events":events,"looping_sounds":sounds,
        "length_unit_metres":crate::halo_reach::rig::METRES_PER_UNIT}),
    )
}

pub(in crate::halo_reach) fn particle(cache: &Cache, tag: &Tag) -> Result<Value> {
    ensure!(tag.group == "prt3", "Source particle tag differs");
    let at = tag.address()?;
    cache.meta(at, 0x1f0)?;
    let material = material::embedded(cache, tag.clone(), at + 0x40)?;
    let mut properties = serde_json::Map::new();
    for offset in [0xd8, 0xf8, 0x118, 0x138, 0x15c, 0x17c, 0x19c] {
        properties.insert(format!("{offset:03X}"), property(cache, at + offset)?);
    }
    Ok(json!({"material":material,"properties":properties,
        "header":hex::encode(cache.meta(at,0x40)?),"appearance":hex::encode(cache.meta(at+0xa4,0x34)?),
        "model":cache.reference(at+0x1bc,None)?,"orientation":hex::encode(cache.meta(at+0x1cc,12)?)}))
}

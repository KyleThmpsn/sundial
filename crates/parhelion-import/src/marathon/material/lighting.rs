//! Translate forward material lighting to the target renderer's object probes.
//!
//! Shadowkeep uses three 2D coefficient atlases and a directional-visibility
//! atlas. Marathon's spatial clipmaps are renderer state, not material assets.
//! The material retains its normal, color and coverage equations while the
//! target engine supplies its own probe interpolation and directional lighting.
use super::*;

pub(super) fn adapt(options: &mut dxil::Options, surface: &geometry::Surface) {
    for slot in 16..=19 {
        options.texture_registers.insert(slot, slot - 4);
        options.samples.insert(
            slot,
            dxil::Sample {
                source_kind: 4,
                target_kind: 2,
                coordinates: if slot == 19 {
                    "cb8[4].zw".into()
                } else {
                    "float2(cb8[4].z,cb8[4].w*0.5+0.5)".into()
                },
                // Native forward shaders use visibility.y/(4*pi). The source
                // surface multiplies its directional term by 1/pi.
                scalar: (slot == 19).then_some(('y', 0.25)),
            },
        );
    }
    // Both source sample locations now address the current native object probe.
    // Native visibility already includes the target renderer's light attenuation.
    options.constants.insert((3, 15), "float4(0,1,0,0)".into());
    options.constants.insert((8, 6), "float4(0,0,0,1)".into());
    options.buffers.insert(11, 4);
    for column in 0..3 {
        let m = &surface.transform;
        let weights = [m[column], m[4 + column], m[8 + column]].map(vertex::float);
        options.constants.insert(
            (1, column as u32),
            format!(
                "cb11[0]*{}+cb11[1]*{}+cb11[2]*{}",
                weights[0], weights[1], weights[2]
            ),
        );
    }
}

pub(super) fn validate(
    source: &mut Reader,
    native: &mut Reader,
    material: &Payload,
    inspection: &shader::inputs::Inputs,
) -> Result<()> {
    ensure!(
        material
            .array(0x280, 24, None)?
            .into_iter()
            .all(|row| material
                .u32(row)
                .is_ok_and(|slot| !(12..=19).contains(&slot))),
        "Forward lighting conflicts with a fixed source texture"
    );
    for slot in 12..=15 {
        if slot == 15 && !inspection.resources.contains_key(&slot) {
            continue;
        }
        ensure!(
            inspection
                .resources
                .get(&slot)
                .is_some_and(|kind| kind == "dcl_resource_texture2d (float,float,float,float) @"),
            "Incomplete native forward lighting bindings"
        );
    }
    for (reader, modern) in [(source, true), (native, false)] {
        for index in 0..4 {
            resources::scope_binding(
                reader,
                modern,
                b"transparent",
                &if modern {
                    [0x4d, 0x28, 5 + index, 0x59, 0x30 + index]
                } else {
                    [0x3f, 0x27, 1 + index, 0x47, 0x2c + index]
                },
            )?;
        }
        resources::scope_binding(
            reader,
            modern,
            b"rigid_model",
            &if modern {
                [0x4c, 8, 0, 0x55, 0]
            } else {
                [0x3e, 8, 0, 0x44, 0]
            },
        )?;
    }
    Ok(())
}

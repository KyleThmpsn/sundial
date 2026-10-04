//! Rendered light oracles written before the additional native glow forms.
use super::*;

pub(super) fn render_cases(cases: &mut Vec<(String, Model, [u8; 3])>) {
    for kind in [Kind::DistortedGlow, Kind::WaveGlow] {
        for enabled in [false, true] {
            let mut model = Model::default();
            let mut material = Material {
                kind,
                constants: vec![[0.0; 4]; if kind == Kind::DistortedGlow { 13 } else { 32 }],
                textures: [Some(0), Some(1), None],
                ..Default::default()
            };
            let c = &mut material.constants;
            let intensity = if enabled { 1.0 } else { 0.0 };
            let light = [32.0 / 255.0, 16.0 / 255.0, 8.0 / 255.0, 1.0];
            let rgba = match kind {
                Kind::DistortedGlow => {
                    c[4] = [1.0; 4];
                    c[5] = [1.0, 0.0, 0.0, 0.0];
                    c[6] = c[5];
                    c[7] = [intensity; 4];
                    c[8] = [1.0; 4];
                    c[11] = [1.0; 4];
                    c[12] = [1.0; 4];
                    vec![32, 16, 8, 255]
                }
                Kind::WaveGlow => {
                    for slot in [1, 2, 22, 23] {
                        c[slot] = [1.0, 0.0, 0.0, 0.0];
                    }
                    c[3] = [0.0, 1.0, 0.0, 0.0];
                    c[4] = [1.0; 4];
                    c[24] = [1.0, 0.0, 0.0, 0.0];
                    c[25] = [1.0; 4];
                    c[26] = [1.0; 4];
                    c[27] = light;
                    c[28] = [1.0; 4];
                    c[29] = [intensity; 4];
                    c[30] = [1.0; 4];
                    c[31] = [1.0; 4];
                    vec![255; 4]
                }
                _ => unreachable!(),
            };
            model.textures.push(texture::Texture {
                tag: 1,
                size: [1, 1],
                rgba: vec![0; 4],
            });
            model.textures.push(texture::Texture {
                tag: 2,
                size: [1, 1],
                rgba,
            });
            model.effects.push(material);
            quad(&mut model, 0.0, None, Some([0.0, 0.0, 0.125]));
            quad(&mut model, -0.1, Some(0), None);
            let expected = if enabled {
                [light[0], light[1], 0.125 + light[2]]
            } else {
                [0.0, 0.0, 0.125]
            };
            cases.push((
                format!("effect-glow-{kind:?}-{enabled}"),
                model,
                encoded(expected),
            ));
        }
    }
}

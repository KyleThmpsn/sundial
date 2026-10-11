use super::*;

pub(super) struct Surface<'a> {
    pub material: &'a Material,
    pub constants: BTreeMap<String, [f32; 4]>,
    lens: Option<&'a Lens>,
}

impl<'a> Surface<'a> {
    pub fn new(material: &'a Material, binding: &Binding<'a>, key: u32) -> Self {
        let mut constants = material
            .properties
            .first()
            .map(|p| p.constants.clone())
            .unwrap_or_default();
        if let Some(values) = material
            .tag
            .as_ref()
            .and_then(|t| binding.overrides.get(&t.path))
        {
            constants.extend(values.iter().map(|(k, v)| (k.clone(), *v)));
        }
        Self {
            material,
            constants,
            lens: binding.lenses.get(&key),
        }
    }

    pub fn constant(&self, name: &str) -> Option<[f32; 4]> {
        self.constants.get(name).copied()
    }

    pub fn emission(&self, samples: &BTreeMap<usize, String>, albedo: &str) -> String {
        let tint = self
            .constant("self_illum_color")
            .or_else(|| self.constant("self_illum_tint_color"))
            .unwrap_or([1.; 4]);
        let scale = self
            .constant("self_illum_intensity")
            .map_or(1., |v| v[0])
            .max(0.);
        let source = match self
            .material
            .options
            .get("self_illumination")
            .map(String::as_str)
        {
            Some("off") | None => return "float3(0,0,0)".into(),
            Some("from_diffuse" | "from_albedo") => format!("({albedo}).rgb"),
            Some("3_channel_self_illum") => {
                let Some(mask) = samples.get(&4) else {
                    return "float3(0,0,0)".into();
                };
                let terms = ["channel_a", "channel_b", "channel_c"]
                    .into_iter()
                    .zip(["r", "g", "b"])
                    .map(|(name, lane)| {
                        let c = self.constant(name).unwrap_or([0.; 4]);
                        format!("({mask}).{lane}*{}*{:.9}", rgb(c), c[3])
                    })
                    .collect::<Vec<_>>()
                    .join("+");
                return format!("max(({terms})*{scale:.9},0)");
            }
            Some("palettized_plasma") => {
                let (Some(a), Some(b)) = (samples.get(&6), samples.get(&7)) else {
                    return "float3(0,0,0)".into();
                };
                if !samples.contains_key(&8) {
                    return "float3(0,0,0)".into();
                }
                let alpha = samples
                    .get(&3)
                    .map_or_else(|| "1".into(), |a| format!("({a}).a"));
                let modulation = self
                    .constant("alpha_modulation_factor")
                    .map_or(0., |v| v[0]);
                let v = self.constant("v_coordinate").map_or(0.5, |v| v[0]);
                format!(
                    "Map8.Sample(SurfaceSampler,float2(saturate(abs(({a}).r-({b}).r)+(1-{alpha})*{modulation:.9}),{v:.9})).rgb"
                )
            }
            _ => samples
                .get(&4)
                .map_or_else(|| "float3(0,0,0)".into(), |s| format!("({s}).rgb")),
        };
        format!("max(({source})*{}*{scale:.9},0)", rgb(tint))
    }

    pub fn transparent(&self) -> bool {
        self.lens.is_some() || self.material.alpha_mode() == "BLEND"
    }

    pub fn output(&self) -> &'static str {
        if self.transparent() {
            "out float4 o0:SV_TARGET0"
        } else {
            "out float4 o0:SV_TARGET0,out float4 o1:SV_TARGET1,out float4 o2:SV_TARGET2"
        }
    }

    pub fn finish(&self) -> String {
        if let Some(lens) = self.lens {
            return if lens.reticle {
                format!(
                    "float2 aim=(v4.yz-float2({:.9},{:.9}))/{:.9};float2 edge=max(fwidth(aim),float2(0.002,0.002));float vertical=(1-smoothstep(0.012,0.012+edge.x,abs(aim.x)))*(1-smoothstep(0.22,0.22+edge.y,abs(aim.y)));float horizontal=(1-smoothstep(0.012,0.012+edge.y,abs(aim.y)))*(1-smoothstep(0.22,0.22+edge.x,abs(aim.x)));float mark=max(vertical,horizontal)*(1-step(0.004,abs(v4.x-{:.9})));o0=float4(float3(1,0.08,0.015)*mark+float3(0.001,0.002,0.003),0.015);",
                    lens.center[1], lens.center[2], lens.radius, lens.rear
                )
            } else {
                "o0=float4(0.001,0.002,0.003,0.015);".into()
            };
        }
        if self.transparent() {
            if self
                .material
                .options
                .get("blend_mode")
                .is_some_and(|v| v == "additive")
            {
                "o0=float4(emission,0);".into()
            } else {
                "o0=float4((albedo.rgb+emission)*saturate(albedo.a),saturate(albedo.a));".into()
            }
        } else {
            "float strength=max(emission.r,max(emission.g,emission.b));float3 surface=lerp(albedo.rgb,emission/max(strength,0.00001),saturate(strength));float encodedEmission=strength>0 ? 0.5*(1+2.0/255.0+saturate((log2(strength+1.0/128.0)+7)/13)) : 0.5;o0=float4(saturate(surface),0.04);o1=float4(saturate(n*(roughness*0.125+0.375)+0.5),0);o2=float4(0,encodedEmission,0,v0.w);".into()
        }
    }
}

fn rgb(c: [f32; 4]) -> String {
    format!("float3({:.9},{:.9},{:.9})", c[0], c[1], c[2])
}

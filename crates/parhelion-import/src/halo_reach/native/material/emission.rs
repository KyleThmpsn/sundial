use super::*;

pub(super) struct View<'a> {
    pub camera: &'a str,
    pub tangent: &'a str,
    pub bitangent: &'a str,
    pub depth: bool,
}

pub(super) const SURFACE: View<'_> = View {
    camera: "v5.xyz",
    tangent: "v1.xyz",
    bitangent: "v2.xyz",
    depth: true,
};
pub(super) const SKY: View<'_> = View {
    camera: "skyToCamera",
    tangent: "skyTangent",
    bitangent: "skyBitangent",
    depth: false,
};

pub(super) fn depth_fade(material: &Material) -> bool {
    material.alpha_mode() == "BLEND"
        && matches!(
            material
                .options
                .get("self_illumination")
                .map(String::as_str),
            Some("palettized_plasma" | "palettized_plasma_change_color" | "palettized_depth_fade")
        )
}

/// Native transparent programs read t10 and CB2 for scene depth. Both depths are
/// along the view axis. Reach's fade range uses world units of 3.048 metres.
pub(super) const DEPTH: &str = r#"
Texture2D<float4> EmissionDepth:register(t10);
cbuffer EmissionDepthConstants:register(b2){float4 emissionDepthData[1];};
cbuffer View:register(b12){float4 viewData[14];};
float reachEmissionFade(float4 position,float cosine,float range){
uint width,height;EmissionDepth.GetDimensions(width,height);
if(any(abs(float2(width,height)-viewData[12].xy)>1.5))return 1;
int2 pixel=clamp(int2(position.xy),int2(0,0),int2(width,height)-1);
float depth=EmissionDepth.Load(int3(pixel,0)).x;
float scene=1/max(depth*emissionDepthData[0].y+emissionDepthData[0].x,1e-6);
float surface=1/max(position.z*emissionDepthData[0].y+emissionDepthData[0].x,1e-6);
return saturate((scene-surface)*cosine/(3.048*max(range,1e-8)));
}
"#;

impl surface::Surface<'_> {
    pub fn emission(
        &self,
        samples: &BTreeMap<usize, String>,
        albedo: &str,
        view: &View<'_>,
    ) -> String {
        let mut tint = self.color(
            if self.constants.contains_key("self_illum_color") {
                "self_illum_color"
            } else {
                "self_illum_tint_color"
            },
            [1.; 4],
        );
        let scale = format!("max({},0)", self.scalar("self_illum_intensity", 1.));
        let source = match self
            .material
            .options
            .get("self_illumination")
            .map(String::as_str)
        {
            Some("off" | "none") | None => return "float3(0,0,0)".into(),
            // The source window-room implementation is guarded by DX_VERSION == 9.
            // MCC U13's DirectX 11 permutations have no room textures or emission inputs.
            Some("window_room") => return "float3(0,0,0)".into(),
            Some("from_diffuse" | "from_albedo") => format!("({albedo}).rgb"),
            Some("meter") => {
                let Some(mask) = samples.get(&4) else {
                    return "float3(0,0,0)".into();
                };
                return format!(
                    "(({mask}).r>=0.5?({}>=({mask}).a?{}:{}):float3(0,0,0))",
                    self.scalar("meter_value", 0.),
                    self.color("meter_color_on", [0.; 4]),
                    self.color("meter_color_off", [0.; 4])
                );
            }
            Some("plasma") => {
                let (Some(a), Some(b), Some(alpha)) =
                    (samples.get(&6), samples.get(&7), samples.get(&3))
                else {
                    return "float3(0,0,0)".into();
                };
                let diff = format!("max(0,1-abs(({a}).r-({b}).r))");
                let medium = format!("pow({diff},{})", self.scalar("thinness_medium", 1.));
                let sharp = format!("pow({diff},{})", self.scalar("thinness_sharp", 1.));
                let wide = format!("pow({diff},{})", self.scalar("thinness_wide", 1.));
                let color = |name| {
                    let v = self.value(name, [0.; 4]);
                    format!("({v}).rgb*({v}).a")
                };
                return format!(
                    "({}*({medium}-{sharp})+{}*{sharp}+{}*({wide}-{medium}))*({alpha}).a*{scale}",
                    color("color_medium"),
                    color("color_sharp"),
                    color("color_wide")
                );
            }
            Some("illum_detail" | "change_color_detail") => {
                let (Some(map), Some(detail)) = (samples.get(&4), samples.get(&ILLUM_DETAIL_SLOT))
                else {
                    return "float3(0,0,0)".into();
                };
                if self
                    .material
                    .options
                    .get("self_illumination")
                    .is_some_and(|m| m == "change_color_detail")
                {
                    tint = self.color("primary_change_color", [1.; 4]);
                }
                format!("({map}).rgb*({detail}).rgb*{DETAIL_MULTIPLIER:.6}")
            }
            Some("simple_with_alpha_mask") => {
                let Some(map) = samples.get(&4) else {
                    return "float3(0,0,0)".into();
                };
                let color = self.value("self_illum_color", [1.; 4]);
                format!("({map}).rgb*({map}).a*({color}).a")
            }
            Some("change_color") => {
                tint = self.color("primary_change_color", [1.; 4]);
                samples
                    .get(&4)
                    .map_or_else(|| "float3(0,0,0)".into(), |s| format!("({s}).rgb"))
            }
            Some("times_diffuse" | "self_illum_times_diffuse") => {
                let Some(map) = samples.get(&4) else {
                    return "float3(0,0,0)".into();
                };
                let blend = format!("max(({map}).g*10-9,0)");
                tint = format!(
                    "lerp({tint},{},{})",
                    self.color("primary_change_color", [1.; 4]),
                    self.scalar("primary_change_color_blend", 0.)
                );
                format!("lerp(({albedo}).rgb,float3(1,1,1),{blend})*({map}).rgb")
            }
            Some("scope_blur") => {
                let Some((map, sampler, uv)) =
                    samples.get(&4).and_then(|s| surface::sample_parts(s))
                else {
                    return "float3(0,0,0)".into();
                };
                let average = format!(
                    "(({map}.Sample({sampler},({uv})+float2(0.000868,0.0015625)).rg+{map}.Sample({sampler},({uv})+float2(-0.000868,0.0015625)).rg+{map}.Sample({sampler},({uv})+float2(-0.000868,-0.0015625)).rg+{map}.Sample({sampler},({uv})+float2(0.000868,-0.0015625)).rg)*0.25)"
                );
                return format!(
                    "(({average}).r*{tint}+(1-({average}).r)*({average}).g*{})*{scale}",
                    self.color("self_illum_heat_color", [0.; 4])
                );
            }
            Some("3_channel_self_illum") => {
                let Some(mask) = samples.get(&4) else {
                    return "float3(0,0,0)".into();
                };
                let terms = ["channel_a", "channel_b", "channel_c"]
                    .into_iter()
                    .zip(["r", "g", "b"])
                    .map(|(name, lane)| {
                        let c = self.value(name, [0.; 4]);
                        format!("({mask}).{lane}*({c}).rgb*({c}).a")
                    })
                    .collect::<Vec<_>>()
                    .join("+");
                return format!("max(({terms})*{scale},0)");
            }
            Some(
                "palettized_plasma" | "palettized_depth_fade" | "palettized_plasma_change_color",
            ) => {
                let Some(a) = samples.get(&6) else {
                    return "float3(0,0,0)".into();
                };
                if !samples.contains_key(&8) {
                    return "float3(0,0,0)".into();
                }
                let option = self
                    .material
                    .options
                    .get("self_illumination")
                    .map(String::as_str);
                let index = if option == Some("palettized_depth_fade") {
                    format!("({a}).r")
                } else if let Some(b) = samples.get(&7) {
                    format!("abs(({a}).r-({b}).r)")
                } else {
                    return "float3(0,0,0)".into();
                };
                if option == Some("palettized_plasma_change_color") {
                    tint = self.color("primary_change_color", [1.; 4]);
                }
                let alpha = samples
                    .get(&3)
                    .map_or_else(|| "1".into(), |a| format!("({a}).a"));
                let modulation = self.scalar("alpha_modulation_factor", 0.);
                let v = self.scalar("v_coordinate", 0.5);
                let fade = if view.depth && depth_fade(self.material) {
                    format!(
                        "reachEmissionFade(position,dot(normalize(v5.xyz),n),{})",
                        self.scalar("depth_fade_range", 1.)
                    )
                } else {
                    "1".into()
                };
                let Some((map, sampler, _)) =
                    samples.get(&8).and_then(|s| surface::sample_parts(s))
                else {
                    return "float3(0,0,0)".into();
                };
                format!(
                    "{map}.Sample({sampler},float2(saturate({index}+(1-{alpha}*{fade})*{modulation}),{v})).rgb"
                )
            }
            Some("multilayer_additive") => {
                let Some((map, sampler, uv)) =
                    samples.get(&4).and_then(|s| surface::sample_parts(s))
                else {
                    return "float3(0,0,0)".into();
                };
                let value = |name: &str, default: f32| self.scalar(name, default);
                let transform = self.value("self_illum_map", [1., 1., 0., 0.]);
                format!(
                    "multilayerIllumination({map},{sampler},{uv},({transform}).xy,{},{},{},{},{},clamp({},1,64),{},{})",
                    view.camera,
                    view.tangent,
                    view.bitangent,
                    value("layer_depth", 1.),
                    value("texcoord_aspect_ratio", 1.),
                    value("layers_of_4", 1.),
                    value("depth_darken", 1.),
                    value("layer_contrast", 1.),
                )
            }
            Some("simple") => samples
                .get(&4)
                .map_or_else(|| "float3(0,0,0)".into(), |s| format!("({s}).rgb")),
            _ => return "float3(0,0,0)".into(),
        };
        format!("max(({source})*{tint}*{scale},0)")
    }
}

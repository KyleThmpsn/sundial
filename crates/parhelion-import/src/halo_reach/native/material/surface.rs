use super::*;

/// The shader function of Reach's `multilayer_additive` self illumination, read from the compiled
/// program of `ca_heh_forerunner_halogram`'s template (`D3DDisassemble` of its per-pixel lightmap
/// entry point). It steps the map's coordinates back along the view direction in the surface's
/// tangent frame, `layer_depth` over the layer count per sample with the map's own scale and the
/// U step times `texcoord_aspect_ratio`, takes `layers_of_4` times four samples each weighted by a
/// running product of `depth_darken`, averages them and raises the average to `layer_contrast`.
/// The caller multiplies by the color and intensity. Hang 'Em High's holograms set intensities of
/// 171 and 472 for that sparse, squared average. Taken as one sample, they drew solid cyan.
/// Which of Reach's two tangent vectors steps U was not traced, so U follows the tangent.
pub(super) const MULTILAYER_FUNCTIONS: &str = "float3 multilayerIllumination(Texture2D<float4> map,SamplerState s,float2 uv,float2 scale,float3 toCamera,float3 tangent,float3 bitangent,float depth,float aspect,float layers4,float darken,float contrast){float3 view=normalize(toCamera);float count=4*layers4;float2 step=float2(dot(tangent,view)*aspect,dot(bitangent,view))*scale*depth/count;float3 sum=0;float weight=1;[loop]for(int i=0;i<(int)count;i++){sum+=weight*map.Sample(s,uv).rgb;uv-=step;weight*=darken;}return pow(max(sum/count,0.000001),contrast);}";

/// The texture, sampler and coordinates of a `Map{slot}.Sample(sampler,uv)` sample expression.
pub(super) fn sample_parts(sample: &str) -> Option<(&str, &str, &str)> {
    let (map, rest) = sample.split_once(".Sample(")?;
    let (sampler, uv) = rest.split_once(',')?;
    let uv = uv.strip_suffix(')')?;
    map.starts_with("Map").then_some((map, sampler, uv))
}

pub(super) struct Surface<'a> {
    pub material: &'a Material,
    pub constants: BTreeMap<String, [f32; 4]>,
    pub parameters: animation::Parameters,
    lens: Option<&'a Lens>,
}

impl<'a> Surface<'a> {
    pub fn new(material: &'a Material, binding: &Binding<'a>, key: u32) -> Self {
        let mut constants = material
            .properties
            .first()
            .map(|p| p.constants.clone())
            .unwrap_or_default();
        let overrides = material
            .tag
            .as_ref()
            .and_then(|t| binding.overrides.get(&t.path));
        if let Some(values) = overrides {
            constants.extend(values.iter().map(|(k, v)| (k.clone(), *v)));
        }
        Self {
            parameters: animation::Parameters::new(material, &constants, &|name| {
                overrides.is_some_and(|values| values.contains_key(name))
            }),
            material,
            constants,
            lens: binding.lenses.get(&key),
        }
    }

    pub fn constant(&self, name: &str) -> Option<[f32; 4]> {
        self.constants.get(name).copied()
    }

    pub fn boolean(&self, name: &str) -> Option<bool> {
        self.material
            .properties
            .iter()
            .find_map(|property| property.booleans.get(name).copied())
    }

    pub fn value(&self, name: &str, default: [f32; 4]) -> String {
        self.parameters
            .value(name, self.constant(name).unwrap_or(default))
    }

    pub fn scalar(&self, name: &str, default: f32) -> String {
        format!("({}).x", self.value(name, [default; 4]))
    }

    pub fn color(&self, name: &str, default: [f32; 4]) -> String {
        format!("({}).rgb", self.value(name, default))
    }

    pub fn transparent(&self) -> bool {
        self.lens.is_some() || self.material.alpha_mode() == "BLEND"
    }

    /// A glass surface's albedo, and the shading that replaces it with the color and opacity its
    /// blend writes, as Reach's compiled glass programs compute them. The albedo is the base and
    /// detail maps times the albedo color, with all three alphas. Coverage is that alpha, or the
    /// opacity map's alpha when one is mapped. With the opacity fresnel constants, the opacity is
    /// `1 - (1 - coverage) * (1 - fresnel)`, the fresnel term being
    /// `saturate(coefficient * (1 - n.v)^steepness + bias)` on the vertex normal facing the camera
    /// and zero facing away. Without them it is the coverage. Glass lights its albedo only by
    /// analytical and dynamic lights, which these levels lack, so its color is its environment
    /// reflection times `1 + fresnel`. The dynamic environment cube `environment` samples is
    /// normalized to an average of one ([`super::environment`]), so, as opaque surfaces scale it by
    /// their baked light, glass scales it by the light at its lightmap coordinates or vertex record:
    /// irradiance from half its lobe, the stand-in for an average normal, plus its ambient color,
    /// at `light_scale`. Without a lightmap the light is neutral, and without a cube it stands in
    /// for the cube's average. Taken from the ambient color alone, with no cube, Paradiso's Forge
    /// glass reflected about 0.004 and drew as a faint darkening. `v2.w` carries the vertex
    /// normal's cosine to the camera.
    pub fn glass(
        &self,
        samples: &BTreeMap<usize, String>,
        light_scale: Option<f32>,
        environment: Option<&str>,
    ) -> (String, String) {
        let tint = self.value("albedo_color", [1.; 4]);
        let base = samples
            .get(&0)
            .cloned()
            .unwrap_or_else(|| "float4(1,1,1,1)".into());
        let albedo = match samples.get(&5) {
            Some(detail) => format!(
                "float4(({base}).rgb*({detail}).rgb*({tint}).rgb*{DETAIL_MULTIPLIER:.6},({base}).a*({detail}).a*({tint}).a)"
            ),
            None => format!("({base})*({tint})"),
        };
        let fresnel = match (
            self.constant("opacity_fresnel_coefficient"),
            self.constant("opacity_fresnel_curve_steepness"),
            self.constant("opacity_fresnel_curve_bias"),
        ) {
            (Some(_), Some(_), Some(_)) => format!(
                "float fresnel=v2.w>0?saturate({}*pow(saturate(1-v2.w),{})+{}):0;",
                self.scalar("opacity_fresnel_coefficient", 0.),
                self.scalar("opacity_fresnel_curve_steepness", 1.),
                self.scalar("opacity_fresnel_curve_bias", 0.)
            ),
            _ => "float fresnel=0;".to_owned(),
        };
        let coverage = samples
            .get(&GLASS_OPACITY_SLOT)
            .map_or_else(|| "albedo.a".to_owned(), |s| format!("({s}).a"));
        let light = match light_scale {
            Some(scale) => {
                format!(
                    "float4 glassLobe;float4 glassAmbient;float3 glassDirection;if(v8.w>0.5){{glassLobe=v6;glassAmbient=v7;}}else{{reachLightmap(abs(v3.zw),glassLobe,glassAmbient,glassDirection);}}float3 glassLight=3.141592654*(glassLobe.rgb*0.5+glassAmbient.rgb)*{scale:.9};"
                )
            }
            None => "float3 glassLight=float3(1,1,1);".to_owned(),
        };
        let reflection = if self
            .material
            .options
            .get("environment_mapping")
            .is_none_or(|v| v == "none")
        {
            "float3(0,0,0)".to_owned()
        } else {
            let contribution = self.scalar("environment_map_specular_contribution", 0.);
            let cube = environment.map_or_else(String::new, |sample| format!("({sample})*"));
            format!(
                "{cube}glassLight*{}*{contribution}*(1+fresnel)",
                self.color("env_tint_color", [1.; 4])
            )
        };
        let shading = format!(
            "{fresnel}float opacity=saturate(1-(1-{coverage})*(1-fresnel));{light}albedo=float4({reflection},opacity);"
        );
        (albedo, shading)
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
            if let Some(finish) = decal::multiplication(self.material, "float3(0,0,0)") {
                finish
            } else if self
                .material
                .options
                .get("blend_mode")
                .is_some_and(|v| v == "additive")
            {
                if self.material.decal() {
                    "o0=float4(albedo.rgb,0);".into()
                } else {
                    "o0=float4(emission,0);".into()
                }
            } else if decal::premultiplied(self.material) {
                "o0=float4(albedo.rgb+emission*saturate(albedo.a),saturate(albedo.a));".into()
            } else {
                "o0=float4((albedo.rgb+emission)*saturate(albedo.a),saturate(albedo.a));".into()
            }
        } else {
            "float strength=max(emission.r,max(emission.g,emission.b));float3 surface=lerp(albedo.rgb,emission/max(strength,0.00001),saturate(strength));float encodedEmission=strength>0 ? 0.5*(1+2.0/255.0+saturate((log2(strength+1.0/128.0)+7)/13)) : 0.5;o0=float4(saturate(surface),0.04);o1=float4(saturate(n*((1-roughness)*0.125+0.375)+0.5),0);o2=float4(0,encodedEmission,0,v0.w);".into()
        }
    }
}

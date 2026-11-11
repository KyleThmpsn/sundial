//! A surface's pixel program and its shadow, glow and baked variants, written from its bound
//! resources and lighting, and the workers that compile them while later surfaces build.
use super::resources::{Lighting, Resources};
use super::*;

/// A surface's pixel program and the variants its other techniques draw.
pub(super) struct Pixel {
    text: String,
    /// The alpha-tested shadow program of a [`clipped_caster`].
    shadow: Option<String>,
    /// An opaque lightmapped surface's glow stage program.
    glow: Option<String>,
    /// An opaque lightmapped surface's additional analytical light ([`additional_light`]).
    baked: Option<String>,
}

impl Pixel {
    /// Writes `setup`'s program and its variants, adding the notes they leave to `resources`.
    pub(super) fn write(
        binding: &Binding<'_>,
        setup: &Setup<'_>,
        resources: &mut Resources,
        lighting: &Lighting,
    ) -> Result<Self> {
        let material = setup.material;
        let main = Main::write(binding, setup, resources, lighting)?;
        let text = source(binding, setup, resources, lighting, &main);
        resources
            .notes
            .extend(setup.surface.parameters.unused(&text));
        decal::notes(material, &mut resources.notes);
        if material.water() {
            resources.notes.push(water_note(setup.refracts).into());
        }
        let output = setup.surface.output();
        // Reach's shadow_generate uses the surface's alpha test. View-dependent aim hiding is
        // separate from material coverage, so it does not remove the object's shadow.
        let shadow = clipped_caster(material)
            .then(|| clip_program(&text.replace(main.aim_clip, ""), output, &main.clip))
            .transpose()?;
        let opaque = binding.lightmap.filter(|_| !setup.surface.transparent());
        let glow = opaque
            .map(|lightmap| {
                variant(
                    &text,
                    output,
                    "out float4 o0:SV_TARGET0",
                    &main.finish,
                    &glow_finish(lightmap),
                )
            })
            .transpose()?;
        let baked = opaque
            .filter(|lightmap| additional_light(lightmap, setup.key))
            .map(|lightmap| {
                variant(
                    &text,
                    output,
                    "out float4 o0:SV_TARGET0,out float4 o1:SV_TARGET1",
                    &main.finish,
                    &baked_finish(lightmap),
                )
            })
            .transpose()?;
        Ok(Self {
            text,
            shadow,
            glow,
            baked,
        })
    }
}

/// `text` writing `targets` through `finish` in place of its own `output` and `own` finish.
fn variant(text: &str, output: &str, targets: &str, own: &str, finish: &str) -> Result<String> {
    let text = crate::tiger::shader::replace_once(text, output, targets)?;
    crate::tiger::shader::replace_once(&text, own, finish)
}

/// The statements of the program's `main` and the parts of them its variants change.
struct Main {
    statements: String,
    /// The aim discard, which the shadow variant leaves out.
    aim_clip: &'static str,
    /// The coverage discard, which the shadow variant keeps.
    clip: String,
    /// The statements that write the targets, which the glow and baked variants replace.
    finish: String,
    /// The frame constants the statements read.
    frame: &'static str,
}

impl Main {
    fn write(
        binding: &Binding<'_>,
        setup: &Setup<'_>,
        resources: &mut Resources,
        lighting: &Lighting,
    ) -> Result<Self> {
        let material = setup.material;
        let surface = &setup.surface;
        let parameter_source = &surface.parameters.source;
        let weights = terrain(material).then_some("terrainDraw");
        let terrain_parameters = if terrain(material) {
            terrain::parameters(surface, &terrain_weights(material, &resources.samples))
        } else {
            String::new()
        };
        let mux_parameters = mux::parameters(surface, &resources.samples);
        let (color, shading) = color(binding, setup, resources, weights, &lighting.statements)?;
        let emission = surface.emission(&resources.samples, "unlit", &emission::SURFACE);
        let extinguish = if material
            .options
            .get("self_illumination")
            .is_some_and(|m| matches!(m.as_str(), "from_diffuse" | "from_albedo"))
        {
            "albedo.rgb=0;"
        } else {
            ""
        };
        let normal = match weights {
            Some(weights) => terrain_normal(&resources.samples, weights),
            None => normal::expression(surface, &resources.samples),
        };
        let rough = roughness(material, surface, &resources.samples)?;
        let diffuse = diffuse_statement(binding, material, surface);
        let clip = clip_statement(material, &resources.samples);
        let (finish, frame) = finish_statement(binding, setup);
        // An animated transform reads the game clock from the frame constants.
        let clock = resources
            .samples
            .values()
            .any(|sample| sample.contains("reachPhase("))
            || parameter_source.contains("reachPhase(");
        let frame = if clock { FRAME } else { frame };
        let time = if clock {
            "float time=frameData[0].x;"
        } else {
            ""
        };
        let tinting = dye_tint(setup.paint, &resources.samples)?;
        let decal_coordinates = decal::coordinates(material, surface);
        let parallax_coordinates =
            parallax_coordinates(material, surface, &resources.samples, &mut resources.notes)?;
        let decal_tint = decal::tint(material, surface);
        let warp_coordinates = overlay::coordinates(surface, &resources.samples)?;
        let overlay_parameters =
            overlay::parameters(surface, &resources.samples, &mut resources.notes)?;
        let opacity_parameters = opacity::parameters(surface, &resources.samples)?;
        let probe = if binding.lightmap.is_some() {
            super::probe(surface)
        } else {
            String::new()
        };
        let aim_clip = if setup.aimed.is_some() {
            "clip(0.99-aimVisibility.x);"
        } else {
            ""
        };
        let statements = format!(
            "{aim_clip}{time}{parameter_source}{decal_coordinates}{parallax_coordinates}{warp_coordinates}{terrain_parameters}{mux_parameters}float3 n={normal};n=normalize(n.x*v1.xyz+n.y*v2.xyz+n.z*v0.xyz);float4 albedo={color};{decal_tint}{tinting}{clip}float4 unlit=albedo;float3 emission={emission};{extinguish}{opacity_parameters}{diffuse}{probe}{shading}\nfloat roughness={rough};{overlay_parameters}{finish}"
        );
        Ok(Self {
            statements,
            aim_clip,
            clip,
            finish,
            frame,
        })
    }
}

/// The program's text. A two-sided card lights both faces by its authored normal. Turned toward
/// the camera on back faces, as the stock grass program turns its own, Paradiso's pine branches
/// drew saturated blue, violet and magenta from below, which the baked lobe's response falling
/// off away from the lobe, leaving the ambient term, would explain. Stock grass lights by native
/// light only and has no directional baked term.
fn source(
    binding: &Binding<'_>,
    setup: &Setup<'_>,
    resources: &Resources,
    lighting: &Lighting,
    main: &Main,
) -> String {
    let material = setup.material;
    let declarations = declarations(&resources.registers);
    let dye = setup.paint.map_or_else(String::new, |paint| {
        format!(
            "cbuffer GearPaint:register(b{}) {{float4 dye[27];}};",
            paint.slot
        )
    });
    let functions = functions(binding, material, lighting.light.is_some());
    let scene_declarations = if setup.refracts {
        water::SCENE_DECLARATIONS
    } else if emission::depth_fade(material) {
        emission::DEPTH
    } else {
        ""
    };
    format!(
        r#"
{declarations}
{frame}
{dye}
{display}
{functions}
{curve_functions}
{sampler_declarations}
{lighting_declarations}
{texture_functions}
{environment_declarations}
{specular_declarations}
{wave_functions}
{scene_declarations}
void main(float4 v0:TEXCOORD0,float4 v1:TEXCOORD1,float4 v2:TEXCOORD2,float4 v3:TEXCOORD3,float3 v4:TEXCOORD4,float4 position:SV_POSITION0,float4 v5:TEXCOORD5,float4 v6:TEXCOORD6,float4 v7:TEXCOORD7,float4 v8:TEXCOORD8,float4 v9:TEXCOORD9,uint front:SV_isFrontFace0,
{output}) {{
{statements}
}}
"#,
        frame = main.frame,
        display = setup.display,
        curve_functions = resources.curve_functions,
        sampler_declarations = resources.samplers.declarations,
        lighting_declarations = lighting.declarations,
        texture_functions = resources.texture_functions,
        environment_declarations = resources.environment_declarations,
        specular_declarations = resources.specular.as_deref().unwrap_or(""),
        wave_functions = resources.wave_functions,
        output = setup.surface.output(),
        statements = main.statements,
    )
}

/// One texture declaration per native register. Roles can share one texture binding, and a role
/// whose register another role declared takes that role's name.
fn declarations(registers: &BTreeMap<usize, usize>) -> String {
    let mut declared = BTreeMap::new();
    let mut declarations = String::new();
    for (slot, register) in registers {
        if let Some(first) = declared.get(register) {
            declarations.push_str(&format!("\n#define Map{slot} Map{first}\n"));
        } else {
            declared.insert(register, slot);
            declarations.push_str(&format!(
                "Texture2D<float4> Map{slot}:register(t{register});\n"
            ));
        }
    }
    declarations
}

/// The helpers every surface program declares, then the terrain, fog and highlight ones it calls.
fn functions(binding: &Binding<'_>, material: &Material, highlight: bool) -> String {
    format!(
        "float4 reachNormal(float2 xy){{return float4(float3(xy,sqrt(saturate(1-dot(xy,xy))))*0.5+0.5,1);}}\n{TERRAIN_NORMAL}{}{}{}{}{}",
        animation::CLOCK,
        surface::MULTILAYER_FUNCTIONS,
        if terrain(material) {
            TERRAIN_FUNCTIONS
        } else {
            ""
        },
        binding
            .lightmap
            .and_then(|lightmap| lightmap.fog)
            .map_or_else(String::new, |fog| fog.functions()),
        if highlight { HIGHLIGHT_FUNCTIONS } else { "" }
    )
}

/// The surface's albedo and the lighting statements that follow it. Glass replaces its lighting
/// with the blend its compiled programs write, and a post-lighting decal draws without lighting.
fn color(
    binding: &Binding<'_>,
    setup: &Setup<'_>,
    resources: &Resources,
    weights: Option<&str>,
    lighting: &str,
) -> Result<(String, String)> {
    let material = setup.material;
    let surface = &setup.surface;
    let samples = &resources.samples;
    let mut color = if material.water() {
        water_albedo(material, surface, samples, setup.refracts)
    } else if mux::is(material) {
        mux::albedo(samples)?
    } else {
        albedo(material, samples, surface, weights)
    };
    if let Some(decal_color) = decal::albedo(material, surface, samples)? {
        color = decal_color;
    }
    if material
        .decal
        .as_ref()
        .is_some_and(|decal| decal.render_pass == 1)
    {
        ensure!(
            surface.transparent(),
            "Post-lighting opaque decals require a separate native render path"
        );
        return Ok((color, String::new()));
    }
    if material.glass() {
        return Ok(surface.glass(
            samples,
            binding.lightmap.map(|lightmap| lightmap.scale),
            resources.environment.as_deref(),
        ));
    }
    Ok((color, lighting.to_owned()))
}

/// The native roughness. The native highlight follows Reach's analytical one
/// ([`native_roughness`]). Cook-Torrance and organism surfaces take their Beckmann slope
/// ([`beckmann_roughness`]). Two-lobe Phong surfaces take `analytical_power`, the Phong exponent
/// of their analytical highlight. Their `roughness` only selects the slice of the VMF specular
/// table. Diffuse-only surfaces, decorators, foliage, decals, holograms and every other surface
/// without a specular model stay fully rough instead of gaining a reflected highlight.
fn roughness(
    material: &Material,
    surface: &surface::Surface<'_>,
    samples: &BTreeMap<usize, String>,
) -> Result<String> {
    if terrain(material) {
        return Ok(terrain::roughness(material));
    }
    if mux::is(material) {
        return Ok("1".into());
    }
    Ok(
        match material.options.get("material_model").map(String::as_str) {
            Some("cook_torrance" | "organism") => beckmann_roughness(material, surface, samples)?,
            Some("two_lobe_phong") => native_roughness(
                &format!(
                    "sqrt(2/(max({},0)+2))",
                    surface.scalar("analytical_power", 16.)
                ),
                &specular_strength(material, surface, samples),
            ),
            _ => "1".into(),
        },
    )
}

/// The `analytical_roughness` slope of a Cook-Torrance or organism surface, which only an enabled
/// material texture blends through alpha. A texture bound by another model does not define a
/// gloss channel.
fn beckmann_roughness(
    material: &Material,
    surface: &surface::Surface<'_>,
    samples: &BTreeMap<usize, String>,
) -> Result<String> {
    let name = if surface.constant("analytical_roughness").is_some() {
        "analytical_roughness"
    } else {
        "roughness"
    };
    let rough = surface.scalar(name, 0.48);
    let rough = if surface.boolean("use_material_texture") == Some(true) {
        let sample = samples
            .get(&2)
            .context("Material texture enabled without a bound texture")?;
        let black = surface.scalar("material_texture_black_roughness", 1.);
        format!("lerp({black},{rough},({sample}).a)")
    } else {
        rough
    };
    Ok(native_roughness(
        &rough,
        &specular_strength(material, surface, samples),
    ))
}

/// The diffuse coefficient a lightmapped surface's albedo takes, or a terrain's blended one.
fn diffuse_statement(
    binding: &Binding<'_>,
    material: &Material,
    surface: &surface::Surface<'_>,
) -> String {
    if terrain(material) {
        "albedo.rgb*=terrainDiffuse;".to_owned()
    } else if binding.lightmap.is_some() && !material.water() && !material.glass() {
        format!(
            "albedo.rgb*=max({},0);",
            surface.scalar("diffuse_coefficient", 1.)
        )
    } else {
        String::new()
    }
}

/// The coverage discard of an alpha-tested surface. A decal also draws only within its own
/// coordinates ([`material::Material::decal`]). Drawn with a clamped texture past them,
/// Boardwalk's dirt decals smeared their edge texels into streaks and flat brown across its floors.
fn clip_statement(material: &Material, samples: &BTreeMap<usize, String>) -> String {
    let clip = if material.alpha_mode() == "MASK" {
        format!(
            "clip({}-0.5);",
            samples
                .get(&3)
                .map(|s| format!("({s}).a"))
                .unwrap_or_else(|| "albedo.a".into())
        )
    } else {
        String::new()
    };
    if material.decal() {
        format!("clip(float4(v3.xy,1-v3.xy));{clip}")
    } else {
        clip
    }
}

/// The statements that write the surface's targets and the frame constants they read. A
/// lightmapped surface's finish reads the level's fog, or none where the level has none.
/// Refracting water replaces the scene it covers with the refracted scene ([`water::lighting`]).
fn finish_statement(binding: &Binding<'_>, setup: &Setup<'_>) -> (String, &'static str) {
    let surface = &setup.surface;
    let (finish, frame) = match binding.lightmap {
        Some(lightmap) if !surface.transparent() => (native_finish(lightmap), FRAME),
        Some(lightmap) => (baked_transparent_finish(setup.material, lightmap), FRAME),
        None => (format!("emission+=surfaceOverlay;{}", surface.finish()), ""),
    };
    let finish = match binding.lightmap.map(|lightmap| lightmap.fog.is_some()) {
        Some(true) => format!("float4 fog=reachFog(v5.xyz,v5.w);{finish}"),
        Some(false) => format!("float4 fog=float4(0,0,0,1);{finish}"),
        None => finish,
    };
    let finish = if setup.refracts {
        format!("{finish}{}", water::REFRACTED_FINISH)
    } else {
        finish
    };
    (finish, frame)
}

/// The parallax coordinates of a shader or halogram material, the two groups that map parallax.
fn parallax_coordinates(
    material: &Material,
    surface: &surface::Surface<'_>,
    samples: &BTreeMap<usize, String>,
    notes: &mut Vec<String>,
) -> Result<String> {
    if material
        .tag
        .as_ref()
        .is_some_and(|t| matches!(t.group.as_str(), "rmsh" | "rmhg"))
    {
        parallax::coordinates(surface, samples, notes)
    } else {
        Ok(String::new())
    }
}

fn water_note(refracts: bool) -> &'static str {
    if refracts {
        "Water retains source slope animation and uses separate body coordinates where the geometry supplies them. Map reports count base-UV fallbacks. Painted foam and source reflection require baked lighting. Refraction reads the native transparent scope's scene color and depth, with the refracted point on the water pixel's view ray, only where both textures match the view's target size. Otherwise the water draws as deep water, covering the scene but for its bank alpha. Ripple interactions, underwater state and geometry displacement remain untranslated."
    } else {
        "Water retains source slope animation and uses separate body coordinates where the geometry supplies them. Map reports count base-UV fallbacks. Painted foam and source reflection require baked lighting. Without source refraction, coverage approximates the refraction depth. Ripple interactions and geometry displacement remain untranslated."
    }
}

/// Pixel programs compiling, or loading from the shader cache, on workers while later surfaces
/// build. Each program's nodes take their place in the graph at once, and their payloads are
/// written after the last surface. A cache hit alone opened, leased and validated about eight
/// files.
pub(super) struct Programs {
    workers: crate::cancellation::Background<((Vec<u8>, String), String)>,
    /// Each submitted program's symbol and native template, in submission order.
    pending: Vec<(String, u32)>,
}

/// The symbols of the pixel programs one surface's techniques bind.
pub(super) struct Shaders {
    pub(super) color: String,
    pub(super) shadow: Option<String>,
    pub(super) glow: Option<String>,
    pub(super) baked: Option<String>,
}

impl Programs {
    pub(super) fn new() -> Result<Self> {
        Ok(Self {
            workers: crate::cancellation::Background::new()?,
            pending: Vec::new(),
        })
    }

    /// Reserves `pixel`'s programs, copies of native `template`, and submits them for compiling.
    pub(super) fn submit(
        &mut self,
        native: &mut Reader,
        g: &mut Graph,
        binding: &Binding<'_>,
        key: u32,
        template: u32,
        pixel: Pixel,
    ) -> Result<Shaders> {
        let symbol = |suffix: &str| format!("{}pixel-{key:08X}{suffix}", binding.prefix);
        let color = self.program(native, g, symbol(""), template, pixel.text)?;
        let shadow = pixel
            .shadow
            .map(|text| self.program(native, g, symbol("-shadow"), template, text))
            .transpose()?;
        let glow = pixel
            .glow
            .map(|text| self.program(native, g, symbol("-glow"), template, text))
            .transpose()?;
        let baked = pixel
            .baked
            .map(|text| self.program(native, g, symbol("-baked"), template, text))
            .transpose()?;
        Ok(Shaders {
            color,
            shadow,
            glow,
            baked,
        })
    }

    fn program(
        &mut self,
        native: &mut Reader,
        g: &mut Graph,
        symbol: String,
        template: u32,
        text: String,
    ) -> Result<String> {
        reserve_program(native, g, &symbol, template)?;
        self.workers
            .submit(move || Ok((crate::tiger::shader::compile(&text, false)?, text)));
        self.pending.push((symbol.clone(), template));
        Ok(symbol)
    }

    /// Writes every program once its worker finishes, in submission order.
    pub(super) fn finish(self, native: &mut Reader, g: &mut Graph) -> Result<()> {
        for ((symbol, template), ((bytes, warnings), text)) in
            self.pending.into_iter().zip(self.workers.finish()?)
        {
            write_program(native, g, &symbol, template, &text, &bytes, &warnings)?;
        }
        Ok(())
    }
}

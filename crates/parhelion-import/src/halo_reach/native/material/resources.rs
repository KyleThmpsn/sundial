//! What one surface's pixel program reads: its textures and samplers, the curve, wave,
//! environment and specular tables its samples call, and the level lighting it evaluates, bound
//! to registers clear of the ones its setup reserves.
use super::*;

pub(super) struct Resources {
    pub(super) notes: Vec<String>,
    /// Each bound texture's register and symbol.
    pub(super) bindings: Vec<(usize, String)>,
    /// The sample of each bound map, by map slot.
    pub(super) samples: BTreeMap<usize, String>,
    pub(super) samplers: Samplers,
    /// Each bound map's register, by map slot.
    pub(super) registers: BTreeMap<usize, usize>,
    /// The sampler of the bound diffuse table, in map slot `VMF_TABLE_SLOT`.
    pub(super) table: Option<String>,
    /// Resource declarations and selection helpers for source texture arrays.
    pub(super) texture_functions: String,
    /// The material curve table periodic samples read.
    pub(super) curve_functions: String,
    /// A water surface's wave functions.
    pub(super) wave_functions: String,
    pub(super) environment_declarations: String,
    /// The environment map sample of a transparent lightmapped surface's source lighting.
    pub(super) environment: Option<String>,
    /// The VMF specular table's declarations, when the level supplies one.
    pub(super) specular: Option<String>,
}

impl Resources {
    /// Binds `setup`'s textures and samplers, then the tables its samples and source lighting
    /// read. A transparent lightmapped surface lights itself from the source environment and
    /// specular tables, where an opaque one takes native lighting.
    pub(super) fn bind(
        c: &Cache,
        pages: &mut Pages,
        native: &mut Reader,
        g: &mut Graph,
        binding: &Binding<'_>,
        setup: &Setup<'_>,
        clamped: [u8; 16],
    ) -> Result<Self> {
        let material = setup.material;
        let reserved = setup.reserved.as_slice();
        let TextureBindings {
            mut notes,
            mut bindings,
            mut samples,
            samplers: modes,
            registers,
            table,
            functions,
        } = textures(c, pages, native, g, material, setup.paint, reserved)?;
        let samplers = samplers(native, g, clamped, &modes)?;
        if !binding.lenses.contains_key(&setup.key) {
            support::validate(material, &samples, &mut notes).with_context(|| {
                format!(
                    "Surface {}",
                    material.tag.as_ref().map_or("unnamed", |t| &t.path)
                )
            })?;
        }
        notes.extend(setup.surface.parameters.notes.iter().cloned());
        let curve_functions = curves(native, g, &mut bindings, &samples, setup)?;
        let wave_functions = if material.water() {
            waves(c, pages, native, g, setup, &mut samples, &mut bindings)?
        } else {
            String::new()
        };
        let mut resources = Self {
            notes,
            bindings,
            samples,
            samplers,
            registers,
            table,
            texture_functions: functions,
            curve_functions,
            wave_functions,
            environment_declarations: String::new(),
            environment: None,
            specular: None,
        };
        if binding.lightmap.is_some() && setup.surface.transparent() {
            resources.source_lighting(c, pages, native, g, setup)?;
        }
        Ok(resources)
    }

    /// Binds the source environment map and VMF specular table.
    fn source_lighting(
        &mut self,
        c: &Cache,
        pages: &mut Pages,
        native: &mut Reader,
        g: &mut Graph,
        setup: &Setup<'_>,
    ) -> Result<()> {
        let material = setup.material;
        (self.environment_declarations, self.environment) = environment::bind(
            c,
            pages,
            native,
            g,
            material,
            &setup.surface,
            &mut self.bindings,
            &setup.reserved,
            &mut self.notes,
        )?;
        self.specular = specular::bind(c, pages, native, g, material, &mut self.bindings)?;
        if matches!(
            material.options.get("material_model").map(String::as_str),
            Some("two_lobe_phong" | "cook_torrance")
        ) && self.specular.is_none()
        {
            self.notes.push("The source VMF specular table is absent. Area and environment specular retain only the ambient basis.".into());
        }
        Ok(())
    }
}

/// Binds the material curve table when a sample or animated parameter calls `reachPeriodic`, and
/// returns its declarations. The table takes the next register, which a reserved one moves past.
fn curves(
    native: &mut Reader,
    g: &mut Graph,
    bindings: &mut Vec<(usize, String)>,
    samples: &BTreeMap<usize, String>,
    setup: &Setup<'_>,
) -> Result<String> {
    let periodic = |text: &str| text.contains("reachPeriodic(");
    if !samples.values().any(|sample| periodic(sample.as_str()))
        && !periodic(setup.surface.parameters.source.as_str())
    {
        return Ok(String::new());
    }
    let free = free_registers(bindings.as_slice(), 1, &setup.reserved);
    let functions = animation::periodic(native, g, bindings)?;
    match bindings.last_mut() {
        Some((register, _)) if *register != free => {
            let declared = format!("register(t{register})");
            ensure!(
                functions.matches(&declared).count() == 1,
                "The material curve table's declaration differs"
            );
            let functions = functions.replace(&declared, &format!("register(t{free})"));
            *register = free;
            Ok(functions)
        }
        _ => Ok(functions),
    }
}

/// Replaces a water surface's normal sample with the slope of its waves, or of its bump map for a
/// `bump` wave shape, and returns the wave functions that slope calls.
fn waves(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    setup: &Setup<'_>,
    samples: &mut BTreeMap<usize, String>,
    bindings: &mut Vec<(usize, String)>,
) -> Result<String> {
    let material = setup.material;
    ensure!(
        !water::painted_foam(material)
            || [WATER_SHAPE_SLOT, WATER_FOAM_SLOT, WATER_FOAM_DETAIL_SLOT]
                .iter()
                .all(|slot| samples.contains_key(slot)),
        "Painted water foam is missing its shape or foam textures"
    );
    let (functions, normal) = if material
        .options
        .get("waveshape")
        .is_some_and(|shape| shape == "bump")
    {
        (String::new(), water_normal(samples))
    } else {
        water::waves(
            c,
            pages,
            native,
            g,
            material,
            &setup.surface,
            bindings,
            &setup.reserved,
        )
        .with_context(|| {
            format!(
                "Water waves for {}",
                material.tag.as_ref().map_or("unnamed", |tag| &tag.path)
            )
        })?
    };
    samples.insert(1, normal);
    samples.remove(&BUMP_DETAIL_SLOT);
    Ok(functions)
}

/// A lightmapped surface's level lighting: the analytical highlight its selected light gives, the
/// source reflection a transparent surface's specular model adds, and the lighting statements
/// with their declarations. A surface without a lightmap has none of them.
pub(super) struct Lighting {
    pub(super) light: Option<Highlight>,
    /// The reflection expression and its scale.
    pub(super) reflected: Option<(String, [f32; 3])>,
    pub(super) statements: String,
    pub(super) declarations: String,
}

impl Lighting {
    /// Evaluates `setup`'s level lighting. Transparent surfaces evaluate their analytical term
    /// here, after native lighting. The lighting images bind clear of the reserved registers.
    pub(super) fn bind(
        binding: &Binding<'_>,
        setup: &Setup<'_>,
        resources: &mut Resources,
    ) -> Result<Self> {
        let Some(lightmap) = binding.lightmap else {
            return Ok(Self {
                light: None,
                reflected: None,
                statements: String::new(),
                declarations: String::new(),
            });
        };
        let material = setup.material;
        let surface = &setup.surface;
        let selected = lightmap.analytical.get(&setup.key).copied();
        let light = highlight(lightmap, selected);
        let environment = resources.environment.as_deref();
        let reflected = if !surface.transparent() {
            None
        } else if terrain(material) {
            terrain::reflection(
                surface,
                resources.table.as_deref(),
                environment,
                lightmap.level,
                light.as_ref(),
            )
        } else if !material.water() && !material.glass() {
            reflection(
                material,
                surface,
                environment,
                &resources.samples,
                resources.specular.is_some(),
                light.as_ref(),
            )
        } else {
            None
        };
        let water = material.water().then(|| {
            water::lighting(
                surface,
                &resources.samples,
                environment,
                lightmap.level,
                setup.refracts,
            )
        });
        let (statements, declarations) = level_lighting(
            lightmap,
            selected,
            !surface.transparent(),
            &mut resources.bindings,
            resources.table.as_deref(),
            reflected
                .as_ref()
                .map(|(expression, _)| expression.as_str()),
            water.as_deref(),
            &setup.reserved,
        )?;
        Ok(Self {
            light,
            reflected,
            statements,
            declarations,
        })
    }
}

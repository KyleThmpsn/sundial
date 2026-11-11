//! A surface's native records: its technique copied from the opaque or transparent shell with
//! its texture, sampler and model input tables, the glow and baked companions of an opaque
//! lightmapped surface and the alpha-tested shadow of a clipped caster.
use super::pixel::Shaders;
use super::resources::Resources;
use super::*;

/// Adds `setup`'s technique, its companions and its alpha-tested shadow to the graph, and
/// returns the native stage the technique draws in.
pub(super) fn add(
    native: &mut Reader,
    g: &mut Graph,
    binding: &Binding<'_>,
    setup: &Setup<'_>,
    shells: &mut Shells,
    resources: &Resources,
    shaders: &Shaders,
) -> Result<u8> {
    let key = setup.key;
    let (template, envelope, stage) = if setup.surface.transparent() {
        (0x815246aa, &shells.transparent, 7)
    } else {
        (0x80ec270d, &shells.opaque, 0)
    };
    let record = Record::new(
        binding,
        setup,
        envelope,
        resources,
        shells.sampler_class,
        &shaders.color,
    )?;
    for (suffix, pixel) in [("baked", &shaders.baked), ("glow", &shaders.glow)] {
        let Some(pixel) = pixel else { continue };
        companion(
            g,
            binding.prefix,
            key,
            suffix,
            pixel,
            &record,
            &shells.transparent,
        )?;
    }
    g.add(
        &format!("{}material-{key:08X}", binding.prefix),
        template,
        &record.bytes,
        None,
        record.patches,
    )?;
    if let Some(pixel) = &shaders.shadow {
        if shells.clipped.is_none() {
            shells.clipped = Some(clipped_shadow_envelope(native, binding.statics)?);
        }
        clipped_shadow(
            g,
            shells
                .clipped
                .as_deref()
                .context("Native alpha-tested shadow technique")?,
            binding.statics,
            &record.bytes,
            &format!("{}shadow-vertex", binding.prefix),
            pixel,
            &resources.bindings,
            &resources.samplers.symbols,
            &format!("{}material-{key:08X}-shadow", binding.prefix),
        )?;
    }
    Ok(stage)
}

/// A technique's bytes and the patches that bind its programs and resources by symbol.
struct Record {
    bytes: Vec<u8>,
    patches: Vec<Value>,
}

impl Record {
    /// The surface's technique from `envelope`, scoped for a static instance or gear paint, with
    /// its texture and sampler tables and the model inputs its pixel program reads, bound to the
    /// shared vertex program, `pixel` and the surface's resources.
    fn new(
        binding: &Binding<'_>,
        setup: &Setup<'_>,
        envelope: &Payload,
        resources: &Resources,
        sampler_class: u32,
        pixel: &str,
    ) -> Result<Self> {
        let mut bytes = fresh_surface(envelope)?;
        if binding.statics {
            static_scope(&mut bytes, binding, setup)?;
        }
        if let Some(paint) = setup.paint {
            for offset in [24, 28] {
                let current = Payload(bytes.clone()).u32(offset)?;
                put(&mut bytes, offset, &(current | paint.scopes).to_le_bytes())?;
            }
        }
        let mut fixed = Vec::new();
        for (slot, _) in &resources.bindings {
            fixed.extend((*slot as u32).to_le_bytes());
            fixed.extend(u32::MAX.to_le_bytes());
        }
        append(&mut bytes, 0x2d0, 0x80807211, &fixed, 8)?;
        append(
            &mut bytes,
            0x308,
            u64::from(sampler_class),
            &resources.samplers.table,
            16,
        )?;
        inputs(binding, setup, &mut bytes, resources)?;
        let mut patches = surface_patches(
            &bytes,
            &format!("{}vertex", binding.prefix),
            pixel,
            &resources.bindings,
            &resources.samplers.symbols,
        )?;
        if let Some(symbol) = binding.vertex_data {
            let mut row = binding.vertex_data_register().to_le_bytes().to_vec();
            row.extend(u32::MAX.to_le_bytes());
            append(&mut bytes, 0x50, 0x80807211, &row, 8)?;
            let at = Payload(bytes.clone()).array(0x50, 8, None)?[0];
            patches.push(json!({"offset":at+4,"symbol":symbol}));
        }
        Ok(Self { bytes, patches })
    }
}

/// A static instance's scopes in place of an object's, with its depth state and culling.
fn static_scope(bytes: &mut [u8], binding: &Binding<'_>, setup: &Setup<'_>) -> Result<()> {
    for offset in [24, 28] {
        let current = Payload(bytes.to_vec()).u32(offset)?;
        put(
            bytes,
            offset,
            &((current & !OBJECT_SCOPE) | STATIC_SCOPE).to_le_bytes(),
        )?;
    }
    if !setup.surface.transparent() {
        bytes[DEPTH_STATE] = 0;
    }
    if two_sided_static(setup.material) {
        bytes[RASTERIZER] = if binding.paired_faces.contains(&setup.key) {
            BACK_FACES_CULLED
        } else {
            TWO_SIDED
        };
    }
    Ok(())
}

/// Binds the surface's live model inputs to the material rows after its lighting control rows,
/// in the order the constants declare, and appends the expression that loads them with the
/// surface's samplers.
fn inputs(
    binding: &Binding<'_>,
    setup: &Setup<'_>,
    bytes: &mut Vec<u8>,
    resources: &Resources,
) -> Result<()> {
    let mut expression = if resources.bindings.is_empty() {
        vec![]
    } else {
        resources.samplers.expression.clone()
    };
    let mut inputs = Vec::new();
    if setup.animated {
        inputs.push(
            *binding
                .objects
                .get("32CC5402")
                .context("Native current-magazine model input")?,
        );
    }
    inputs.extend(setup.aimed);
    for (row, &input) in inputs.iter().enumerate() {
        expression.extend([0x4d, input, 0x43, u8::try_from(setup.input_base() + row)?]);
    }
    if !inputs.is_empty() {
        bytes[0x33c] = 0x10;
        put(
            bytes,
            0x338,
            &u32::try_from(binding.objects.len())?.to_le_bytes(),
        )?;
        put(bytes, 0x348, &0u32.to_le_bytes())?;
    }
    let mut initial = if setup.controlled_lighting {
        lighting_control::bind(bytes, &mut expression)?;
        lighting_control::initial()
    } else {
        Vec::new()
    };
    initial.resize(initial.len() + 16 * inputs.len(), 0);
    append(bytes, 0x318, 0x80800090, &initial, 16)?;
    put(bytes, 0x34c, &u32::MAX.to_le_bytes())?;
    append(bytes, 0x2e8, 0x80800009, &expression, 1)
}

/// Adds the `suffix` companion of the surface's technique, which draws `pixel` in the baked or
/// glow stage from the transparent shell's header. Both companions retain the surface's instance
/// or object producer and resources. Only the later glow draw requests the transparent
/// renderer's scene inputs.
fn companion(
    g: &mut Graph,
    prefix: &str,
    key: u32,
    suffix: &str,
    pixel: &str,
    record: &Record,
    transparent: &Payload,
) -> Result<()> {
    let bytes = &record.bytes;
    let baked = suffix == "baked";
    let mut companion = bytes.clone();
    lighting_control::stage(&mut companion, if baked { 4 } else { 7 })?;
    let envelope = fresh_surface(transparent)?;
    companion[8..MATERIAL_HEADER].copy_from_slice(&envelope[8..MATERIAL_HEADER]);
    for offset in [24, 28] {
        let original = Payload(bytes.clone()).u32(offset)?;
        let scopes = Payload(envelope.clone()).u32(offset)?;
        let scopes = if baked {
            original
        } else {
            (scopes & !(OBJECT_SCOPE | STATIC_SCOPE)) | original
        };
        put(&mut companion, offset, &scopes.to_le_bytes())?;
    }
    companion[RASTERIZER] = bytes[RASTERIZER];
    if baked {
        // Native blend row 2 adds each target. Composite depth/stencil ordinal 15
        // selects depth definition 3, which tests the surface without writing depth.
        companion[0x20] = 0x82;
        companion[0x21] = 0x8f;
        companion[0x23] = 0x80;
        if companion[RASTERIZER] & 0x80 == 0 {
            companion[RASTERIZER] = BACK_FACES_CULLED;
        }
    }
    let mut patches = record.patches.clone();
    let pixel_patch = patches
        .iter_mut()
        .find(|patch| patch["offset"].as_u64() == Some(0x2C8))
        .context("Companion material pixel binding")?;
    pixel_patch["symbol"] = json!(pixel);
    g.add(
        &format!("{prefix}material-{key:08X}-{suffix}"),
        0x815246aa,
        &companion,
        None,
        patches,
    )
}

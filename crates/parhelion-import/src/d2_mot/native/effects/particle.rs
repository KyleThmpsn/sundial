//! Particle pixel equations with checked native renderer resources.
use super::*;
use serde::Serialize;
mod signature;

#[derive(Serialize)]
pub struct Pixel {
    pub hlsl: String,
    /// Append after the material program. These bind native renderer resources
    /// into the shader's private slots, preserving the source sampling equations.
    pub bindings: Vec<u8>,
    pub required_scopes: u64,
}

fn transparent(root: &Path, modern: bool) -> Result<Payload> {
    let era = if modern { "modern" } else { "native" };
    let directory = root.join(format!("tfx-{era}"));
    let context = load(&directory.join("context.json"))?;
    let entries = context["scopes"]
        .as_array()
        .context("particle renderer scopes")?
        .iter()
        .filter(|scope| scope["name"] == "transparent")
        .collect::<Vec<_>>();
    ensure!(
        entries.len() == 1,
        "particle transparent scope is missing or ambiguous"
    );
    ensure!(
        entries[0]["index"] == 13,
        "particle transparent scope index differs"
    );
    let tag = entries[0]["tag"]
        .as_str()
        .context("particle transparent scope tag")?;
    Ok(Payload(fs::read(directory.join(format!("raw/{tag}.bin")))?))
}

fn resource(code: &[u8], expected: &[u8]) -> Result<()> {
    ensure!(
        code.windows(expected.len())
            .filter(|bytes| *bytes == expected)
            .count()
            == 1,
        "particle renderer resource contract differs"
    );
    Ok(())
}

/// Convert the inspected transparent particle family. The caller still owns
/// material textures, emitter constants, render state and shader compilation.
pub fn pixel(source: &str, bytecode: &[u8], refs: &Path) -> Result<Pixel> {
    let source = source.replace("\r\n", "\n");
    let source = signature::restore(&source, bytecode)?;
    ensure!(
        inputs::cb_count(&source, 2)? == Some(1)
            && inputs::cb_count(&source, 8)? == Some(8)
            && inputs::cb_count(&source, 12)? == Some(15)
            && inputs::cb_count(&source, 13)? == Some(2),
        "particle pixel scope layout differs"
    );
    inputs::validate_pixel_view(refs)?;
    let modern = transparent(refs, true)?;
    let native = transparent(refs, false)?;
    let modern_code = array_bytes(&modern, 0x60, 1)?;
    let native_code = array_bytes(&native, 0x58, 1)?;
    ensure!(
        modern.u32(0xB0)? == 2
            && native.u32(0xB8)? == 2
            && modern.array(0x90, 16, Some(0x80800090))?.len() == 6
            && native.array(0x88, 16, Some(0x80800090))?.len() == 6,
        "particle depth buffer producer differs"
    );
    // Both runtimes reconstruct depth using Deferred vector zero in CB2[0].
    // The Deferred texture moved from source resource 15 to native resource 7.
    resource(&modern_code, &[0x4B, 3, 0, 0x52, 0])?;
    resource(&native_code, &[0x3D, 3, 0, 0x43, 0])?;
    resource(&modern_code, &[0x4D, 3, 15, 0x56, 0x2A])?;
    resource(&native_code, &[0x3F, 3, 7, 0x47, 0x2A])?;
    let (mut hlsl, replaced) = lighting::adapt(&source)?;
    ensure!(replaced, "particle lighting family is unsupported");
    hlsl = inputs::pixel_with_plates(&hlsl, None)?;
    let mut bindings = lighting::bindings(refs)?;
    for (slot, source_resource, native_resource, native_slot, dimension) in
        [(20, 9, 5, 16, "Texture2D"), (21, 10, 6, 17, "Texture3D")]
    {
        ensure!(
            hlsl.contains(&format!("{dimension}<float4> t{slot} : register(t{slot});")),
            "particle atmosphere resource dimension differs"
        );
        resource(
            &modern_code,
            &[0x4D, 0x28, source_resource, 0x56, 0x20 | slot],
        )?;
        resource(
            &native_code,
            &[0x3F, 0x27, native_resource, 0x47, 0x20 | native_slot],
        )?;
        bindings.extend([0x3F, 0x27, native_resource, 0x47, 0x20 | slot]);
    }
    ensure!(
        inputs::slots(&hlsl, 'b')? == [0, 2, 8, 12, 13].into_iter().collect(),
        "particle pixel retains unsupported constant buffers"
    );
    ensure!(
        inputs::slots(&hlsl, 't')?
            .iter()
            .all(|slot| matches!(slot, 0..=10 | 20..=21 | 27..=31)),
        "particle pixel retains unsupported textures"
    );
    Ok(Pixel {
        hlsl,
        bindings,
        required_scopes: (1 << 0) | (1 << 1) | (1 << 13) | (1 << 14),
    })
}

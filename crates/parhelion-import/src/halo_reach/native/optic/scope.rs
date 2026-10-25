//! A dedicated reticle pass follows the native sight's named visibility input.
use super::*;
use crate::presentation::append;

const VERTEX: &str = r#"
void main(float4 p:POSITION0,
out float2 uv:TEXCOORD0,out float4 position:SV_POSITION0) {
uv=p.xy;position=float4(p.xy,0.5,1);
}
"#;

mod layers;

pub(super) struct Source<'a> {
    pub cache: &'a cache::Cache,
    pub pages: &'a mut resource::Pages,
    pub overlay: &'a crate::halo_reach::hud::scope::Scope,
}

pub(super) fn visibility(native: &mut Reader) -> Result<String> {
    let witness = native.tag(0x80ba71c2, Some(0x80809c36))?;
    crate::tiger::channel::object_channel_map(&witness.0)?
        .into_iter()
        .find_map(|(name, index)| (index == 2).then_some(name))
        .context("Native reticle visibility input is absent")
}

pub(super) fn build(
    native: &mut Reader,
    graph: &mut Graph,
    symbol: &str,
    inputs: &BTreeMap<String, u8>,
    source: Source<'_>,
) -> Result<Value> {
    // This native reticle's input two controls its existing appearance ramp.
    // Resolve the same named input on the selected sight, whose order can differ.
    let name = visibility(native)?;
    let input = *inputs
        .get(&name)
        .context("Selected optic lacks reticle visibility")?;
    let material = native.tag(0x8152c441, Some(0x808071e8))?;
    ensure!(
        material.u32(24)? == 0x04002083 && material.u32(32)? == 0x88,
        "Native reticle rendering contract differs"
    );
    let mut bytes = material.0.clone();
    let code = material.array(0x2e8, 1, Some(0x80800009))?;
    let expected = hex::decode(
        "4c0049213f490c47264d022200232200370023220043033c5c0043063c5c0143073c5c0243083c5c0343093c5c04430a3c5c05430b3d49054312",
    )?;
    ensure!(
        code.len() == expected.len()
            && code
                .iter()
                .zip(&expected)
                .all(|(&at, &value)| material.0[at] == value),
        "Native reticle visibility program differs"
    );
    // Keep the native linear clamp sampler. The selected weapon supplies all
    // texture pixels and the selected optic supplies its live aim input.
    for at in [0x2d0, 0x2f8, 0x328] {
        put(&mut bytes, at, &[0; 16])?;
    }
    append(&mut bytes, 0x2e8, 0x80800009, &[0x4d, input, 0x43, 3], 1)?;
    bytes[0x33c] = 0x10;
    put(&mut bytes, 0x348, &0u32.to_le_bytes())?;
    put(&mut bytes, 0x34c, &u32::MAX.to_le_bytes())?;
    // State zero disables both depth and stencil. Rasterizer one is two-sided.
    // A screen-space overlay must not inherit the physical sight's clipping.
    bytes[0x21] = 0x80;
    bytes[0x22] = 0x81;
    put(
        &mut bytes,
        0x338,
        &u32::try_from(inputs.len())?.to_le_bytes(),
    )?;
    // The full-screen vertex positions need no view or object expression reads.
    for at in [0x50, 0x68, 0x78, 0x88, 0x98, 0xa8] {
        put(&mut bytes, at, &[0; 16])?;
    }
    put(&mut bytes, 0xb8, &0u32.to_le_bytes())?;
    bytes[0xbc] = 0;
    put(&mut bytes, 0xc8, &u32::MAX.to_le_bytes())?;
    let mut textures = Vec::new();
    let mut slots = Vec::new();
    for layer in &source.overlay.layers {
        let texture = crate::halo_reach::native::material::texture(
            source.cache,
            source.pages,
            native,
            graph,
            &layer.mapping,
            false,
            false,
        )?;
        let slot = if let Some(slot) = textures.iter().position(|s| s == &texture) {
            slot
        } else {
            textures.push(texture);
            textures.len() - 1
        };
        slots.push(slot);
    }
    ensure!(
        textures.len() <= 16,
        "Scope exceeds the native texture binding limit"
    );
    let mut fixed = Vec::new();
    for slot in 0..textures.len() {
        fixed.extend(u32::try_from(slot)?.to_le_bytes());
        fixed.extend(u32::MAX.to_le_bytes());
    }
    append(&mut bytes, 0x2d0, 0x80807211, &fixed, 8)?;
    let vertex = "scope-vertex";
    let pixel = "scope-pixel";
    crate::halo_reach::native::material::program(
        native,
        graph,
        vertex,
        material.u32(0x48)?,
        VERTEX,
        true,
    )?;
    crate::halo_reach::native::material::program(
        native,
        graph,
        pixel,
        material.u32(0x2c8)?,
        &layers::pixel(source.overlay, &slots)?,
        false,
    )?;
    put(&mut bytes, 0x48, &u32::MAX.to_le_bytes())?;
    put(&mut bytes, 0x2c8, &u32::MAX.to_le_bytes())?;
    let mut patches = vec![
        json!({"offset":0x48,"symbol":vertex}),
        json!({"offset":0x2c8,"symbol":pixel}),
    ];
    for (row, texture) in Payload(bytes.clone())
        .array(0x2d0, 8, Some(0x80807211))?
        .into_iter()
        .zip(&textures)
    {
        patches.push(json!({"offset":row+4,"symbol":texture}));
    }
    graph.add("scope-material", 0x8152c441, &bytes, None, patches)?;

    let mesh = Mesh {
        positions: vec![[-1., -1., 0.], [1., -1., 0.], [-1., 1., 0.], [1., 1., 0.]],
        attributes: vec![[0., 0., 0., 0., 1., 1., 1., 0., 0., 1.]; 4],
        groups: vec![(0, vec![[0, 1, 2], [2, 1, 3]])],
        weights: Vec::new(),
        bones: 0,
    };
    let model = native.tag(0x80ba71be, Some(0x808073a5))?;
    let row = *model
        .array(16, 136, Some(0x80807378))?
        .first()
        .context("Native reticle mesh")?;
    let mut encoded =
        geometry::encode_source_passes(&mesh, &model, row, &BTreeMap::from([(0, 16)]))?;
    for (index, kind) in ["positions", "attributes", "indices"]
        .into_iter()
        .enumerate()
    {
        let tag = model.u32(row + [0, 4, 16][index])?;
        let mut header = native.tag(tag, None)?.0.clone();
        put(
            &mut header,
            if index == 2 { 8 } else { 0 },
            &u32::try_from(encoded.streams[index].len())?.to_le_bytes(),
        )?;
        if index == 2 {
            header[1] = 0;
        } else {
            put(
                &mut header,
                4,
                &(if index == 0 { 8u16 } else { 24u16 }).to_le_bytes(),
            )?;
        }
        let header_symbol = format!("scope-{kind}-header");
        let data_symbol = format!("scope-{kind}-data");
        graph.add(&header_symbol, tag, &header, Some(&data_symbol), vec![])?;
        graph.add(
            &data_symbol,
            native.reference(tag)?,
            &encoded.streams[index],
            Some(&header_symbol),
            vec![],
        )?;
        let old = format!("{kind}-header");
        for patch in &mut encoded.patches {
            if patch["symbol"] == old {
                patch["symbol"] = json!(header_symbol);
            }
        }
    }
    for patch in &mut encoded.patches {
        if patch["symbol"] == "material-00000000" {
            patch["symbol"] = json!("scope-material");
        }
    }
    graph.add(symbol, 0x80ba71be, &encoded.header, None, encoded.patches)?;
    Ok(
        json!({"model":symbol,"material":"scope-material","stage":16,"visibility_input":name,"visibility_slot":input,
        "native_visibility_material":"8152C441","projection":"Source bitmap placement uniformly fitted to the viewport with mirrored borders","source_hud":source.overlay,"textures":textures,
        "depth_stencil_state":0,"rasterizer_state":1,
        "physical_lenses_preserved":true,"gameplay_verified":false}),
    )
}

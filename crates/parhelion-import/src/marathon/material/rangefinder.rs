//! A sight display's distance readout, driven by the scene depth under the view centre.
//!
//! The source sight component publishes `reticle_distance` through a scalar getter, and a
//! display material turns it into digits. Shadowkeep's ordinary sight component `8080393B` has
//! no such getter. Its only stock distance provider is the target sight `80803820`, whose
//! `active_target_distance` getter feeds three stock reticle banks. The transparent scope
//! already binds scene depth to `t10` and its linearization to `CB2[0]`, and the stock forward
//! program `80B38506` reads `1 / (t10 * CB2[0].y + CB2[0].x)` as view depth. The readout takes
//! that depth at the view centre, which is the distance to whatever the sight points at.
use super::*;

/// `reticle_distance`, the source sight's distance in metres.
pub(super) const RETICLE_DISTANCE: u32 = 0xEE12_EB97;

/// A material row whose X lane is the whole metres plus a constant, with its other lanes fixed.
pub(super) struct Readout {
    pub row: usize,
    /// The row at zero distance.
    pub base: [f32; 4],
}

fn lanes(value: &[u8; 16]) -> [f32; 4] {
    std::array::from_fn(|i| f32::from_le_bytes(value[i * 4..i * 4 + 4].try_into().unwrap()))
}

/// The rows of `code` that depend on the distance. Each must be `floor(distance) + c` in X and
/// constant elsewhere, the form the examined three-digit readout stores, because the pixel
/// adapter rebuilds exactly that. The expression is evaluated at several distances instead of
/// matched by shape, so any expression with that result qualifies and any other is refused.
pub(super) fn readouts(
    context: &expression::Context,
    code: &[u8],
    values: &[[u8; 16]],
    outputs: &BTreeSet<u32>,
) -> Result<Vec<Readout>> {
    let at = |metres: f32| -> Result<BTreeMap<usize, [u8; 16]>> {
        let mut context = context.clone();
        context.distance = Some(metres);
        Ok(context.lower(code, values, outputs, false)?.constants)
    };
    let zero = at(0.)?;
    let samples = [1.25f32, 37.9, 512.3]
        .into_iter()
        .map(|metres| Ok((metres, at(metres)?)))
        .collect::<Result<Vec<_>>>()?;
    let mut found = Vec::new();
    for (&row, value) in &zero {
        let base = lanes(value);
        let mut depends = false;
        let mut whole = true;
        for (metres, rows) in &samples {
            let sampled = rows
                .get(&row)
                .map(lanes)
                .context("Distance readout row is not closed at every distance")?;
            depends |= sampled != base;
            whole &=
                sampled[1..] == base[1..] && (sampled[0] - base[0] - metres.floor()).abs() < 1e-3;
        }
        if depends {
            ensure!(
                whole,
                "Distance readout row {row} is not whole metres plus a constant"
            );
            found.push(Readout { row, base });
        }
    }
    ensure!(
        samples.iter().all(|(_, rows)| rows.keys().eq(zero.keys())),
        "Distance changes which material rows are closed"
    );
    Ok(found)
}

/// Whether the material's fixed textures leave the scope's depth register to the readout. A
/// source program that declares a texture at that register is refused by [`sample`]. A source
/// program's own buffer 2 is its frame buffer, which the transparent adapter closes to constants,
/// so the translated program reads that register only where the readout does.
pub(super) fn free(listing: Option<&str>, material: &Payload) -> Result<()> {
    listing.context("Distance readout has no pixel program")?;
    for texture in material.array(0x280, 24, None)? {
        ensure!(
            material.u32(texture)? != 10,
            "Distance readout conflicts with a fixed texture at the depth register"
        );
    }
    Ok(())
}

/// The replacement for a readout row's load: the sampled whole metres in X.
pub(super) fn row(readout: &Readout) -> String {
    let [x, y, z, w] = readout.base.map(vertex::float);
    format!("float4(rangefinderMetres()+{x},{y},{z},{w})")
}

/// Declare the scope's depth texture and add the sampler the readout rows call. The view centre
/// comes from the native viewport row `cb12[12]`. The distance is clamped to the three digits a
/// readout holds. The caller declares `cb2` and `cb12`.
pub(super) fn sample(mut pixel: String) -> Result<String> {
    ensure!(
        !pixel.contains("register(t10)"),
        "Distance readout conflicts with a source texture at the depth register"
    );
    let entry = pixel.find("void main(").context("Pixel entry")?;
    pixel.insert_str(
        entry,
        "Texture2D<float4> rangefinderDepth:register(t10);\n\
         float rangefinderMetres() {\n\
         float depth=rangefinderDepth.Load(int3(int2(cb12[12].xy*0.5),0)).x;\n\
         return floor(clamp((/* Native reciprocal depth */ 1.0/(depth*cb2[0].y+cb2[0].x)),0.0,999.0));\n\
         }\n",
    );
    Ok(pixel)
}

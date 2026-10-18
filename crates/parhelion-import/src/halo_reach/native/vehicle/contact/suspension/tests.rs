//! Emit calibration cases for independent original-native force replay.
use super::*;
use serde_json::json;
use std::{env, fs, path::PathBuf};

#[test]
#[ignore = "Requires a fresh output directory and independent native consumer replay"]
fn middleware_suspension_calibration() -> Result<()> {
    let output = PathBuf::from(
        env::var_os("PARHELION_SUSPENSION_OUTPUT")
            .ok_or_else(|| anyhow::anyhow!("Set PARHELION_SUSPENSION_OUTPUT"))?,
    );
    ensure!(!output.exists(), "Use a fresh suspension output");
    fs::create_dir_all(&output)?;
    let mut rows = Vec::new();
    for (index, (height, travel, load, ratio, gravity)) in [
        (0.4, 0.3, 1.4, 0.7, 1.),
        (0.4, 0.3, 0.6, 0.7, 1.),
        (0.6, 0.4, 1.2, 0.5, 0.6),
        (0.6, 0.4, 0.8, 0.5, 1.4),
    ]
    .into_iter()
    .enumerate()
    {
        let (rest, stiffness) = parameters(height, travel, load, 3.)?;
        let (slope, coefficient) = damping(
            height,
            height + travel,
            rest,
            stiffness,
            load,
            ratio,
            gravity,
        )?;
        let settings = [
            height + travel,
            rest,
            stiffness * gravity,
            coefficient,
            coefficient,
        ];
        let bytes: Vec<u8> = settings.into_iter().flat_map(f32::to_le_bytes).collect();
        fs::write(output.join(format!("{index}.bin")), bytes)?;
        rows.push(
            json!({"file":format!("{index}.bin"),"height":height,"load":load,
            "target_ratio":ratio,"gravity_scale":gravity,"equilibrium_slope":slope,"maximum":3.}),
        );
    }
    fs::write(
        output.join("cases.json"),
        serde_json::to_vec_pretty(&json!({
            "converter_revision":crate::graph::CONVERTER_REVISION,"cases":rows,
            "limits":["Corner-load calibration cases for independent native replay, not a whole-vehicle terrain simulation."]
        }))?,
    )?;
    Ok(())
}

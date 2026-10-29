//! Select the immediate primary recording without interpreting environment tails as variations.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

pub(super) fn source<'a>(behavior: &'a Value, effect: &Value) -> Result<&'a Value> {
    let sound = crate::halo_reach::audio::fire_sound_from(
        behavior,
        vec![
            effect["tag"]["datum"]
                .as_u64()
                .context("Melee effect identity")?,
        ],
    )?
    .context("Melee effect has no primary source recording")?;
    let mut parts = 0;
    for event in effect["data"]["events"]
        .as_array()
        .context("Melee effect events")?
    {
        for part in event["parts"].as_array().context("Melee effect parts")? {
            if part["tag"] != sound["tag"] {
                continue;
            }
            ensure!(
                event["skip_fraction"] == 0.0
                    && event["delay_seconds"] == json!([0., 0.])
                    && event["flags"] == 0
                    && part["environment"] == 0
                    && part["violence"] == 0
                    && part["flags"] == 0
                    && part["scale_a"] == 0
                    && part["scale_b"] == 0,
                "Melee source impact sound requires delayed or conditional playback"
            );
            parts += 1;
        }
    }
    ensure!(
        parts == 1,
        "Melee primary impact sound is absent or repeated"
    );
    let data = &sound["data"];
    ensure!(
        data["playback"]["skip_fraction"] == 0.0 && data["playback"]["gain_variance_db"] == 0.0,
        "Melee impact sound requires randomized gain or skipping"
    );
    for range in data["pitch_ranges"]
        .as_array()
        .context("Melee impact pitch ranges")?
    {
        ensure!(
            range["variants"]
                .as_array()
                .context("Melee impact variations")?
                .iter()
                .all(|v| v["skip_fraction_encoded"] == 0 && v["gain_encoded"] == 0),
            "Melee impact variations require gain or skipping"
        );
    }
    Ok(sound)
}

//! Source rotor overlays spun through a native pose layer that integrates a rotation rate.
use super::{
    super::curve::Program,
    wheel::{PoseInput, PoseLayer, PoseProgram, phase_duration},
    *,
};
use crate::halo_reach::Scene;

/// The source overlay a vtol's blades turn through, one cycle per prop rotation.
const CLIP: &str = "vehicle:rotate_props";

impl Assembly {
    /// Spin a vtol's blades through its rotor overlay. Reach advances the overlay phase at the
    /// prop rotation speed, in cycles per second from idle to full throttle, and wraps it at one
    /// cycle (MCC U13 5192F4 from 519F52). The native layer adds its parameter times the frame
    /// time and wraps over the clip duration (0104C9D0 option mode 3), so the parameter is that
    /// duration times the speed. The flight thrust output stands in for the source throttle, idle
    /// at zero and full from cruise thrust.
    pub fn rotor(
        &mut self,
        native: &mut Reader,
        graph: &mut Graph,
        scene: &Scene,
        bones: &Value,
        controls: &Value,
        motion: &motion::Motion,
    ) -> Result<Value> {
        let bounds = controls["source"]["prop_rotation_cycles_per_second"]
            .as_array()
            .context("Source prop rotation speed")?
            .iter()
            .map(|value| value.as_f64().map(|v| v as f32))
            .collect::<Option<Vec<_>>>()
            .context("Source prop rotation speed bounds")?;
        let [idle, full] = bounds[..] else {
            anyhow::bail!("Source prop rotation speed is not a pair of bounds");
        };
        ensure!(
            idle.is_finite() && full.is_finite() && idle >= 0. && full >= idle && full > 0.,
            "Source prop rotation speed is outside the rotor domain"
        );
        let declared = patches(graph, "vehicle-motion")?;
        let provider = declared
            .iter()
            .find(|patch| patch["symbol"] == "vehicle-flight-thrust-output")
            .context("Flight thrust provider")?;
        let offset = provider["offset"]
            .as_u64()
            .context("Flight thrust provider offset")?
            .checked_sub(8)
            .context("Flight thrust provider prefix")?;
        let object = links::Object {
            owner: motion::OWNER,
            class: 0x80809ae1,
            offset,
        };
        let contract = links::Interface::read(
            &motion.payload,
            object,
            &graph.read("vehicle-flight-thrust-output")?,
            false,
        )?;
        ensure!(
            contract.methods.len() == 1
                && contract.methods[0].index == 9
                && contract.methods[0].implementation_class == 0x80803cf2,
            "Flight thrust output contract differs"
        );
        let channel = self
            .channel_bridge(native, graph, "vehicle-rotor-channels", &[(object, 0)])?
            .into_iter()
            .next()
            .context("Rotor thrust channel")?;
        let (cruise, _) = super::super::flight::thrust(controls)?;
        let duration = phase_duration(native)?;
        // Reverse thrust saturates to idle, boost to full.
        let program = Program::ramp([0., cruise], [idle * duration, full * duration])?;
        let mut report = self.pose_layers(
            native,
            graph,
            scene,
            bones,
            "vehicle-rotor",
            vec![PoseInput {
                parameter: "source/rotor_thrust".to_owned(),
                channel,
            }],
            vec![PoseProgram {
                parameter: "source/rotor_rate",
                references: vec![0],
                program,
            }],
            vec![PoseLayer {
                clip: CLIP.to_owned(),
                parameter: 1,
                mode: Some(3),
                yields: false,
            }],
        )?;
        report["prop_rotation_cycles_per_second"] = json!([idle, full]);
        report["clip_duration_seconds"] = json!(duration);
        report["full_rate_thrust"] = json!(cruise);
        report["gameplay_verified"] = json!(false);
        report["limits"] = json!([
            "Reach follows engine lift through its gear speed function and stops the blades without a driver. The native rate follows flight thrust, so the blades turn at the idle speed whenever the vehicle exists.",
            "The source overlay is resampled over the native clip. A source loop whose last frame is not one full cycle repeats with a step at the wrap."
        ]);
        Ok(report)
    }
}

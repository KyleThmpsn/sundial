//! A bounded CI selection of the same package, rendering and independent readback workflow.
use super::Case;

pub(super) fn name() -> &'static str {
    match std::env::var("SUNDIAL_PREVIEW_VERIFY_SCOPE").as_deref() {
        Err(std::env::VarError::NotPresent) | Ok("full") => "full",
        Ok("ci") => "ci",
        _ => panic!("SUNDIAL_PREVIEW_VERIFY_SCOPE must be full or ci"),
    }
}

pub(super) fn select(mut cases: Vec<Case>) -> Vec<Case> {
    if name() == "ci" {
        cases.retain(|case| selected(&case.name));
    }
    cases
}

fn selected(name: &str) -> bool {
    // Keep two complete timelines and their rewind controls. The remaining cases cover
    // native mips, geometry, output and independently expected light within the CI budget.
    name.starts_with("offload-")
        || name.starts_with("native-cubic-timeline-")
        || name.starts_with("output-time-")
        || name.starts_with("transparent-order-shared-")
        || name.starts_with("transparent-order-motion-")
        || name.starts_with("transparent-order-pose-")
        || name == "transparent-order-coplanar"
        || matches!(
            name,
            "native-opaque-color-and-depth"
                | "native-unsigned-arithmetic-2"
                | "native-affine-derivative"
                | "native-immediate-1"
                | "native-immediate-2"
                | "native-vertex-image-0"
                | "native-vertex-image-3"
                | "native-layered-3"
                | "native-layered-7"
                | "native-layered-17"
                | "native-layered-24"
                | "native-layered-28"
                | "native-layered-29"
                | "packaged-particle-mesh-composed-false"
                | "packaged-particle-mesh-composed-true"
                | "native-hdr-26-true"
                | "native-placed-detail-9"
                | "native-body-cutoff-0.3"
                | "native-body-cutoff-0.7"
                | "legacy-cloth-0-0"
                | "native-sampling-0"
                | "native-sampling-4"
                | "native-sampling-6"
                | "native-sampling-9"
                | "native-sampling-10"
                | "native-canvas-wide"
                | "skeletal-interval-1"
                | "skeletal-interval-3"
                | "tangent-mirrored-0"
                | "output-ambient-8-1-1-2-0"
                | "output-attenuated"
                | "output-halo"
                | "output-halo-wide"
                | "output-unsupported-intensity"
                | "vehicle-emission-8-1-32"
                | "vehicle-decal-64-false"
                | "vehicle-decal-255-true"
                | "vehicle-framing"
                | "studio-gray"
        )
}

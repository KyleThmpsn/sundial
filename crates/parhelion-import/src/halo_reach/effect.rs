//! Bounded Reach effect properties and their explicit native presentation adapters.
pub(super) mod beam;
pub(super) mod material;
pub(super) mod particle;
pub(super) mod property;
pub(super) mod source;
pub(super) mod trail;

/// Material byte `+0x22` overriding the draw with native rasterizer row 1, solid with no culling.
/// The CPU sprite template's row 2 culls back faces with counterclockwise fronts. The generated
/// camera-facing quads place their corners as the mirror of its program `80FF02AF`, so that row
/// removes every one of them. Drawing both faces also keeps quads with mirrored source sizes.
const BOTH_FACES: u8 = 0x81;

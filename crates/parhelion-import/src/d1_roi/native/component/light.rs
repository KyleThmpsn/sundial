//! Source sight-light transforms and live parameters on a checked native light renderer.
use super::*;
pub(super) mod inputs;
mod program;

const OWNER: u32 = 0x80BBCC83;
const ENTITY: u32 = 0x80BBCC85;

impl Assembly<'_> {
    pub(super) fn light(
        &mut self,
        source_tag: u32,
        ordinal: usize,
        source: &Payload,
        source_entity: &Payload,
        name: &str,
        index: usize,
    ) -> Result<Component> {
        let s = source.pointer(24)?;
        ensure!(
            source.u32(s - 4)? == 0x80801B6F,
            "Source sight-light class differs"
        );
        let mut component = self.component(OWNER, ENTITY, name, index)?;
        let n = component.payload.pointer(24)?;
        ensure!(
            component.payload.u32(n - 4)? == 0x80807134,
            "Native sight-light class differs"
        );
        let connections = source_entity
            .array(0x30, 88, Some(0x8080054E))?
            .into_iter()
            .filter(|&a| {
                source_entity.u32(a + 8).ok() == Some(source_tag)
                    && source_entity.u64(a + 32).ok() == Some(ordinal as u64)
                    && source_entity.u32(a + 24).ok() == Some(0x8080073D)
            })
            .collect::<Vec<_>>();
        ensure!(
            connections.len() == 1,
            "Source light has no unique skeleton provider"
        );
        let link = connections[0];
        ensure!(
            source_entity.u64(link + 16)? == (s + 0x130) as u64
                && source_entity.u32(link + 40)? == 0xAE7D0AED
                && source_entity.u32(link + 48)? == u32::MAX
                && source_entity.u64(link + 72)? == 0x3FFFF
                && source_entity.u64(link + 80)? == 0,
            "Source light requires a different skeleton binding"
        );
        ensure!(
            source.u32(s + 0x150)? == 0x8080052A
                && source.u32(s + 0x154)? == 0
                && source.u32(s + 0x158)? <= 2
                && source.u32(s + 0x15C)? == u32::MAX
                && source.u64(s + 0x160)? == 0
                && source.u32(s + 0x168)? == 0x80801AAA
                && source.u32(s + 0x16C)? == 0x811C9DC5
                && source.u64(s + 0x170)? == 1,
            "Source light pose selection differs"
        );
        let poses = source.array(s + 0x178, 64, Some(0x80801AAA))?;
        ensure!(poses.len() == 1, "Source light requires one local pose");
        let at = poses[0];
        ensure!(
            source.u32(at)? == 0 && source.u32(at + 4)? <= 1 && source.u64(at + 8)? == 0,
            "Source light pose requires another bone binding"
        );
        for offset in (16..52).step_by(4) {
            ensure!(
                source.f32(at + offset)?.is_finite(),
                "Nonfinite source light transform"
            );
        }
        let channels = inputs::names(self.source, source, source_entity, source_tag, ordinal)?;
        activation(source, &mut component.payload)?;
        put(
            &mut component.payload.0,
            n + 0x130,
            &source.bytes::<16>(s + 0x158)?,
        )?;
        array(
            &mut component.payload.0,
            n + 0x150,
            0x80807149,
            &source.bytes::<64>(at)?,
            64,
        )?;
        self.light_inputs(&mut component, &channels)?;
        let source_draw = source.u32(s + 0x110)?;
        ensure!(
            self.source.reference(source_draw)? == 0x80801BD9,
            "Source light draw class differs"
        );
        let draw = self.source.read(source_draw)?;
        ensure!(
            draw.0.len() == 0xB0 && draw.u32(0x94)? == u32::MAX,
            "Source light has unsupported draw resources"
        );
        let template_tag = component.payload.u32(n + 0xF8)?;
        let mut target = (*self.native.tag(template_tag, Some(0x80807140))?).clone();
        ensure!(target.0.len() == 0xD0, "Native light draw size differs");
        for offset in (0..0x8C).step_by(4) {
            ensure!(
                draw.f32(offset)?.is_finite(),
                "Nonfinite source light volume"
            );
        }
        for offset in [0x9C, 0xA0, 0xA4] {
            ensure!(
                draw.f32(offset)?.is_finite(),
                "Nonfinite source light parameter"
            );
        }
        put(&mut target.0, 0, &draw.0[..0x8C])?;
        put(&mut target.0, 0xA8, &draw.bytes::<8>(0x9C)?)?;
        put(&mut target.0, 0xB8, &draw.bytes::<4>(0xA4)?)?;
        let (parameters, supplemental) = self.light_program(&draw, &target, &channels, name)?;
        let draw_symbol = format!("{name}-draw");
        let source_flags = draw.u32(0xA8)?;
        put(&mut target.0, 0xA0, &u32::MAX.to_le_bytes())?;
        put(&mut target.0, 0xA4, &u32::MAX.to_le_bytes())?;
        self.node(
            &draw_symbol,
            template_tag,
            &target.0,
            None,
            &[(0xA0, parameters), (0xA4, supplemental)],
        )?;
        component.patch(n + 0xF8, &draw_symbol)?;
        component.seal()?;
        self.approximations.insert("Sight lights retain source local poses, volumes and supported live parameter expressions on native light shaders. Supplemental light parameters use the source initial color. Rendering across light passes requires gameplay acceptance.".into());
        self.evidence.push(json!({"source_component":source_tag,"resource_class":0x80801B6Fu32,
            "native_resource_class":0x80807134u32,"light":{"source_draw":source_draw,
            "native_template":template_tag,"input_names":channels,"source_flags":source_flags,
            "native_flags":target.u32(0xBC)?,"shading":"native light shader",
            "parameters":"source expression and initial vectors","supplemental_color":"source initial value"},
            "gameplay_verified":false}));
        Ok(component)
    }
}

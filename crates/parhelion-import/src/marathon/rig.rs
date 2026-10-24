//! Source weapon bones on a private native palette, with attachment bindings retained.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

type Matrix = [f32; 16];
const IDENTITY: Matrix = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

fn inverse(m: &Matrix) -> Matrix {
    let mut out = IDENTITY;
    for a in 0..3 {
        for b in 0..3 {
            out[a * 4 + b] = m[b * 4 + a];
        }
        out[a * 4 + 3] = -(0..3).map(|b| out[a * 4 + b] * m[b * 4 + 3]).sum::<f32>();
    }
    out
}
fn components(r: &mut Reader, entity: u32) -> Result<Vec<(std::sync::Arc<Payload>, usize, u32)>> {
    let entity = r.tag(entity, None)?;
    entity
        .array(8, 12, None)?
        .into_iter()
        .map(|row| {
            let owner = r.tag(entity.u32(row)?, None)?;
            let base = owner.pointer(24)?;
            let class = owner.u32(base - 4)?;
            Ok((owner, base, class))
        })
        .collect()
}

pub(super) struct Binding {
    mapping: Vec<u16>,
    attachments: BTreeMap<u32, usize>,
    local: BTreeMap<u32, Vec<u16>>,
    pub bones: u32,
    pub program: String,
    pub report: Value,
}

impl Binding {
    /// Source weapon bones skin to their own bones in a private native palette, which the
    /// translated source controller animates. Geometry keeps its source object positions, and
    /// clip channels carry any native bind difference, so the palette needs no calibration.
    pub fn owned(source: &mut Reader, parts: &[Value], palette: &[u32]) -> Result<Self> {
        let root = hash(parts.first().context("Empty source assembly")?, "entity")?;
        let (entity, owner, base, placements, attached) = Self::skeleton(source, root)?;
        let mut mapping = Vec::new();
        let mut rows = Vec::new();
        for (index, row) in owner
            .array(base + 0xB0, 16, Some(0x8080AF42))?
            .into_iter()
            .enumerate()
        {
            let name = owner.u32(row)?;
            let native = palette
                .iter()
                .position(|n| *n == name)
                .with_context(|| format!("Private palette lacks source bone {name:08X}"))?;
            ensure!(index > 0 || native == 0, "Private palette root differs");
            mapping.push(u16::try_from(native)?);
            rows.push(json!({"source":index,"name":format!("{name:08X}"),"native":native}));
        }
        ensure!(
            palette.len() <= 256,
            "Private weapon palette exceeds byte indices"
        );
        let pre = vec![IDENTITY; palette.len()];
        let mut binding =
            Self::assemble(source, parts, root, mapping, &pre, &placements, &attached)?;
        binding.report = json!({"revision":3,"source_entity":format!("{entity:08X}"),"bones":rows,"attachments":binding.attachments,"local_rigs":binding.report,
            "policy":"Every source weapon bone skins to its own private native bone, animated by the translated source controller.",
            "limits":"Attachment-local source controllers remain untranslated, so local attachment skeletons follow their host bone."});
        Ok(binding)
    }

    /// Source weapon bones skin to the donor's equipment bones, which the donor's own first-person
    /// animation moves. A source bone takes the donor bone of the same name, else its nearest
    /// source ancestor's, and the source root takes the donor root. Geometry keeps its source
    /// object positions, so a mechanism shared by name pivots about the donor's bind.
    pub fn donor(source: &mut Reader, parts: &[Value], bones: &[(u32, i32)]) -> Result<Self> {
        let root = hash(parts.first().context("Empty source assembly")?, "entity")?;
        let (entity, owner, base, placements, attached) = Self::skeleton(source, root)?;
        ensure!(!bones.is_empty(), "Native equipment skeleton is empty");
        ensure!(
            bones.len() <= 256,
            "Native weapon palette exceeds byte indices"
        );
        let mut mapping: Vec<u16> = Vec::new();
        let mut rows = Vec::new();
        for (index, row) in owner
            .array(base + 0xB0, 16, Some(0x8080AF42))?
            .into_iter()
            .enumerate()
        {
            let name = owner.u32(row)?;
            let parent = owner.u32(row + 4)? as i32;
            ensure!(
                (index == 0) == (parent == -1) && parent < index as i32,
                "Invalid source weapon hierarchy"
            );
            let (native, policy) = match bones.iter().position(|(n, _)| *n == name) {
                Some(native) => (native, "named"),
                None if index == 0 => (0, "root"),
                None => (usize::from(mapping[parent as usize]), "ancestor"),
            };
            mapping.push(u16::try_from(native)?);
            rows.push(json!({"source":index,"name":format!("{name:08X}"),"native":native,"policy":policy}));
        }
        let pre = vec![IDENTITY; bones.len()];
        let mut binding =
            Self::assemble(source, parts, root, mapping, &pre, &placements, &attached)?;
        binding.report = json!({"revision":3,"source_entity":format!("{entity:08X}"),"bones":rows,"attachments":binding.attachments,"local_rigs":binding.report,
            "policy":"Every source weapon bone skins to the donor bone of its name, else its nearest named ancestor's, and moves with the donor's own first-person animation.",
            "limits":"Source mechanisms without a donor bone of their name stay fixed to their ancestor. Shared mechanisms pivot about the donor's bind."});
        Ok(binding)
    }

    #[allow(clippy::type_complexity)]
    fn skeleton(
        source: &mut Reader,
        root: u32,
    ) -> Result<(
        u32,
        std::sync::Arc<Payload>,
        usize,
        BTreeMap<u32, usize>,
        BTreeSet<u32>,
    )> {
        let mut skeleton = None;
        for (owner, base, class) in components(source, root)? {
            if class == 0x80809FB7 {
                ensure!(skeleton.is_none(), "Ambiguous source weapon skeleton");
                skeleton = Some((owner.clone(), base));
            }
        }
        let (owner, base) = skeleton.context("Source weapon skeleton")?;
        let mut placements = BTreeMap::new();
        let mut attached = BTreeSet::new();
        for (component, at, class) in components(source, root)? {
            if class == 0x80804A14 {
                for row in component.array(at + 0x3D8, 32, None)? {
                    attached.insert(source.ref64(&component, row + 16)?);
                }
            }
            if class != 0x80809F76 {
                continue;
            }
            for row in component.array(at + 0xA8, 48, None)? {
                ensure!(
                    placements
                        .insert(
                            component.u32(row + 0x28)?,
                            component.u32(row + 0x24)? as usize
                        )
                        .is_none(),
                    "Ambiguous mechanism attachment point"
                );
            }
        }
        Ok((root, owner, base, placements, attached))
    }

    #[allow(clippy::too_many_arguments)]
    fn assemble(
        source: &mut Reader,
        parts: &[Value],
        root: u32,
        mapping: Vec<u16>,
        pre: &[Matrix],
        placements: &BTreeMap<u32, usize>,
        attached: &BTreeSet<u32>,
    ) -> Result<Self> {
        let mut attachments = BTreeMap::new();
        for part in parts {
            let attachment = hash(part, "entity")?;
            if !attached.contains(&attachment) {
                continue;
            }
            for (owner, base, class) in components(source, attachment)? {
                if class != 0x808032C4 || owner.u64(base + 0x148)? == 0 {
                    continue;
                }
                let point = owner.pointer(base + 0x148)?;
                ensure!(
                    owner.u32(point - 4)? == 0x808032C9,
                    "Unsupported mechanism attachment"
                );
                let &bone = placements
                    .get(&owner.u32(point)?)
                    .context("Mechanism attachment placement")?;
                ensure!(
                    bone < mapping.len(),
                    "Mechanism attachment has another skeleton"
                );
                ensure!(
                    attachments.insert(attachment, bone).is_none(),
                    "Ambiguous mechanism attachment"
                );
            }
        }
        let mut local = BTreeMap::new();
        let mut local_reports = Vec::new();
        for part in parts {
            let attachment = hash(part, "entity")?;
            if attachment == root {
                continue;
            }
            for (owner, base, class) in components(source, attachment)? {
                if class != 0x80809FB7 {
                    continue;
                }
                let &parent = attachments
                    .get(&attachment)
                    .context("Independent source skeleton has no attachment host")?;
                let nodes = owner.array(base + 0xB0, 16, Some(0x8080AF42))?;
                let inverse = owner.array(base + 0xD0, 32, Some(0x8080BF47))?;
                ensure!(
                    !nodes.is_empty() && nodes.len() == inverse.len(),
                    "Local attachment skeleton count differs"
                );
                let mut seen = BTreeSet::new();
                for (index, &row) in nodes.iter().enumerate() {
                    let parent = owner.u32(row + 4)? as i32;
                    ensure!(
                        seen.insert(owner.u32(row)?)
                            && (parent == -1 || (parent >= 0 && (parent as usize) < index)),
                        "Invalid local attachment hierarchy"
                    );
                }
                ensure!(
                    local
                        .insert(attachment, vec![mapping[parent]; nodes.len()])
                        .is_none(),
                    "Ambiguous local attachment skeleton"
                );
                local_reports.push(json!({"entity":format!("{attachment:08X}"),"bones":nodes.len(),"native_parent":mapping[parent],"policy":"Local source animation is not translated. Every local influence follows the attachment host."}));
            }
        }
        Ok(Self {
            mapping,
            attachments,
            local,
            bones: u32::try_from(pre.len())?,
            program: program(pre),
            // The caller replaces this with its complete report.
            report: json!(local_reports),
        })
    }

    pub fn vertices(
        &self,
        source: &mut Reader,
        model: &Payload,
        mesh: usize,
        entity: u32,
        positions: &Payload,
        header: &Payload,
    ) -> Result<Vec<[u8; 8]>> {
        match header.u16(6)? {
            0 => {
                ensure!(
                    positions
                        .0
                        .chunks_exact(24)
                        .all(|v| v[6..8] == i16::MAX.to_le_bytes()),
                    "Unskinned vertex lacks homogeneous W"
                );
                let bone = self.mapping[self.attachments.get(&entity).copied().unwrap_or(0)];
                Ok(vec![
                    [
                        255, 0, 0, 0, bone as u8, bone as u8, bone as u8, bone as u8
                    ];
                    positions.0.len() / 24
                ])
            }
            1 => {
                ensure!(
                    !self.attachments.contains_key(&entity) || self.local.contains_key(&entity),
                    "Skinned attachment needs its own palette"
                );
                let weighted = positions.0.chunks_exact(24).any(|v| {
                    crate::tiger::skinning::selector(v).is_ok_and(crate::tiger::skinning::weighted)
                });
                let auxiliary = if weighted {
                    source
                        .tag(source.reference(model.u32(mesh + 24)?)?, None)?
                        .0
                        .clone()
                } else {
                    Vec::new()
                };
                crate::tiger::skinning::native_vertices(
                    &positions.0,
                    &auxiliary,
                    self.local.get(&entity).unwrap_or(&self.mapping),
                )
            }
            code => anyhow::bail!("Unsupported Marathon skin declaration {code}"),
        }
    }
}

fn program(pre: &[Matrix]) -> String {
    let rows = |matrices: Vec<Matrix>| {
        matrices
            .into_iter()
            .flat_map(|m| {
                (0..3).map(move |r| {
                    format!(
                        "float4({})",
                        m[r * 4..r * 4 + 4]
                            .iter()
                            .map(|v| format!("{v:.9}"))
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                })
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        r#"
static const float4 bindPre[{count}]={{{pre}}};
static const float4 bindPost[{count}]={{{post}}};
float3 mechanism(uint bone,float4 p) {{
uint row=bone*3;
float4 q=float4(dot(bindPre[row],p),dot(bindPre[row+1],p),dot(bindPre[row+2],p),p.w);
float3 world=float3(dot(objectData[8+row],q),dot(objectData[9+row],q),dot(objectData[10+row],q));
float3 a=objectData[8].xyz,b=objectData[9].xyz,c=objectData[10].xyz;
float3 relative=world-float3(objectData[8].w,objectData[9].w,objectData[10].w)*p.w;
float3 local=(cross(b,c)*relative.x+cross(c,a)*relative.y+cross(a,b)*relative.z)/dot(a,cross(b,c));
q=float4(local,p.w);
q=float4(dot(bindPost[row],q),dot(bindPost[row+1],q),dot(bindPost[row+2],q),p.w);
return float3(dot(objectData[8],q),dot(objectData[9],q),dot(objectData[10],q));
}}
void deform(float4 p,float3 normal,float3 tangent,float4 weight,uint4 bone,out float3 world,out float3 n,out float3 t) {{
world=0;n=0;t=0;
[unroll] for(uint i=0;i<4;i++) {{
world+=mechanism(bone[i],p)*weight[i];
n+=mechanism(bone[i],float4(normal,0))*weight[i];
t+=mechanism(bone[i],float4(tangent,0))*weight[i];
}}
n=normalize(n);t=normalize(t);
}}
"#,
        count = pre.len() * 3,
        pre = rows(pre.to_vec()),
        post = rows(pre.iter().map(inverse).collect())
    )
}

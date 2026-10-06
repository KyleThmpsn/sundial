//! Native cube payloads store all six faces of each mip before the next mip.
use super::super::*;

#[derive(Clone, Copy, Debug)]
pub(in crate::model_preview) struct Cube {
    pub edge: usize,
    pub levels: usize,
}

impl Cube {
    pub(super) fn load(
        manager: &PackageManager,
        tag: u32,
    ) -> Result<(Self, texture::Texture), String> {
        let entry = manager
            .get_entry(tag)
            .ok_or("Reflection texture header is missing")?;
        if entry.file_type != 32 || !matches!(entry.file_subtype, 1..=3) {
            return Err("Invalid reflection texture type".into());
        }
        let header = manager.read_tag(tag)?;
        let edge = usize::from(u16_at(&header, 14)?);
        let levels = usize::from(
            *header
                .get(23)
                .ok_or("Truncated reflection texture header")?,
        );
        if edge == 0
            || edge > 512
            || u16_at(&header, 16)? as usize != edge
            || u16_at(&header, 18)? != 1
            || u16_at(&header, 20)? != 6
            || levels == 0
            || levels > edge.ilog2() as usize + 1
        {
            return Err("Unsupported reflection texture dimensions".into());
        }
        let large = u32_at(&header, 36)?;
        let payload = if matches!(large, 0 | u32::MAX) {
            entry.reference
        } else {
            large
        };
        let data_entry = manager
            .get_entry(payload)
            .ok_or("Reflection texture pixels are missing")?;
        if data_entry.file_size > 32 * 1024 * 1024 {
            return Err("Reflection texture exceeds the preview budget".into());
        }
        let mut bytes = manager.read_tag(payload)?;
        if payload != entry.reference {
            let tail = manager
                .get_entry(entry.reference)
                .ok_or("Reflection texture mip tail is missing")?;
            if bytes.len().saturating_add(tail.file_size as usize) > 32 * 1024 * 1024 {
                return Err("Reflection texture exceeds the preview budget".into());
            }
            bytes.extend(manager.read_tag(entry.reference)?);
        }
        let format = u32_at(&header, 4)?;
        let cube = Self { edge, levels };
        let size = [edge * 6, (0..levels).map(|i| (edge >> i).max(1)).sum()];
        let mut rgba = vec![0; size[0] * size[1] * 4];
        let mut linear = matches!(format, 10 | 26).then(|| vec![[0.0; 4]; size[0] * size[1]]);
        let mut offset = 0;
        let mut top = 0;
        for level in 0..levels {
            let width = (edge >> level).max(1);
            let length = match format {
                26 | 28 | 29 | 87 | 88 | 91 | 93 => width * width * 4,
                10 => width * width * 8,
                71 | 72 | 80 => width.div_ceil(4).pow(2) * 8,
                74 | 75 | 77 | 78 | 83 | 98 | 99 => width.div_ceil(4).pow(2) * 16,
                _ => return Err(format!("Unsupported reflection texture format {format}")),
            };
            for face in 0..6 {
                let data = bytes
                    .get(offset..offset + length)
                    .ok_or("Truncated reflection texture mip")?;
                let pixels = texture::decode(data, format, width, width)?;
                let floats = texture::float::decode(data, format, width, width)?;
                for y in 0..width {
                    let to = ((top + y) * size[0] + face * edge) * 4;
                    rgba[to..to + width * 4]
                        .copy_from_slice(&pixels[y * width * 4..(y + 1) * width * 4]);
                    if let (Some(atlas), Some(pixels)) = (&mut linear, &floats) {
                        atlas[to / 4..to / 4 + width]
                            .copy_from_slice(&pixels[y * width..(y + 1) * width]);
                    }
                }
                offset += length;
            }
            top += width;
        }
        Ok((
            cube,
            texture::Texture {
                mips: None,
                tag,
                size,
                rgba,
                linear,
            },
        ))
    }

    pub(super) fn sample(
        self,
        texture: &texture::Texture,
        direction: [f32; 3],
        level: f32,
        color: bool,
    ) -> [f32; 4] {
        let (face, uv) = face(direction);
        let level = level.clamp(0.0, self.levels as f32 - 1.0);
        let sample = |level: usize| -> [f32; 4] {
            let width = (self.edge >> level).max(1);
            let top: usize = (0..level).map(|i| (self.edge >> i).max(1)).sum();
            let xy = uv.map(|v| (v * width as f32 - 0.5).clamp(0.0, width as f32 - 1.0));
            let first = xy.map(|v| v.floor() as usize);
            let last = first.map(|v| (v + 1).min(width - 1));
            let t = [xy[0] - first[0] as f32, xy[1] - first[1] as f32];
            let pixel = |x: usize, y: usize, channel: usize| {
                let index = (top + y) * texture.size[0] + face * self.edge + x;
                if let Some(pixels) = &texture.linear {
                    return pixels[index][channel];
                }
                let value = f32::from(texture.rgba[index * 4 + channel]) / 255.0;
                if color && channel < 3 {
                    shader::linear(value)
                } else {
                    value
                }
            };
            std::array::from_fn(|c| {
                let a = pixel(first[0], first[1], c) * (1.0 - t[0])
                    + pixel(last[0], first[1], c) * t[0];
                let b =
                    pixel(first[0], last[1], c) * (1.0 - t[0]) + pixel(last[0], last[1], c) * t[0];
                a * (1.0 - t[1]) + b * t[1]
            })
        };
        let a: [f32; 4] = sample(level.floor() as usize);
        let b = sample((level.ceil() as usize).min(self.levels - 1));
        std::array::from_fn(|i| a[i] + (b[i] - a[i]) * level.fract())
    }

    pub(super) fn lod(self, direction: [f32; 3], dx: [f32; 3], dy: [f32; 3]) -> [f32; 4] {
        let (face, _) = face(direction);
        let components = |v: [f32; 3]| match face {
            0 => [-v[2], -v[1], v[0]],
            1 => [v[2], -v[1], -v[0]],
            2 => [v[0], v[2], v[1]],
            3 => [v[0], -v[2], -v[1]],
            4 => [v[0], -v[1], v[2]],
            _ => [-v[0], -v[1], -v[2]],
        };
        let p = components(direction);
        let footprint = |delta: [f32; 3]| {
            let d = components(delta);
            let uv: [f32; 2] = std::array::from_fn(|i| {
                0.5 * (d[i] * p[2] - p[i] * d[2]) / p[2].powi(2).max(1e-20)
            });
            (uv[0] * uv[0] + uv[1] * uv[1]).sqrt() * self.edge as f32
        };
        let lod = footprint(dx).max(footprint(dy)).max(1e-20).log2();
        [lod.clamp(0.0, self.levels as f32 - 1.0), lod, 0.0, 0.0]
    }
}

pub(super) fn face([x, y, z]: [f32; 3]) -> (usize, [f32; 2]) {
    let [ax, ay, az] = [x.abs(), y.abs(), z.abs()];
    let (face, s, t, major) = if ax >= ay && ax >= az {
        if x >= 0.0 {
            (0, -z, -y, ax)
        } else {
            (1, z, -y, ax)
        }
    } else if ay >= az {
        if y >= 0.0 {
            (2, x, z, ay)
        } else {
            (3, x, -z, ay)
        }
    } else if z >= 0.0 {
        (4, x, -y, az)
    } else {
        (5, -x, -y, az)
    };
    (
        face,
        [
            (s / major.max(1e-20) + 1.0) * 0.5,
            (t / major.max(1e-20) + 1.0) * 0.5,
        ],
    )
}

pub(super) const GLSL: &str = r#"
vec3 nCubeFace(vec3 d){
    vec3 a=abs(d); float f; vec2 uv; float m;
    if(a.x>=a.y && a.x>=a.z){f=d.x>=0.0?0.0:1.0;uv=vec2(d.x>=0.0?-d.z:d.z,-d.y);m=a.x;}
    else if(a.y>=a.z){f=d.y>=0.0?2.0:3.0;uv=vec2(d.x,d.y>=0.0?d.z:-d.z);m=a.y;}
    else{f=d.z>=0.0?4.0:5.0;uv=vec2(d.z>=0.0?d.x:-d.x,-d.y);m=a.z;}
    return vec3((uv/max(m,1e-20)+1.0)*0.5,f);
}
vec4 nCubeLevel(sampler2D atlas,vec3 face,int edge,int level){
    int width=max(edge>>level,1); int top=0;
    for(int i=0;i<level;i++)top+=max(edge>>i,1);
    vec2 p=clamp(face.xy*float(width)-0.5,vec2(0.0),vec2(float(width-1)));
    ivec2 a=ivec2(floor(p)),b=min(a+ivec2(1),ivec2(width-1));
    ivec2 origin=ivec2(int(face.z)*edge,top); vec2 t=fract(p);
    return mix(mix(texelFetch(atlas,origin+a,0),texelFetch(atlas,origin+ivec2(b.x,a.y),0),t.x),
               mix(texelFetch(atlas,origin+ivec2(a.x,b.y),0),texelFetch(atlas,origin+b,0),t.x),t.y);
}
vec4 nCube(sampler2D atlas,vec3 direction,float lod,int edge,int levels){
    vec3 face=nCubeFace(direction); lod=clamp(lod,0.0,float(levels-1));
    return mix(nCubeLevel(atlas,face,edge,int(floor(lod))),nCubeLevel(atlas,face,edge,int(ceil(lod))),fract(lod));
}
vec3 nCubeComponents(vec3 v,int face){
    if(face==0)return vec3(-v.z,-v.y,v.x);
    if(face==1)return vec3(v.z,-v.y,-v.x);
    if(face==2)return vec3(v.x,v.z,v.y);
    if(face==3)return vec3(v.x,-v.z,-v.y);
    if(face==4)return vec3(v.x,-v.y,v.z);
    return vec3(-v.x,-v.y,-v.z);
}
vec4 nCubeLod(vec3 direction,vec3 dx,vec3 dy,int edge,int levels){
    int face=int(nCubeFace(direction).z);
    vec3 p=nCubeComponents(direction,face),x=nCubeComponents(dx,face),y=nCubeComponents(dy,face);
    vec2 u=0.5*(x.xy*p.z-p.xy*x.z)/max(p.z*p.z,1e-20);
    vec2 v=0.5*(y.xy*p.z-p.xy*y.z)/max(p.z*p.z,1e-20);
    float lod=log2(max(max(length(u),length(v))*float(edge),1e-20));
    return vec4(clamp(lod,0.0,float(levels-1)),lod,0.0,0.0);
}
"#;

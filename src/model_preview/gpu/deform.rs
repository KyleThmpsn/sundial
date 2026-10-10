//! Resident geometry, bone skinning, imported motion and retained cloth interpolation.
//! OpenGL 3.3 transform feedback keeps the result available to every material pass.
use super::*;
use crate::model_preview::{animation::Transform, effects::native};
use resource::{Objects, Table};
use std::fmt::Write;

pub(super) struct Prepared {
    source: Vec<[f32; 4]>,
    cloth: Vec<[f32; 4]>,
    shader: String,
    rows: usize,
    cloth_offsets: Vec<(usize, usize)>,
}

impl Prepared {
    pub fn new(model: &Model) -> Option<Self> {
        if model.animation.is_none()
            && model.rigs.is_empty()
            && model.motions.is_empty()
            && model.cloth.is_empty()
        {
            return None;
        }
        let mut cloth = Vec::new();
        let mut cloth_offsets = Vec::new();
        let mut mapping = vec![[-1.0, 0.0, 0.0, 0.0]; model.vertices.len()];
        for (timeline, value) in model.cloth.iter().enumerate() {
            let (vertices, samples) = value.gpu_samples();
            let base = cloth.len();
            let stride = vertices.len() * 3;
            for (i, &vertex) in vertices.iter().enumerate() {
                mapping[vertex] = [timeline as f32, (base + i * 3) as f32, stride as f32, 0.0];
            }
            cloth_offsets.push((base, stride));
            cloth.extend(samples);
        }
        let mut source = Vec::with_capacity(model.vertices.len() * 6);
        for (index, &point) in model.vertices.iter().enumerate() {
            let normal = model.normals.get(index).copied();
            let tangent = model.tangents.get(index).copied();
            source.push([
                point[0],
                point[1],
                point[2],
                f32::from(u8::from(tangent.is_some())),
            ]);
            let n = normal.unwrap_or([0.0; 3]);
            source.push([n[0], n[1], n[2], f32::from(u8::from(normal.is_some()))]);
            source.push(tangent.unwrap_or([0.0, 0.0, 0.0, 1.0]));
            let weights = model.weights.get(index).and_then(Option::as_ref);
            source.push(weights.map_or([0.0; 4], |w| w.values.map(f32::from)));
            source.push(weights.map_or([0.0; 4], |w| w.bones.map(f32::from)));
            source.push(mapping[index]);
        }
        let (shader, rows) = source_code(model);
        Some(Self {
            source,
            cloth,
            shader,
            rows,
            cloth_offsets,
        })
    }
}

pub(super) struct Pipeline {
    objects: Objects,
    program: glow::Program,
    vao: glow::VertexArray,
    source: Table,
    cloth: Table,
    palette: Table,
    output: Table,
    reduction: [Table; 2],
    reduce_program: glow::Program,
    cloth_offsets: Vec<(usize, usize)>,
    seconds: Option<u32>,
    root: Transform,
    depth: Option<([u32; 5], f32)>,
}

impl Pipeline {
    pub unsafe fn new(
        gl: &glow::Context,
        prepared: Prepared,
        count: usize,
    ) -> Result<Self, String> {
        if prepared.cloth.len() > 16_777_216 {
            return Err("The cloth timeline exceeds the GPU indexing budget.".into());
        }
        let mut objects = Objects::default();
        // SAFETY: construction and failure cleanup run on the paint context.
        let built = unsafe {
            (|| {
                let program = objects.program(
                    link_feedback(
                        gl,
                        &prepared.shader,
                        EMPTY_FRAGMENT,
                        &["oPosition", "oNormal", "oTangent"],
                    )
                    .ok_or("GPU deformation shader could not initialize.")?,
                );
                let reduce_program = objects.program(
                    link_feedback(gl, REDUCE, EMPTY_FRAGMENT, &["oValue"])
                        .ok_or("GPU deformation bounds could not initialize.")?,
                );
                let vao = objects.array(gl)?;
                let source = objects.table(gl, &prepared.source, 0)?;
                let cloth = objects.table(gl, &prepared.cloth, 0)?;
                let palette = objects.table(gl, &[], prepared.rows)?;
                let output = objects.table(gl, &[], count * 3)?;
                let reduction = [
                    objects.table(gl, &[], count.div_ceil(256))?,
                    objects.table(gl, &[], count.div_ceil(256))?,
                ];
                Ok::<_, String>((
                    program,
                    reduce_program,
                    vao,
                    source,
                    cloth,
                    palette,
                    output,
                    reduction,
                ))
            })()
        };
        match built {
            Ok((program, reduce_program, vao, source, cloth, palette, output, reduction)) => {
                Ok(Self {
                    objects,
                    program,
                    reduce_program,
                    vao,
                    source,
                    cloth,
                    palette,
                    output,
                    reduction,
                    cloth_offsets: prepared.cloth_offsets,
                    seconds: None,
                    root: Transform::identity(),
                    depth: None,
                })
            }
            Err(error) => {
                // SAFETY: partially created objects belong to this context too.
                unsafe {
                    objects.delete(gl);
                }
                Err(error)
            }
        }
    }

    pub unsafe fn sample(&mut self, gl: &glow::Context, model: &Model, seconds: f32) -> bool {
        if self.seconds == Some(seconds.to_bits()) {
            return false;
        }
        let mut rows = Vec::new();
        for motion in &model.motions {
            let frame = motion.frame(seconds);
            rows.push([f32::from(u8::from(frame.is_some())), 0.0, 0.0, 0.0]);
            rows.extend(frame.unwrap_or([[0.0; 4]; 128]));
        }
        self.root = Transform::identity();
        if let Some(animation) = &model.animation {
            let (palette, root) = animation.palette(seconds);
            self.root = root;
            rows.extend(palette);
        }
        for rig in &model.rigs {
            let (palette, root) = rig.animation.palette(seconds);
            if model.rigs.len() == 1 && rig.vertices.len() == model.vertices.len() {
                self.root = root;
            }
            rows.extend(palette);
        }
        for (timeline, &(base, stride)) in model.cloth.iter().zip(&self.cloth_offsets) {
            let (first, next, mix) = timeline.sample_time(seconds);
            // The static mapping already contains base. Frames add their relative offsets.
            let _ = base;
            rows.push([(first * stride) as f32, (next * stride) as f32, mix, 0.0]);
        }
        // SAFETY: prepared data fixes the maximum output and palette sizes. A completed
        // transform feedback pass is visible to subsequent texture-buffer reads in GL 3.3.
        unsafe {
            gl.bind_buffer(glow::TEXTURE_BUFFER, Some(self.palette.buffer));
            gl.buffer_sub_data_u8_slice(glow::TEXTURE_BUFFER, 0, bytes_of(rows.as_flattened()));
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vao));
            self.source.bind(gl, self.program, "uSource", 0);
            self.palette.bind(gl, self.program, "uPalette", 1);
            self.cloth.bind(gl, self.program, "uCloth", 2);
            feedback(gl, self.output.buffer, model.vertices.len());
        }
        self.seconds = Some(seconds.to_bits());
        self.depth = None;
        true
    }

    pub fn center(&self, bind: [f32; 3]) -> [f32; 3] {
        self.root.point(bind)
    }

    pub unsafe fn bind(&self, gl: &glow::Context, program: glow::Program) {
        // SAFETY: this pipeline and program belong to the caller's active context.
        unsafe {
            self.output.bind(gl, program, "uGeometry", 11);
        }
    }

    pub unsafe fn depth_radius(
        &mut self,
        gl: &glow::Context,
        count: usize,
        center: [f32; 3],
        camera: Camera,
        radius: f32,
    ) -> f32 {
        let key = [
            camera.yaw.to_bits(),
            camera.pitch.to_bits(),
            center[0].to_bits(),
            center[1].to_bits(),
            center[2].to_bits(),
        ];
        if let Some((previous, depth)) = self.depth
            && previous == key
        {
            return depth.max(radius);
        }
        if count == 0 {
            return radius;
        }
        let (sy, cy) = camera.yaw.sin_cos();
        let (sp, cp) = camera.pitch.sin_cos();
        let mut input = self.output;
        let mut count = count;
        let mut pass = 0;
        // SAFETY: each pass reduces into a different buffer. Only one final float is read
        // back, retaining the exact posed depth extent without a CPU mesh copy.
        let value = unsafe {
            gl.use_program(Some(self.reduce_program));
            gl.bind_vertex_array(Some(self.vao));
            let location = |name| gl.get_uniform_location(self.reduce_program, name);
            gl.uniform_3_f32(
                location("uCenter").as_ref(),
                center[0],
                center[1],
                center[2],
            );
            gl.uniform_3_f32(location("uDirection").as_ref(), cp * sy, cp * cy, -sp);
            loop {
                input.bind(gl, self.reduce_program, "uValues", 0);
                gl.uniform_1_i32(location("uCount").as_ref(), count as i32);
                gl.uniform_1_i32(location("uFirst").as_ref(), i32::from(pass == 0));
                let output = self.reduction[pass % 2];
                count = count.div_ceil(256);
                feedback(gl, output.buffer, count);
                if count == 1 {
                    let mut bytes = [0; 4];
                    gl.bind_buffer(glow::COPY_READ_BUFFER, Some(output.buffer));
                    gl.get_buffer_sub_data(glow::COPY_READ_BUFFER, 0, &mut bytes);
                    gl.bind_buffer(glow::COPY_READ_BUFFER, None);
                    break f32::from_ne_bytes(bytes);
                }
                input = output;
                pass += 1;
            }
        };
        let value = if value.is_finite() {
            value.max(radius)
        } else {
            radius
        };
        self.depth = Some((key, value));
        value
    }

    pub unsafe fn delete(self, gl: &glow::Context) {
        // SAFETY: called by the upload owner before releasing its context.
        unsafe {
            self.objects.delete(gl);
        }
    }
}

unsafe fn feedback(gl: &glow::Context, output: glow::Buffer, count: usize) {
    // SAFETY: the caller binds a compatible program and VAO with a sufficiently large
    // output buffer. It must not read that buffer in the same pass.
    unsafe {
        gl.bind_buffer_base(glow::TRANSFORM_FEEDBACK_BUFFER, 0, Some(output));
        gl.enable(glow::RASTERIZER_DISCARD);
        gl.begin_transform_feedback(glow::POINTS);
        gl.draw_arrays(glow::POINTS, 0, count as i32);
        gl.end_transform_feedback();
        gl.disable(glow::RASTERIZER_DISCARD);
        gl.bind_buffer_base(glow::TRANSFORM_FEEDBACK_BUFFER, 0, None);
    }
}

fn source_code(model: &Model) -> (String, usize) {
    let mut shader = String::from(HEADER);
    shader.push_str(native::GPU_HELPERS);
    for (index, motion) in model.motions.iter().enumerate() {
        shader.push_str(&motion.gpu_source(&format!("motion{index}")));
    }
    shader.push_str("void main(){int vertex=gl_VertexID;vec4 original=texelFetch(uSource,vertex*6),basis=texelFetch(uSource,vertex*6+1);vec3 p=original.xyz,n=basis.xyz;vec4 t=texelFetch(uSource,vertex*6+2);\n");
    let mut rows = 0;
    for (index, motion) in model.motions.iter().enumerate() {
        writeln!(shader, "if(vertex>={}&&vertex<{}&&texelFetch(uPalette,{rows}).x!=0.0){{constantBase={};motion{index}(p,n,t);}}", motion.vertices.start, motion.vertices.end, rows+1).unwrap();
        rows += 129;
    }
    if let Some(animation) = &model.animation {
        writeln!(shader, "skin(vertex,{rows},p,n,t);").unwrap();
        rows += animation.bone_count() * 2;
    }
    for rig in &model.rigs {
        writeln!(
            shader,
            "if(vertex>={}&&vertex<{})skin(vertex,{rows},p,n,t);",
            rig.vertices.start, rig.vertices.end
        )
        .unwrap();
        rows += rig.animation.bone_count() * 2;
    }
    writeln!(shader, "vec4 cloth=texelFetch(uSource,vertex*6+5);if(cloth.x>=0.0){{vec4 time=texelFetch(uPalette,{rows}+int(cloth.x));int a=int(cloth.y+time.x),b=int(cloth.y+time.y);p=texelFetch(uCloth,a).xyz*(1.0-time.z)+texelFetch(uCloth,b).xyz*time.z;n=texelFetch(uCloth,a+1).xyz*(1.0-time.z)+texelFetch(uCloth,b+1).xyz*time.z;t.xyz=texelFetch(uCloth,a+2).xyz*(1.0-time.z)+texelFetch(uCloth,b+2).xyz*time.z;}}oPosition=vec4(p,original.w);oNormal=vec4(n,basis.w);oTangent=t;gl_Position=vec4(0.0);}}").unwrap();
    (shader, rows + model.cloth.len())
}

const EMPTY_FRAGMENT: &str = "#version 330 core\nout vec4 color;void main(){color=vec4(0.0);}";
const HEADER: &str = r#"#version 330 core
uniform samplerBuffer uSource,uPalette,uCloth;
out vec4 oPosition,oNormal,oTangent;
int constantBase;
vec4 nConstant(int buffer,int index){
 if(buffer==0)return (index>=0&&index<128)?texelFetch(uPalette,constantBase+index):vec4(0.0);
 if(buffer==11){if(index==5||index==7)return vec4(0,0,0,1);if(index==6)return vec4(1,1,0,0);if(index>=8&&index<=10){vec4 v=vec4(0);v[index-8]=1.0;return v;}}
 if(buffer==12&&index>=0&&index<=3){vec4 v=vec4(0);v[index]=1.0;return v;}return vec4(0);
}
vec3 rotateBone(vec4 q,vec3 p){vec3 t=2.0*cross(q.xyz,p);return p+q.w*t+cross(q.xyz,t);}
vec3 safeNormal(vec3 p){float len=length(p);return len>1e-8&&!isinf(len)?p/len:vec3(0);}
void skin(int vertex,int base,inout vec3 p,inout vec3 n,inout vec4 t){
 vec4 weights=texelFetch(uSource,vertex*6+3),bones=texelFetch(uSource,vertex*6+4);
 float total=weights.x+weights.y+weights.z+weights.w;if(total==0.0)return;
 vec3 point=vec3(0),normal=vec3(0),tangent=vec3(0);
 for(int i=0;i<4;i++){if(weights[i]==0.0)continue;int bone=base+int(bones[i])*2;
  vec4 q=texelFetch(uPalette,bone),s=texelFetch(uPalette,bone+1);float weight=weights[i]/total;
  point+=(rotateBone(q,p*s.w)+s.xyz)*weight;
  if(abs(s.w)>1e-8){normal+=rotateBone(q,n)/s.w*weight;tangent+=rotateBone(q,t.xyz)/s.w*s.w*s.w*weight;}
 }
 p=point;n=safeNormal(normal);t.xyz=safeNormal(tangent);
}
"#;

const REDUCE: &str = r#"#version 330 core
uniform samplerBuffer uValues;
uniform int uCount,uFirst;
uniform vec3 uCenter,uDirection;
out vec4 oValue;
void main(){float result=0.0;int first=gl_VertexID*256;
 for(int i=0;i<256;i++){int at=first+i;if(at>=uCount)break;
  float value=uFirst!=0?abs(dot(texelFetch(uValues,at*3).xyz-uCenter,uDirection)):texelFetch(uValues,at).x;
  if(!isnan(value)&&!isinf(value))result=max(result,value);
 }oValue=vec4(result,0,0,0);gl_Position=vec4(0);}
"#;

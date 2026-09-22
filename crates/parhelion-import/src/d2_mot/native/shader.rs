mod cache;
#[cfg(windows)]
use anyhow::Context;
use anyhow::{Result, ensure};
#[cfg(windows)]
use std::ffi::c_void;

#[cfg(windows)]
#[link(name = "d3dcompiler")]
unsafe extern "system" {
    fn D3DCompile(
        data: *const u8,
        size: usize,
        name: *const u8,
        defines: *const c_void,
        include: *const c_void,
        entry: *const u8,
        target: *const u8,
        flags: u32,
        flags2: u32,
        code: *mut *mut c_void,
        errors: *mut *mut c_void,
    ) -> i32;
}

#[cfg(windows)]
unsafe fn take_blob(blob: *mut c_void) -> Vec<u8> {
    if blob.is_null() {
        return vec![];
    }
    // SAFETY: The caller passes an owned ID3DBlob returned by D3DCompile.
    // Its vtable and buffer remain valid until Release below.
    unsafe {
        let table = *(blob as *const *const usize);
        let data: extern "system" fn(*mut c_void) -> *const u8 = std::mem::transmute(*table.add(3));
        let size: extern "system" fn(*mut c_void) -> usize = std::mem::transmute(*table.add(4));
        let release: extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(*table.add(2));
        let output = std::slice::from_raw_parts(data(blob), size(blob)).to_vec();
        release(blob);
        output
    }
}

#[cfg(windows)]
fn compile_uncached(text: &str, vertex: bool) -> Result<(Vec<u8>, String)> {
    let mut code = std::ptr::null_mut();
    let mut errors = std::ptr::null_mut();
    // SAFETY: Input pointers reference live buffers and nul-terminated strings.
    // D3DCompile initializes the two owned blob pointers consumed by take_blob.
    let (hr, bytes, messages) = unsafe {
        let hr = D3DCompile(
            text.as_ptr(),
            text.len(),
            c"parhelion-source-material".as_ptr().cast(),
            std::ptr::null(),
            std::ptr::null(),
            c"main".as_ptr().cast(),
            if vertex { c"vs_5_0" } else { c"ps_5_0" }.as_ptr().cast(),
            1 << 15,
            0,
            &mut code,
            &mut errors,
        );
        (hr, take_blob(code), take_blob(errors))
    };
    let messages = String::from_utf8(messages).context("compiler diagnostics encoding")?;
    ensure!(hr >= 0, "shader compilation failed: {messages}");
    ensure!(
        bytes.starts_with(b"DXBC"),
        "shader compiler returned invalid bytecode"
    );
    Ok((bytes, messages))
}

#[cfg(not(windows))]
fn compile_uncached(_text: &str, _vertex: bool) -> Result<(Vec<u8>, String)> {
    anyhow::bail!("Native shader conversion requires the Windows D3D compiler")
}

/// Reuse identical translated shaders across materials and donor attempts.
pub fn compile(text: &str, vertex: bool) -> Result<(Vec<u8>, String)> {
    cache::compile(text, vertex)
}

pub fn replace_once(text: &str, old: &str, new: &str) -> Result<String> {
    ensure!(
        text.matches(old).count() == 1,
        "shader contract differs: {old}"
    );
    Ok(text.replacen(old, new, 1))
}

pub fn vertex(text: &str) -> Result<String> {
    ensure!(
        text.matches("float2 v4 : TEXCOORD2").count() == 1
            && text.contains("o2.xyz = v3.www * r0.xyz;"),
        "native vertex input contract differs"
    );
    let text = replace_once(text, "o3.zw = v4.xy * r0.xy;", "o3.zw = v4.xy;")?;
    replace_once(
        &text,
        "out float4 o2 : TEXCOORD2",
        "out float3 o2 : TEXCOORD2",
    )
}

pub fn pixel(text: &str, rect: [usize; 4], size: [usize; 2]) -> Result<String> {
    let [x, y, w, h] = rect;
    let [aw, ah] = size;
    let mut text = text.replace("v3.xy", "source_uv");
    for (slot, sampler) in [(0, 3), (1, 4), (2, 5)] {
        text = replace_once(
            &text,
            &format!("t{slot}.Sample(s{sampler}_s, source_uv)"),
            &format!("t{slot}.SampleGrad(s{sampler}_s, plate_uv, plate_dx, plate_dy)"),
        )?;
    }
    let code = format!(
        "uint4 bitmask, uiDest;\n  float2 source_uv = (v3.xy * float2({aw},{ah}) - float2({x},{y})) / float2({w},{h});\n  float2 plate_uv = (saturate(source_uv) * float2({w},{h}) + float2({x},{y})) / float2({aw},{ah});\n  float2 plate_dx = ddx(v3.xy);\n  float2 plate_dy = ddy(v3.xy);"
    );
    replace_once(&text, "uint4 bitmask, uiDest;", &code)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_ambiguous_or_unknown_contracts() {
        assert!(replace_once("xx", "x", "y").is_err());
        assert!(vertex("void main() {}").is_err());
        assert!(pixel("void main() {}", [0, 0, 8, 8], [8, 8]).is_err());
    }
}

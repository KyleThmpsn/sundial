//! One serialized D3D11 context. Hardware failure disables it for this process.
use super::Input;
use ::windows::{
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0},
            Direct3D11::*,
            Dxgi::IDXGIDevice,
        },
    },
    core::Interface,
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

const MAX_BYTES: usize = 64 * 1024 * 1024;
const THREADS: usize = 64;

struct Device {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    shaders: BTreeMap<&'static str, ID3D11ComputeShader>,
    adapter: String,
}

impl Device {
    fn open() -> Result<Self> {
        let (mut device, mut context) = (None, None);
        // SAFETY: Outputs are valid optional COM slots. Hardware-only selection never substitutes
        // WARP, and the normal Windows GPU timeout remains enabled.
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        let device = device.context("No hardware compute device")?;
        let context = context.context("No hardware compute context")?;
        // SAFETY: The device owns the adapter. GetDesc returns an initialized fixed-size record.
        let description = unsafe { device.cast::<IDXGIDevice>()?.GetAdapter()?.GetDesc()? };
        let end = description
            .Description
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(description.Description.len());
        Ok(Self {
            device,
            context,
            shaders: BTreeMap::new(),
            adapter: String::from_utf16_lossy(&description.Description[..end]),
        })
    }

    fn buffer(
        &self,
        bytes: Option<&[u8]>,
        size: usize,
        stride: usize,
        bind: u32,
        staging: bool,
    ) -> Result<ID3D11Buffer> {
        ensure!(
            size > 0 && size <= MAX_BYTES,
            "Compute buffer exceeds its bounded allocation"
        );
        ensure!(
            bytes.is_none_or(|data| data.len() == size),
            "Compute buffer extent differs"
        );
        let description = D3D11_BUFFER_DESC {
            ByteWidth: u32::try_from(size)?,
            Usage: if staging {
                D3D11_USAGE_STAGING
            } else {
                D3D11_USAGE_DEFAULT
            },
            BindFlags: bind,
            CPUAccessFlags: if staging {
                D3D11_CPU_ACCESS_READ.0 as u32
            } else {
                0
            },
            MiscFlags: if stride > 0 && !staging {
                D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32
            } else {
                0
            },
            StructureByteStride: if staging { 0 } else { u32::try_from(stride)? },
        };
        let initial = bytes.map(|data| D3D11_SUBRESOURCE_DATA {
            pSysMem: data.as_ptr().cast(),
            ..Default::default()
        });
        let mut buffer = None;
        // SAFETY: Initial data covers the declared byte extent and lives through CreateBuffer.
        unsafe {
            self.device.CreateBuffer(
                &description,
                initial.as_ref().map(|data| data as *const _),
                Some(&mut buffer),
            )?;
        }
        buffer.context("Compute buffer creation returned no resource")
    }

    fn shader(&mut self, source: &'static str) -> Result<ID3D11ComputeShader> {
        if let Some(shader) = self.shaders.get(source) {
            return Ok(shader.clone());
        }
        let bytecode = crate::tiger::shader::compute(source)?;
        let mut shader = None;
        // SAFETY: The compiler returned complete cs_5_0 bytecode, consumed during this call.
        unsafe {
            self.device
                .CreateComputeShader(&bytecode, None, Some(&mut shader))?;
        }
        let shader = shader.context("Compute shader creation returned no shader")?;
        self.shaders.insert(source, shader.clone());
        Ok(shader)
    }

    fn run(
        &mut self,
        source: &'static str,
        constants: [u32; 4],
        inputs: &[Input<'_>],
        stride: usize,
        elements: usize,
    ) -> Result<Vec<u8>> {
        ensure!(
            !inputs.is_empty() && inputs.len() <= 8,
            "Invalid compute input count"
        );
        ensure!(
            elements > 0 && stride > 0 && stride.is_multiple_of(4),
            "Invalid compute output layout"
        );
        let groups = elements.div_ceil(THREADS);
        ensure!(groups <= 65_535, "Compute dispatch exceeds the D3D11 limit");
        let bytes = elements
            .checked_mul(stride)
            .context("Compute output size overflow")?;
        let shader = self.shader(source)?;
        let mut resources = Vec::with_capacity(inputs.len());
        for input in inputs {
            ensure!(
                input.stride > 0
                    && input.stride.is_multiple_of(4)
                    && input.bytes.len().is_multiple_of(input.stride),
                "Invalid compute input layout"
            );
            let buffer = self.buffer(
                Some(input.bytes),
                input.bytes.len(),
                input.stride,
                D3D11_BIND_SHADER_RESOURCE.0 as u32,
                false,
            )?;
            let mut view = None;
            // SAFETY: The default view spans the complete structured buffer.
            unsafe {
                self.device
                    .CreateShaderResourceView(&buffer, None, Some(&mut view))?;
            }
            resources.push(Some(view.context("Missing compute input view")?));
        }
        let output = self.buffer(
            None,
            bytes,
            stride,
            D3D11_BIND_UNORDERED_ACCESS.0 as u32,
            false,
        )?;
        let readback = self.buffer(None, bytes, 0, 0, true)?;
        let mut view = None;
        // SAFETY: The default view spans the complete structured output buffer.
        unsafe {
            self.device
                .CreateUnorderedAccessView(&output, None, Some(&mut view))?;
        }
        let views = [Some(view.context("Missing compute output view")?)];
        let constants = constants
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        let constant = self.buffer(
            Some(&constants),
            16,
            0,
            D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            false,
        )?;
        crate::cancellation::check()?;
        // SAFETY: The global lease gives this call sole ownership of the immediate context.
        // Shader guards bound every access, and all resources remain alive through readback.
        unsafe {
            self.context.CSSetShader(&shader, None);
            self.context.CSSetShaderResources(0, Some(&resources));
            self.context
                .CSSetConstantBuffers(0, Some(&[Some(constant)]));
            self.context
                .CSSetUnorderedAccessViews(0, 1, Some(views.as_ptr()), None);
            self.context.Dispatch(u32::try_from(groups)?, 1, 1);
            self.context.ClearState();
            self.context.CopyResource(&readback, &output);
        }
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: Map waits for this bounded dispatch and CopyResource. A successful map remains
        // valid until Unmap, and bytes is exactly the staging buffer's declared extent.
        unsafe {
            self.context
                .Map(&readback, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
        }
        let result = if mapped.pData.is_null() {
            Err(anyhow::anyhow!("Compute readback returned no mapped bytes"))
        } else {
            // SAFETY: The successful buffer map exposes exactly the checked allocation above.
            Ok(unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u8>(), bytes) }.to_vec())
        };
        // SAFETY: Every successful Map above is paired with this Unmap, including a null result.
        unsafe {
            self.context.Unmap(&readback, 0);
        }
        crate::cancellation::check()?;
        result
    }
}

pub(super) fn dispatch(
    source: &'static str,
    constants: [u32; 4],
    inputs: &[Input<'_>],
    stride: usize,
    elements: usize,
) -> Result<(Vec<u8>, String)> {
    static DEVICE: OnceLock<Mutex<Result<Device, String>>> = OnceLock::new();
    let device = DEVICE.get_or_init(|| Mutex::new(Device::open().map_err(|e| format!("{e:#}"))));
    let mut state = crate::cancellation::mutex(device)?;
    let gpu = state
        .as_mut()
        .map_err(|error| anyhow::anyhow!(error.clone()))?;
    let result = gpu.run(source, constants, inputs, stride, elements);
    match result {
        Ok(bytes) => Ok((bytes, gpu.adapter.clone())),
        Err(error) => {
            // SAFETY: The global lease still owns the context exclusively. Release bindings
            // even when preparation or mapping failed. Device removal and
            // unsupported shaders are not retried for every texture in the same conversion.
            unsafe {
                gpu.context.ClearState();
            }
            if !crate::cancellation::is_cancelled(&error) {
                *state = Err(format!("{error:#}"));
            }
            Err(error)
        }
    }
}

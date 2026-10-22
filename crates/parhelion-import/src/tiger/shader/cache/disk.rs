//! Cache entries bind the source key to the DLL that owns the loaded D3DCompile function.
use super::Compiled;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::PathBuf, sync::OnceLock};
use windows::{
    Win32::{
        Foundation::HMODULE,
        System::LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            GetModuleFileNameW, GetModuleHandleExW,
        },
    },
    core::PCWSTR,
};

const MAX_FILE: u64 = 16 * 1024 * 1024;

pub(super) fn compiler() -> Option<[u8; 32]> {
    static IDENTITY: OnceLock<Option<[u8; 32]>> = OnceLock::new();
    *IDENTITY.get_or_init(|| {
        let mut module = HMODULE::default();
        // SAFETY: FROM_ADDRESS interprets this pointer as the address of the loaded function.
        // UNCHANGED_REFCOUNT borrows the module for this lookup without transferring ownership.
        unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                PCWSTR(super::super::D3DCompile as *const () as *const u16),
                &mut module,
            )
            .ok()?;
        }
        let mut path = vec![0u16; 32_768];
        // SAFETY: The live module handle and writable UTF-16 buffer cover the complete call.
        let count = unsafe { GetModuleFileNameW(Some(module), &mut path) } as usize;
        if count == 0 || count >= path.len() {
            return None;
        }
        let path = PathBuf::from(String::from_utf16(&path[..count]).ok()?);
        let mut file = File::open(path).ok()?;
        let mut hash = Sha256::new();
        let mut bytes = [0u8; 64 * 1024];
        loop {
            let count = file.read(&mut bytes).ok()?;
            if count == 0 {
                break;
            }
            hash.update(&bytes[..count]);
        }
        Some(hash.finalize().into())
    })
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    identity: String,
    bytecode: String,
    messages: String,
    sha256: String,
}

pub(super) struct Entry {
    generation: crate::cache::Generation,
    _writer: File,
    identity: String,
    stage: u8,
}

impl Entry {
    pub(super) fn open(source: &[u8; 32], stage: u8) -> Result<Option<Self>> {
        let Some(compiler) = compiler() else {
            return Ok(None);
        };
        let target = match stage {
            0 => c"ps_5_0",
            1 => c"vs_5_0",
            5 => c"cs_5_0",
            _ => return Ok(None),
        };
        let root = std::env::var_os("PARHELION_SHADER_CACHE")
            .map(PathBuf::from)
            .or_else(|| {
                sundial::package_authoring::parhelion_data_directory()
                    .map(|path| path.join("cache/shaders"))
            });
        let Some(root) = root else {
            return Ok(None);
        };
        let mut hash = Sha256::new();
        hash.update(b"parhelion-dxbc-cache-1");
        hash.update(compiler);
        hash.update(source);
        hash.update(target.to_bytes());
        hash.update(super::super::compiler_flags(target).to_le_bytes());
        let identity = hex::encode(hash.finalize());
        let Some(generation) =
            crate::cache::Generation::file(&root.join(hex::encode(source)), &identity)
        else {
            return Ok(None);
        };
        let Some(writer) = generation.writer()? else {
            return Ok(None);
        };
        Ok(Some(Self {
            generation,
            _writer: writer,
            identity,
            stage,
        }))
    }

    fn load(&self) -> Result<Compiled> {
        ensure!(
            std::fs::metadata(self.generation.path())?.len() <= MAX_FILE,
            "Shader cache record is too large"
        );
        let record: Record = serde_json::from_slice(&std::fs::read(self.generation.path())?)?;
        ensure!(
            record.identity == self.identity,
            "Shader cache identity differs"
        );
        let bytes = hex::decode(record.bytecode)?;
        ensure!(
            hex::encode(Sha256::digest(&bytes)) == record.sha256,
            "Shader cache digest differs"
        );
        ensure!(
            bytes.get(4..20) == Some(super::super::dxbc::checksum(&bytes)?.as_slice()),
            "Shader cache checksum differs"
        );
        let range = super::super::dxbc::chunk(&bytes, b"SHEX")?
            .or(super::super::dxbc::chunk(&bytes, b"SHDR")?)
            .context("Shader cache has no executable")?;
        let version = u32::from_le_bytes(
            bytes
                .get(range.start..range.start + 4)
                .context("Shader cache version")?
                .try_into()?,
        );
        ensure!(
            version == ((u32::from(self.stage) << 16) | 0x50),
            "Shader cache stage differs"
        );
        if self.stage != 5 {
            super::super::program::Program::new(
                if self.stage == 1 {
                    super::super::identity::Stage::Vertex
                } else {
                    super::super::identity::Stage::Pixel
                },
                bytes.clone(),
            )?;
        }
        Ok((bytes, record.messages))
    }

    pub(super) fn read(&self) -> Option<Compiled> {
        self.load().ok()
    }

    pub(super) fn write(&self, value: &Compiled) {
        if value.0.len() * 2 + value.1.len() > MAX_FILE as usize {
            return;
        }
        let record = Record {
            identity: self.identity.clone(),
            bytecode: hex::encode(&value.0),
            messages: value.1.clone(),
            sha256: hex::encode(Sha256::digest(&value.0)),
        };
        let _ = crate::cache::write_json(self.generation.path(), &record);
    }
}

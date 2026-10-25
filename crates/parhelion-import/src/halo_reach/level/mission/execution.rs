//! Selected HaloScript control flow compiled for Sunrise's persistent Lua mission host.
//! Native operations require explicit target bindings.
mod binding;
mod compiler;

use super::Program;
use anyhow::{Context, Result, ensure};
pub use binding::{NativeBindings, ObjectBinding};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    /// Explicit entry scripts. Called scripts and dormant wake targets join their closure.
    pub entrypoints: Vec<String>,
    /// Owns the controller's variables and its single timer within this activity.
    pub namespace: String,
    /// Explicit identities for the native operations this controller may request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<NativeBindings>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: u32,
    pub execution: &'static str,
    pub program_sha256: String,
    pub controller_sha256: String,
    pub source_build: String,
    pub source_metadata_sha256: String,
    pub options: Options,
    pub scripts: Vec<usize>,
    pub globals: Vec<usize>,
    pub omitted_scenario_roots: Vec<usize>,
    /// Source expression datum for each instruction, in emitted order. Zero marks synthetic returns.
    pub instruction_sources: Vec<u32>,
    pub limits: Vec<&'static str>,
}

pub struct Controller {
    pub source: String,
    pub report: Report,
}

/// Compiles the complete selected closure before producing any executable output.
pub fn compile(program: &Program, options: &Options) -> Result<Controller> {
    ensure!(
        program.schema == 1 && program.source_build == super::super::super::cache::BUILD,
        "Mission execution requires the MCC Reach U13 source profile"
    );
    ensure!(
        !options.entrypoints.is_empty(),
        "Select at least one mission entry script"
    );
    ensure!(
        !options.namespace.is_empty()
            && options.namespace.len() <= 39
            && options
                .namespace
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_'),
        "Mission namespace must contain 1 to 39 ASCII letters, digits or underscores"
    );
    super::validate(program)?;
    if let Some(native) = &options.native {
        native.validate(program)?;
    }
    let compiled = compiler::compile(program, options)?;
    let payload = serde_json::to_vec(&compiled.payload)?;
    let runtime = include_str!("execution/runtime.lua");
    let mut identity = Sha256::new();
    identity.update(&payload);
    identity.update(runtime.as_bytes());
    let program_sha256 = hex::encode(identity.finalize());
    let source = format!(
        "-- Selected MCC Reach mission control flow for the Sunrise Lua host.\n\
        local P={}\nlocal PROGRAM={:?}\n{}\n",
        compiler::lua(&compiled.payload)?,
        program_sha256,
        runtime
    );
    ensure!(
        source.len() <= 128 * 1024,
        "Selected mission controller exceeds Sunrise's 128 KiB source limit"
    );
    let report = Report {
        schema: 1,
        execution: "selected_scripts",
        program_sha256,
        controller_sha256: hex::encode(Sha256::digest(source.as_bytes())),
        source_build: program.source_build.clone(),
        source_metadata_sha256: program.source_metadata_sha256.clone(),
        options: options.clone(),
        scripts: compiled.scripts,
        globals: compiled.globals,
        omitted_scenario_roots: compiled.omitted,
        instruction_sources: compiled.sites,
        limits: vec![
            "Only explicitly selected scripts and their dependencies execute. This is not a complete campaign controller.",
            "Only explicitly bound constant object activation/removal and mission completion are supported natively. AI, objectives, checkpoints, streaming, cinematics and audio remain unsupported.",
            "Native requests suspend until their matching effect result. transport_staged establishes transport staging, not observed gameplay.",
            "Object bindings select existing authored package entries. They do not import Reach objects.",
            "Source identities are never converted implicitly to target handles.",
            "Sleep intervals use a logical 30 Hz clock paced by authoritative millisecond timers. Timer delivery can delay execution.",
            "sleep_until requires an explicit constant polling interval. Its optional timeout is also constant.",
            "Selected globals initialize before entry scripts. Unselected global initializers do not run.",
            "The controller requires the Sunrise Lua variable, timer and callback transaction contract. Offline execution does not establish gameplay.",
        ],
    };
    Ok(Controller { source, report })
}

/// Writes a self-contained controller and its source mapping into a fresh directory.
/// Package installation is separate and does not implicitly copy this executable sidecar.
pub fn export(program: &Program, options: &Options, output: &Path) -> Result<Report> {
    let controller = compile(program, options)?;
    ensure!(!output.exists(), "Use a fresh mission controller directory");
    let parent = output
        .parent()
        .context("Mission controller directory has no parent")?;
    let temporary = tempfile::Builder::new()
        .prefix(".mission-")
        .tempdir_in(parent)?;
    fs::write(temporary.path().join("controller.lua"), controller.source)?;
    fs::write(
        temporary.path().join("execution.json"),
        serde_json::to_vec_pretty(&controller.report)?,
    )?;
    fs::rename(temporary.path(), output)
        .with_context(|| format!("Publishing mission controller {}", output.display()))?;
    Ok(controller.report)
}

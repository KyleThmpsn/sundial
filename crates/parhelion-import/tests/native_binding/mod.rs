//! Native request lifecycle exercised through emitted Lua and committed host observations.
use super::*;
use parhelion_import::halo_reach::level::mission::binding::Named;

fn options() -> execution::Options {
    execution::Options {
        entrypoints: vec!["main".into(), "clock".into()],
        namespace: "native_test".into(),
        native: Some(execution::NativeBindings {
            activity_id: "authored_activity".into(),
            sdk_build_id: format!("sha256:{}", "a".repeat(64)),
            objects: vec![execution::ObjectBinding {
                source_index: 7,
                source_name: "source_object".into(),
                slot_id: "authored_object".into(),
            }],
            completion: true,
        }),
    }
}

fn source() -> Program {
    let mut f = Fixture { nodes: Vec::new() };
    let initial = f.long(0);
    let mut bindings = Vec::new();
    let mut body = Vec::new();
    for (opcode, progress) in [(96, 1), (103, 2), (1090, 3)] {
        let args = if opcode == 1090 {
            Vec::new()
        } else {
            let object = f.node(78, if opcode == 103 { 70 } else { 78 }, 9, 7);
            bindings.push(Named {
                expression: object,
                value_type: 78,
                value: 7,
                domain: "object_name",
                index: 7,
                name: Some("source_object".into()),
            });
            vec![object]
        };
        body.push(f.op(opcode, 4, &args));
        let value = f.long(progress);
        body.push(f.set(0, 8, value));
        if opcode == 96 {
            let delay = f.long(1);
            body.push(f.op(22, 4, &[delay]));
        }
    }
    let main = f.script("main", 0, 4, &body);
    let delay = f.long(1);
    let sleep = f.op(22, 4, &[delay]);
    let clock = f.script("clock", 0, 4, &[sleep]);
    let mut source = program();
    source.scripts = vec![main, clock];
    source.globals = vec![Global {
        name: "progress".into(),
        value_type: 8,
        root: Some(initial),
    }];
    source.nodes = f.nodes;
    source.bindings.named = bindings;
    source
}

#[test]
#[ignore = "Requires PARHELION_REACH_LUA and fresh PARHELION_REACH_NATIVE_OUTPUT"]
fn native_requests_resume_only_after_matching_results() -> Result<()> {
    let lua = PathBuf::from(env::var_os("PARHELION_REACH_LUA").context("Set Lua runner")?);
    let output =
        PathBuf::from(env::var_os("PARHELION_REACH_NATIVE_OUTPUT").context("Set native output")?);
    ensure!(!output.exists(), "Use a fresh native output");
    fs::create_dir_all(&output)?;
    let source = source();
    let options = options();
    fs::write(
        output.join("source.json"),
        serde_json::to_vec_pretty(&source)?,
    )?;
    fs::write(
        output.join("options.json"),
        serde_json::to_vec_pretty(&options)?,
    )?;
    execution::export(&source, &options, &output.join("controller"))?;
    let harness = output.join("native.lua");
    fs::write(&harness, include_str!("native.lua"))?;
    let run = Command::new(&lua)
        .arg(&harness)
        .arg(output.join("controller/controller.lua"))
        .output()?;
    fs::write(output.join("stdout.txt"), &run.stdout)?;
    fs::write(output.join("stderr.txt"), &run.stderr)?;
    ensure!(
        run.status.success(),
        "Native lifecycle: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let observed: serde_json::Value = serde_json::from_slice(&run.stdout)?;
    ensure!(observed["progress"] == 3 && observed["actions"] == 3 && observed["rollback"] == true);
    for case in 0..5 {
        let mut invalid = options.clone();
        let native = invalid.native.as_mut().unwrap();
        match case {
            0 => native.objects[0].source_name = "stale".into(),
            1 => native.objects.push(native.objects[0].clone()),
            2 => native.completion = false,
            3 => native.sdk_build_id = "unversioned".into(),
            _ => invalid.native = None,
        }
        ensure!(
            execution::export(&source, &invalid, &output.join(format!("invalid-{case}"))).is_err()
        );
        ensure!(!output.join(format!("invalid-{case}")).exists());
    }
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&json!({
            "passed":true,"observed":observed,
            "source_sha256":hex::encode(Sha256::digest(fs::read(output.join("source.json"))?)),
            "controller_sha256":hex::encode(Sha256::digest(fs::read(output.join("controller/controller.lua"))?)),
            "lua_sha256":hex::encode(Sha256::digest(fs::read(lua)?)),
            "harness_sha256":hex::encode(Sha256::digest(fs::read(harness)?)),
            "limits":["Transactional adapter execution, not native gameplay or object import."]
        }))?,
    )?;
    Ok(())
}

#[test]
#[ignore = "Requires Lua, PARHELION_REACH_NATIVE_CACHE, its SHA256 and fresh source native output"]
fn source_completion_requests_the_bound_activity() -> Result<()> {
    let required = |name| env::var(name).with_context(|| format!("Set {name}"));
    let cache = PathBuf::from(required("PARHELION_REACH_NATIVE_CACHE")?);
    let mut file = fs::File::open(&cache)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let size = std::io::Read::read(&mut file, &mut buffer)?;
        if size == 0 {
            break;
        }
        digest.update(&buffer[..size]);
    }
    let digest = hex::encode(digest.finalize());
    ensure!(digest == required("PARHELION_REACH_NATIVE_CACHE_SHA256")?);
    let source = parhelion_import::halo_reach::level::mission::read(&cache)?;
    let output = PathBuf::from(required("PARHELION_REACH_SOURCE_NATIVE_OUTPUT")?);
    ensure!(!output.exists(), "Use a fresh source native output");
    fs::create_dir_all(&output)?;
    let mut options = options();
    options.entrypoints = vec!["end_mission".into()];
    options.native.as_mut().unwrap().objects.clear();
    let report = execution::export(&source, &options, &output.join("controller"))?;
    fs::write(
        output.join("source.json"),
        serde_json::to_vec_pretty(&source)?,
    )?;
    let harness = output.join("completion.lua");
    fs::write(&harness, include_str!("completion.lua"))?;
    let lua = PathBuf::from(required("PARHELION_REACH_LUA")?);
    let run = Command::new(&lua)
        .arg(&harness)
        .arg(output.join("controller/controller.lua"))
        .output()?;
    fs::write(output.join("stdout.txt"), &run.stdout)?;
    fs::write(output.join("stderr.txt"), &run.stderr)?;
    ensure!(
        run.status.success(),
        "Source completion: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let observed: serde_json::Value = serde_json::from_slice(&run.stdout)?;
    ensure!(observed["completion_requests"] == 1 && observed["resumed"] == true);
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&json!({
            "passed":true,"cache":cache.file_name(),"source_sha256":digest,
            "controller":report,"observed":observed,
            "lua_sha256":hex::encode(Sha256::digest(fs::read(lua)?)),
            "harness_sha256":hex::encode(Sha256::digest(fs::read(harness)?)),
            "limits":["Actual U13 source-to-Lua completion request. Adapter, not native gameplay."]
        }))?,
    )?;
    Ok(())
}

//! Execute emitted mission controllers across fresh Lua VMs and persisted host state.
use anyhow::{Context, Result, ensure};
use parhelion_import::halo_reach::{
    Tag,
    level::mission::{Global, Node, Parameter, Program, Script, execution},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf, process::Command};

struct Fixture {
    nodes: Vec<Option<Node>>,
}

mod native_binding;

impl Fixture {
    fn node(&mut self, opcode: u16, value_type: u16, flags: u16, value: u32) -> u32 {
        let datum = 0x1234_0000 | self.nodes.len() as u32;
        self.nodes.push(Some(Node {
            datum,
            opcode,
            value_type,
            flags,
            value,
            next: None,
            source_offset: 0,
            line: 1,
            padding: 0,
            text: None,
            text_bytes: None,
        }));
        datum
    }

    fn long(&mut self, value: u32) -> u32 {
        self.node(8, 8, 9, value)
    }
    fn boolean(&mut self, value: bool) -> u32 {
        self.node(5, 5, 9, u32::from(value))
    }
    fn global(&mut self, index: u32, ty: u16) -> u32 {
        self.node(ty, ty, 13, index)
    }
    fn link(&mut self, args: &[u32]) {
        for pair in args.windows(2) {
            self.nodes[(pair[0] & 0xffff) as usize]
                .as_mut()
                .unwrap()
                .next = Some(pair[1]);
        }
    }
    fn op(&mut self, opcode: u16, ty: u16, args: &[u32]) -> u32 {
        self.link(args);
        let head = self.node(opcode, 2, 9, 0);
        self.nodes[(head & 0xffff) as usize].as_mut().unwrap().next = args.first().copied();
        self.node(opcode, ty, 8, head)
    }
    fn set(&mut self, index: u32, ty: u16, value: u32) -> u32 {
        let target = self.global(index, ty);
        self.op(6, ty, &[target, value])
    }
    fn script(&mut self, name: &str, kind: u16, ty: u16, args: &[u32]) -> Script {
        let root = self.op(0, ty, args);
        Script {
            name_id: 0,
            name: name.into(),
            kind,
            return_type: ty,
            root: Some(root),
            parameters: Vec::new(),
        }
    }
}

// Human-readable behavioral oracle:
// main = counter <- add_wait(2), false branch <- 7, short-circuit a write,
//        wake worker, wait until done, result <- counter + woken.
// add_wait(a) = counter <- counter + a, sleep 2 ticks, return counter + a.
// worker = sleep 0 (yields one controller tick), woken <- 10, done <- true.
// ticker = ticks <- ticks + 1, stop permanently when ticks >= 6.
fn program() -> Program {
    let mut f = Fixture { nodes: Vec::new() };
    let mut globals = Vec::new();
    for (name, ty) in [
        ("counter", 8),
        ("branch", 8),
        ("done", 5),
        ("ticks", 8),
        ("result", 8),
        ("woken", 8),
    ] {
        let root = f.node(ty, ty, 9, 0);
        globals.push(Global {
            name: name.into(),
            value_type: ty,
            root: Some(root),
        });
    }
    globals[0].root = Some(f.global(1, 8));
    let two = f.long(2);
    // Direct-call child starts with a script-name leaf, as in compiled U13 caches.
    let call_name = f.node(1, 2, 9, 0);
    f.nodes[(call_name & 0xffff) as usize]
        .as_mut()
        .unwrap()
        .next = Some(two);
    let call = f.node(1, 8, 10, call_name);
    let assign = f.set(0, 8, call);
    let condition = f.boolean(false);
    let bad = f.long(99);
    let bad = f.set(1, 8, bad);
    let good = f.long(7);
    let good = f.set(1, 8, good);
    let branch = f.op(4, 8, &[condition, bad, good]);
    let short = f.boolean(false);
    let bad = f.long(999);
    let bad = f.set(0, 8, bad);
    let short = f.op(7, 5, &[short, bad]);
    let worker = f.node(10, 10, 9, 2);
    let wake = f.op(26, 4, &[worker]);
    let done = f.global(2, 5);
    let interval = f.long(1);
    let timeout = f.long(8);
    let wait = f.op(24, 5, &[done, interval, timeout]);
    let never = f.boolean(false);
    let interval = f.long(2);
    let timeout = f.long(3);
    let expire = f.op(24, 5, &[never, interval, timeout]);
    let counter = f.global(0, 8);
    let woken = f.global(5, 8);
    let sum = f.op(9, 6, &[counter, woken]);
    let result = f.set(4, 8, sum);
    let main = f.script(
        "main",
        0,
        4,
        &[assign, branch, short, wake, wait, expire, result],
    );
    let counter = f.global(0, 8);
    let arg = f.node(8, 8, 29, 0);
    let sum = f.op(9, 6, &[counter, arg]);
    let assign = f.set(0, 8, sum);
    let delay = f.long(2);
    let sleep = f.op(22, 4, &[delay]);
    let counter = f.global(0, 8);
    let arg = f.node(8, 8, 29, 0);
    let sum = f.op(9, 6, &[counter, arg]);
    let mut add = f.script("add_wait", 3, 8, &[assign, sleep, sum]);
    add.parameters.push(Parameter {
        name: "amount".into(),
        value_type: 8,
    });
    let delay = f.long(0);
    let sleep = f.op(22, 4, &[delay]);
    let ten = f.long(10);
    let woken = f.set(5, 8, ten);
    let yes = f.boolean(true);
    let done = f.set(2, 5, yes);
    let worker = f.script("worker", 1, 4, &[sleep, woken, done]);
    let ticks = f.global(3, 8);
    let one = f.long(1);
    let sum = f.op(9, 6, &[ticks, one]);
    let step = f.set(3, 8, sum);
    let ticks = f.global(3, 8);
    let six = f.long(6);
    let stop = f.op(20, 5, &[ticks, six]);
    let forever = f.op(23, 4, &[]);
    let stop = f.op(4, 4, &[stop, forever]);
    let ticker = f.script("ticker", 2, 4, &[step, stop]);
    Program {
        schema: 1,
        source_build: "Jun 21 2023 15:35:31".into(),
        source_metadata_sha256: "0".repeat(64),
        scenario: Tag {
            datum: 0x1234_ffff,
            group: "scnr".into(),
            path: "fixture".into(),
            offset: None,
        },
        execution: "unsupported",
        scripts: vec![main, add, worker, ticker],
        globals,
        nodes: f.nodes,
        tag_references: Vec::new(),
        bindings: Default::default(),
        analysis: Default::default(),
        audio: json!(null),
    }
}

#[test]
#[ignore = "Requires PARHELION_REACH_LUA and a fresh PARHELION_REACH_EXECUTION_OUTPUT"]
fn emitted_controller_resumes_waits_and_preserves_host_transactions() -> Result<()> {
    let lua = PathBuf::from(env::var_os("PARHELION_REACH_LUA").context("Set PARHELION_REACH_LUA")?);
    let output = PathBuf::from(
        env::var_os("PARHELION_REACH_EXECUTION_OUTPUT")
            .context("Set PARHELION_REACH_EXECUTION_OUTPUT")?,
    );
    ensure!(!output.exists(), "Use a fresh execution output");
    fs::create_dir_all(&output)?;
    let program = program();
    fs::write(
        output.join("source.json"),
        serde_json::to_vec_pretty(&program)?,
    )?;
    let options = execution::Options {
        entrypoints: vec!["main".into(), "ticker".into()],
        namespace: "reach_test".into(),
        native: None,
    };
    execution::export(&program, &options, &output.join("controller"))?;
    let harness = output.join("lifecycle.lua");
    fs::write(&harness, include_str!("halo_reach_execution/lifecycle.lua"))?;
    let run = Command::new(&lua)
        .arg(&harness)
        .arg(output.join("controller/controller.lua"))
        .output()?;
    fs::write(output.join("stdout.txt"), &run.stdout)?;
    fs::write(output.join("stderr.txt"), &run.stderr)?;
    ensure!(
        run.status.success(),
        "Lua execution: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let observed: serde_json::Value = serde_json::from_slice(&run.stdout)?;
    ensure!(observed["result"] == 14 && observed["branch"] == 7 && observed["ticks"] == 6);
    ensure!(observed["reloads"].as_u64().unwrap_or(0) >= 5);
    ensure!(observed["duplicate_events_ignored"] == true && observed["rollback"] == true);
    let mut invalid = program;
    invalid.nodes[0].as_mut().unwrap().flags = 8;
    invalid.nodes[0].as_mut().unwrap().opcode = 0x7fff;
    ensure!(execution::export(&invalid, &options, &output.join("invalid")).is_err());
    ensure!(
        !output.join("invalid").exists(),
        "Rejected controller left output behind"
    );
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&json!({
            "passed":true, "observed":observed,
            "source_sha256":hex::encode(Sha256::digest(fs::read(output.join("source.json"))?)),
            "controller_sha256":hex::encode(Sha256::digest(fs::read(output.join("controller/controller.lua"))?)),
            "lua_sha256":hex::encode(Sha256::digest(fs::read(lua)?)),
            "harness_sha256":hex::encode(Sha256::digest(fs::read(harness)?)),
            "limits":["Emitted Lua execution with a transactional host adapter. No in-game mission acceptance."]
        }))?,
    )?;
    Ok(())
}

#[test]
#[ignore = "Requires Reach caches, Lua, an execution witness and fresh source execution output"]
fn source_scripts_execute_and_unbound_campaign_roots_are_refused() -> Result<()> {
    use parhelion_import::halo_reach::level::mission;
    let required = |key| -> Result<PathBuf> {
        Ok(PathBuf::from(
            env::var_os(key).with_context(|| format!("Set {key}"))?,
        ))
    };
    let maps = required("PARHELION_REACH_MAPS")?;
    let lua = required("PARHELION_REACH_LUA")?;
    let witness_bytes = fs::read(required("PARHELION_REACH_EXECUTION_WITNESS")?)?;
    let witness: serde_json::Value = serde_json::from_slice(&witness_bytes)?;
    let cases = witness["cases"]
        .as_array()
        .filter(|v| !v.is_empty())
        .context("Required execution corpus is empty")?;
    let output = required("PARHELION_REACH_SOURCE_EXECUTION_OUTPUT")?;
    ensure!(!output.exists(), "Use a fresh source execution output");
    fs::create_dir_all(&output)?;
    let harness = output.join("source.lua");
    fs::write(&harness, include_str!("halo_reach_execution/source.lua"))?;
    let mut receipts = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let cache = case["cache"].as_str().context("Witness cache")?;
        ensure!(
            std::path::Path::new(cache)
                .file_name()
                .is_some_and(|v| v == cache)
                && !std::path::Path::new(cache).is_absolute(),
            "Invalid cache filename"
        );
        let path = maps.join(cache);
        // Stream hashing so a configured real cache does not need a second complete RAM copy.
        let mut file = fs::File::open(&path)?;
        let mut hash = Sha256::new();
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            let count = std::io::Read::read(&mut file, &mut buffer)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        let source_hash = hex::encode(hash.finalize());
        ensure!(
            case["source_sha256"] == source_hash,
            "Execution witness source differs"
        );
        let program = mission::read(&path)?;
        let options = execution::Options {
            entrypoints: serde_json::from_value(case["entrypoints"].clone())?,
            namespace: "source_test".into(),
            native: None,
        };
        let folder = output.join(index.to_string());
        let report = execution::export(&program, &options, &folder)?;
        fs::write(
            folder.join("source.json"),
            serde_json::to_vec_pretty(&program)?,
        )?;
        let run = Command::new(&lua)
            .arg(&harness)
            .arg(folder.join("controller.lua"))
            .arg(
                case["global_index"]
                    .as_u64()
                    .context("Witness global index")?
                    .to_string(),
            )
            .arg(
                case["timer_ms"]
                    .as_u64()
                    .context("Witness timer interval")?
                    .to_string(),
            )
            .output()?;
        fs::write(folder.join("stdout.txt"), &run.stdout)?;
        fs::write(folder.join("stderr.txt"), &run.stderr)?;
        ensure!(
            run.status.success(),
            "Source Lua execution: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        let observed: serde_json::Value = serde_json::from_slice(&run.stdout)?;
        ensure!(observed["resumed"] == true && observed["global"] == false);
        let rejected = execution::Options {
            entrypoints: serde_json::from_value(case["rejected_entrypoints"].clone())?,
            namespace: "rejected".into(),
            native: None,
        };
        ensure!(
            !rejected.entrypoints.is_empty(),
            "Witness omits native refusal case"
        );
        let error = execution::export(&program, &rejected, &folder.join("rejected"))
            .err()
            .context("Unbound source mission was exported")?;
        let refusal = format!("{error:#}");
        ensure!(
            refusal.contains(
                case["refusal_contains"]
                    .as_str()
                    .context("Refusal witness")?
            ),
            "Unexpected source refusal: {refusal}"
        );
        ensure!(!folder.join("rejected").exists());
        receipts.push(json!({"cache":cache,"source_sha256":source_hash,
            "controller":report,"observed":observed,"refusal":refusal}));
    }
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&json!({
            "passed":true,"cases":receipts,
            "witness_sha256":hex::encode(Sha256::digest(&witness_bytes)),
            "lua_sha256":hex::encode(Sha256::digest(fs::read(lua)?)),
            "harness_sha256":hex::encode(Sha256::digest(fs::read(harness)?)),
            "limits":["Real source scripts executed in the Lua adapter. Native gameplay bindings remain unsupported."]
        }))?,
    )?;
    Ok(())
}

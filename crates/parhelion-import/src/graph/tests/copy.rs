use super::*;

fn fixture(source: &Path, section: &str, field: &str, path: &str) -> GraphReference {
    fs::create_dir_all(source).unwrap();
    fs::write(source.join("parent.bin"), b"parent").unwrap();
    let mut animation = json!({"status":"linked", "files":{}, "clips":[]});
    if field.starts_with("pose_layers/") {
        animation["pose_layers"] = json!({"file":"parent.bin", "template_file":"parent.bin"});
        *animation.pointer_mut(&format!("/{field}")).unwrap() = path.into();
    } else {
        let (field, member) = field.split_once('/').unwrap();
        animation[field] = json!([{"file":"parent.bin"}]);
        animation[field][0][member] = path.into();
    }
    let mut graph = json!({"item_hash":42,"nodes":[{"symbol":"parent","file":"parent.bin"}]});
    if section == "equipment_animation" {
        graph[section] = animation;
    } else {
        graph["animation"][section] = animation;
    }
    fs::write(
        source.join("asset-graph.json"),
        serde_json::to_vec_pretty(&graph).unwrap(),
    )
    .unwrap();
    GraphReference::new(source, 42).unwrap()
}

#[test]
fn promoted_payloads_are_checked_before_any_copy_is_written() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let output_parent = root.path().join("destination");
    fs::create_dir(&output_parent).unwrap();
    fs::write(root.path().join("outside.bin"), b"source outside").unwrap();
    let sentinel = output_parent.join("outside.bin");
    fs::write(&sentinel, b"destination sentinel").unwrap();
    let mut cases = Vec::new();
    for section in ["first_person", "character", "equipment_animation"] {
        for field in [
            "rigs/file",
            "rigs/template_file",
            "rigs/original_file",
            "extra_clips/file",
            "extra_clips/template_file",
            "pose_layers/file",
            "pose_layers/template_file",
        ] {
            for path in [
                "../outside.bin".to_owned(),
                sentinel.to_string_lossy().into_owned(),
                String::new(),
                "missing.bin".to_owned(),
            ] {
                let reference = fixture(&source, section, field, &path);
                let output = output_parent.join("copy");
                let error = reference.copy_model(42, 43, &output).unwrap_err();
                assert!(!output.exists(), "{section}/{field}: {error:#}");
                assert_eq!(fs::read(&sentinel).unwrap(), b"destination sentinel");
                reference.validate(42).unwrap();
                cases.push(json!({"section":section,"field":field,"path":path,"error":format!("{error:#}")}));
            }
        }
    }
    if let Some(artifacts) = std::env::var_os("SUNDIAL_TEST_ARTIFACTS") {
        let artifacts = PathBuf::from(artifacts).join("graph-copy");
        fs::create_dir_all(&artifacts).unwrap();
        fs::write(
            artifacts.join("rejections.json"),
            serde_json::to_vec_pretty(&cases).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn promoted_source_links_cannot_escape_the_graph() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let outside = root.path().join("outside");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("payload.bin"), b"external sentinel").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, source.join("link")).unwrap();
    #[cfg(windows)]
    assert!(
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(source.join("link"))
            .arg(&outside)
            .output()
            .unwrap()
            .status
            .success()
    );
    for section in ["first_person", "character", "equipment_animation"] {
        for field in ["rigs/file", "extra_clips/file", "pose_layers/file"] {
            let reference = fixture(&source, section, field, "link/payload.bin");
            let output = root.path().join("copy");
            assert!(reference.copy_model(42, 43, &output).is_err());
            assert!(!output.exists());
            assert_eq!(
                fs::read(outside.join("payload.bin")).unwrap(),
                b"external sentinel"
            );
        }
    }
}

#[test]
fn safe_legacy_payloads_survive_copy_reload_and_content_pinning() {
    let root = tempfile::tempdir().unwrap();
    for section in ["first_person", "character", "equipment_animation"] {
        for field in [
            "rigs/file",
            "rigs/template_file",
            "rigs/original_file",
            "extra_clips/file",
            "pose_layers/file",
            "pose_layers/template_file",
        ] {
            let source = root.path().join("source");
            let reference = fixture(&source, section, field, "motion/payload.bin");
            fs::create_dir_all(source.join("motion")).unwrap();
            fs::write(source.join("motion/payload.bin"), b"private motion").unwrap();
            let output = root
                .path()
                .join(format!("{}-{}", section, field.replace('/', "-")));
            let copied = reference.copy_model(42, 43, &output).unwrap();
            let reloaded = GraphReference::new(&output, 43).unwrap();
            assert_eq!(reloaded.sha256, copied.sha256);
            assert_eq!(
                fs::read(output.join("motion/payload.bin")).unwrap(),
                b"private motion"
            );
            fs::write(output.join("motion/payload.bin"), b"changed motion").unwrap();
            assert!(copied.validate(43).is_err());
            reference.validate(42).unwrap();
        }
    }
}

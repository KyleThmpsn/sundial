//! Review-to-install content reuse, exercised through the public filesystem lifecycle.
//! Failure cases are recorded before implementation in the validation-reuse investigation.
use super::*;
use std::time::Instant;
use sundial::package_authoring::account::read_authored_account_source;

const CHILD: &str = "PARHELION_VALIDATION_REUSE_CHILD";
const ROOT: &str = "PARHELION_VALIDATION_REUSE_ROOT";
const TEST: &str =
    "install::tests::validation_reuse::reviewed_content_is_reused_without_accepting_changed_inputs";

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES, SUNRISE_MOD_LOADER_DLL, PARHELION_REUSE_STAGE and SUNDIAL_TEST_ARTIFACTS"]
fn reviewed_content_is_reused_without_accepting_changed_inputs() {
    if let Ok(schema) = std::env::var(CHILD) {
        exercise(schema.parse().unwrap());
        return;
    }
    let root = tempfile::tempdir().unwrap();
    for schema in [8, 18, 3] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                TEST,
                "--exact",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, schema.to_string())
            .env(ROOT, root.path());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "schema {schema}:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        println!("{}", String::from_utf8_lossy(&output.stdout));
    }
}

fn fixture() -> (Fixture, ManifestDocument) {
    let source = crate::test_support::stock_packages();
    let stage = PathBuf::from(std::env::var_os("PARHELION_REUSE_STAGE").unwrap());
    let mut manifest: ManifestDocument =
        sundial::package_authoring::read_json(File::open(stage.join(MANIFEST_FILE_NAME)).unwrap())
            .unwrap();
    manifest.validate().unwrap();
    assert!(!manifest.selected_recipe_files.is_empty());
    assert!(!manifest.artifacts.is_empty());
    assert!(manifest.mods.iter().all(|member| !member.kept));
    assert!(
        manifest.mods.len() > 1,
        "The required corpus must contain importer members"
    );
    let temporary = tempfile::tempdir_in(std::env::var_os(ROOT).unwrap()).unwrap();
    let target = temporary.path().join("packages");
    let staging = temporary.path().join("staging");
    fs::create_dir(&target).unwrap();
    fs::create_dir(&staging).unwrap();
    // Stock inputs are immutable in this workflow. Only independent staged copies are edited.
    for entry in fs::read_dir(&source).unwrap() {
        let entry = entry.unwrap();
        if has_pkg_extension(&entry.path()) {
            fs::hard_link(entry.path(), target.join(entry.file_name())).unwrap();
        }
    }
    let runtime = temporary.path().join("bin/x64");
    fs::create_dir_all(&runtime).unwrap();
    fs::write(temporary.path().join("destiny2.exe"), b"fixture marker").unwrap();
    fs::copy(
        crate::test_support::mod_loader_runtime(),
        runtime.join("steam_api64.dll"),
    )
    .unwrap();
    fs::copy(
        source.parent().unwrap().join("bin/x64/oo2core_3_win64.dll"),
        runtime.join("oo2core_3_win64.dll"),
    )
    .unwrap();
    for name in manifest
        .artifacts
        .iter()
        .chain(&manifest.merge_data)
        .chain(manifest.map_diagnostics.iter())
        .map(|a| &a.file_name)
        .chain(&manifest.selected_recipe_files)
        .chain(&manifest.selected_map_files)
    {
        let destination = staging.join(name);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(stage.join(name), destination).unwrap();
    }
    manifest.source_package_directory = fs::canonicalize(&target)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    fs::write(
        staging.join(MANIFEST_FILE_NAME),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let backups = temporary.path().join("backups");
    (
        Fixture {
            _temporary: temporary,
            staging,
            target,
            backups,
            staged_bytes: BTreeMap::new(),
        },
        manifest,
    )
}

fn install_request(fixture: &Fixture) -> InstallRequest {
    let mut request = InstallRequest::new(
        fixture.staging.clone(),
        fixture.target.clone(),
        fixture.backups.clone(),
    );
    request.game_running_check = game_stopped;
    request
}

fn assert_unpublished(fixture: &Fixture) {
    assert!(
        !sundial::package_authoring::mod_content::directory(fixture.target.parent().unwrap())
            .exists()
    );
    assert!(!fixture.backups.exists());
}

fn replace_bytes(path: &Path, bytes: &[u8], modified: SystemTime) {
    fs::write(path, bytes).unwrap();
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
}

#[allow(
    clippy::cognitive_complexity,
    reason = "One ordered native installation workflow checks persisted outcomes"
)]
fn exercise(schema: u64) {
    let (fixture, manifest) = fixture();
    let (account, _) = loader::account(&fixture, schema);
    let settings = fixture
        .target
        .parent()
        .unwrap()
        .join("bin/x64/Sunrise/settings.json");
    if schema == 8 {
        fs::write(&account, br#"{"version":8,"client":{"reveal_lore_books":false},"unknown":{"keep":true},"state":{}}"#).unwrap();
    } else {
        fs::write(
            &settings,
            br#"{"version":18,"client":{"reveal_lore_books":false},"unknown":{"keep":true}}"#,
        )
        .unwrap();
    }
    let original_account = read_authored_account_source(&account).unwrap();
    let started = Instant::now();
    let reviewed = preview_replacement(&fixture.target, &fixture.staging).unwrap();
    let review_seconds = started.elapsed().as_secs_f64();
    let mut request = install_request(&fixture);
    request.confirmed_replacement = Some(reviewed);
    let mut failures = Vec::new();
    let runtime_path = fixture
        .target
        .parent()
        .unwrap()
        .join("bin/x64/steam_api64.dll");
    let runtime_bytes = fs::read(&runtime_path).unwrap();
    let mut changed_runtime = runtime_bytes.clone();
    changed_runtime.extend_from_slice(b"review identity changed");
    fs::write(&runtime_path, changed_runtime).unwrap();
    sundial::package_authoring::validate_package_authoring_runtime(&fixture.target).unwrap();
    let mut expensive_work = false;
    let error = install_staged_packages_with_progress(&request, |event| {
        expensive_work |= event.phase == InstallPhase::ReviewingAccount
            || (event.phase == InstallPhase::Checking && event.completed > 0);
    })
    .unwrap_err();
    assert!(
        !expensive_work,
        "A changed runtime must fail before package processing"
    );
    assert!(error.message.to_ascii_lowercase().contains("runtime"));
    assert_unpublished(&fixture);
    failures.push(json!({"input":"supported runtime replaced","error":error.message,"failed_before_package_work":true}));
    fs::write(&runtime_path, runtime_bytes).unwrap();
    let mut inputs = vec![
        MANIFEST_FILE_NAME.to_owned(),
        manifest.selected_recipe_files[0].clone(),
        manifest.artifacts[0].file_name.clone(),
    ];
    inputs.extend(manifest.selected_map_files.first().cloned());
    inputs.extend(manifest.merge_data.first().map(|a| a.file_name.clone()));
    inputs.extend(manifest.map_diagnostics.iter().map(|a| a.file_name.clone()));
    for name in inputs {
        let path = fixture.staging.join(&name);
        let bytes = fs::read(&path).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let mut changed = bytes.clone();
        changed[0] ^= 1;
        replace_bytes(&path, &changed, modified);
        let error = install_staged_packages(&request).unwrap_err();
        assert_unpublished(&fixture);
        assert_eq!(
            read_authored_account_source(&account).unwrap(),
            original_account
        );
        failures.push(
            json!({"input":name,"same_length":true,"restored_mtime":true,"error":error.message}),
        );
        replace_bytes(&path, &bytes, modified);
    }
    let extra = fixture.staging.join("unexpected.pkg");
    fs::write(&extra, b"unexpected package").unwrap();
    let error = install_staged_packages(&request).unwrap_err();
    assert_unpublished(&fixture);
    failures.push(json!({"input":"added package","error":error.message}));
    fs::remove_file(extra).unwrap();
    let recipe = fixture.staging.join(&manifest.selected_recipe_files[0]);
    let outside = fixture._temporary.path().join("outside-recipe.json");
    fs::rename(&recipe, &outside).unwrap();
    let error = install_staged_packages(&request).unwrap_err();
    assert_unpublished(&fixture);
    failures.push(json!({"input":"missing recipe","error":error.message}));
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&outside, &recipe).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &recipe).unwrap();
    let error = install_staged_packages(&request).unwrap_err();
    assert_unpublished(&fixture);
    failures.push(json!({"input":"redirected recipe","error":error.message}));
    fs::remove_file(&recipe).unwrap();
    fs::rename(&outside, &recipe).unwrap();

    // A recipe edited after the initial content check must not reach backup or publication.
    let recipe_before = fs::read(&recipe).unwrap();
    let mut edited = false;
    let error = install_staged_packages_with_progress(&request, |event| {
        if !edited && event.phase == InstallPhase::ReviewingAccount {
            let mut bytes = recipe_before.clone();
            bytes[0] ^= 1;
            fs::write(&recipe, bytes).unwrap();
            edited = true;
        }
    })
    .unwrap_err();
    assert!(edited);
    assert_unpublished(&fixture);
    assert_eq!(
        read_authored_account_source(&account).unwrap(),
        original_account
    );
    failures.push(json!({"input":"recipe edited during account review","error":error.message}));
    fs::write(&recipe, recipe_before).unwrap();

    // Account freshness is independent of immutable staged content.
    if schema == 8 {
        let mut document: serde_json::Value =
            serde_json::from_slice(&fs::read(&account).unwrap()).unwrap();
        document["unknown"]["concurrent"] = true.into();
        fs::write(&account, serde_json::to_vec(&document).unwrap()).unwrap();
    } else {
        rusqlite::Connection::open(&account)
            .unwrap()
            .execute("UPDATE characters SET level=level+1", [])
            .unwrap();
    }
    let edited_account = read_authored_account_source(&account).unwrap();
    assert_ne!(edited_account, original_account);
    let error = install_staged_packages(&request).unwrap_err();
    assert_unpublished(&fixture);
    assert_eq!(
        read_authored_account_source(&account).unwrap(),
        edited_account
    );
    failures.push(json!({"input":"account changed after review","error":error.message}));

    request.confirmed_replacement =
        Some(preview_replacement(&fixture.target, &fixture.staging).unwrap());
    let started = Instant::now();
    let mut progress = Vec::new();
    let report = install_staged_packages_with_progress(&request, |event| {
        progress.push(json!({"seconds":started.elapsed().as_secs_f64(),"phase":format!("{:?}",event.phase),"label":event.current_artifact,"completed":event.completed,"total":event.total}));
    }).unwrap();
    let install_seconds = started.elapsed().as_secs_f64();
    let readback = read_packages(&fixture, &manifest.artifacts);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&settings).unwrap()).unwrap()["unknown"]
            ["keep"],
        true
    );
    if schema != 8 {
        let version: u32 = rusqlite::Connection::open(&account)
            .unwrap()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, if schema == 3 { 3 } else { 2 });
    }
    if schema == 8 {
        let before = read_authored_account_source(&account).unwrap();
        let direct = install_request(&fixture);
        install_staged_packages(&direct).unwrap();
        assert_eq!(read_authored_account_source(&account).unwrap(), before);
    }
    let kept = exercise_kept(&fixture, manifest.clone(), &account);
    let output = crate::test_support::artifact_dir("validation-reuse");
    fs::create_dir_all(&output).unwrap();
    fs::copy(
        fixture.staging.join(MANIFEST_FILE_NAME),
        output.join(format!("manifest-{schema}.json")),
    )
    .unwrap();
    fs::write(output.join(format!("schema-{schema}.json")), serde_json::to_vec_pretty(&json!({
        "schema":schema,"review_seconds":review_seconds,"install_seconds":install_seconds,
        "refusals":failures,"progress":progress,"readback":readback,"kept":kept,
        "installed_artifacts":report.artifacts.len(),"account_schema_preserved":true,
        "account_after":crate::artifact::digest_bytes(&read_authored_account_source(&account).unwrap()),
    })).unwrap()).unwrap();
    println!("validation reuse schema {schema}: passed");
}

fn snapshot(directory: &Path) -> BTreeMap<PathBuf, String> {
    fn walk(root: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, String>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    digest_file(&path).unwrap().sha256,
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    walk(directory, directory, &mut files);
    files
}

fn exercise_kept(
    fixture: &Fixture,
    mut manifest: ManifestDocument,
    account: &Path,
) -> serde_json::Value {
    use sundial::package_authoring::mod_content;
    let install = fixture.target.parent().unwrap();
    let changed_inputs = keep_members(fixture, &mut manifest);
    let mut request = install_request(fixture);
    request.confirmed_replacement =
        Some(preview_replacement(&fixture.target, &fixture.staging).unwrap());
    let account_before = read_authored_account_source(account).unwrap();
    let backups_before = snapshot(&fixture.backups);
    let mut refusals = Vec::new();
    for path in changed_inputs {
        let original = fs::read(&path).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let changed = changed_member(&path, &original);
        replace_bytes(&path, &changed, modified);
        let before = snapshot(&mod_content::runtime_directory(install).join("mods"));
        let error = install_staged_packages(&request).unwrap_err();
        assert_eq!(
            snapshot(&mod_content::runtime_directory(install).join("mods")),
            before
        );
        assert_eq!(snapshot(&fixture.backups), backups_before);
        assert_eq!(
            read_authored_account_source(account).unwrap(),
            account_before
        );
        assert!(
            !mod_content::runtime_directory(install)
                .join(mod_content::JOURNAL)
                .exists()
        );
        refusals.push(json!({"path":path.strip_prefix(install).unwrap(),"error":error.message}));
        replace_bytes(&path, &original, modified);
    }
    // A valid outside edit after consent must not become the transaction's new baseline.
    let main_manifest = mod_content::directory(install).join(mod_content::MANIFEST);
    let original = fs::read(&main_manifest).unwrap();
    let modified = fs::metadata(&main_manifest).unwrap().modified().unwrap();
    let mut document: serde_json::Value = serde_json::from_slice(&original).unwrap();
    document["name"] = "Concurrently Edited Main Mod".into();
    let changed = serde_json::to_vec(&document).unwrap();
    let mods = mod_content::runtime_directory(install).join("mods");
    let mut after_edit = None;
    let error = install_staged_packages_with_progress(&request, |event| {
        if event.current_artifact.as_deref() == Some("Account Review Complete") {
            replace_bytes(&main_manifest, &changed, modified);
            after_edit = Some(snapshot(&mods));
        }
    })
    .unwrap_err();
    assert_eq!(
        snapshot(&mods),
        after_edit.expect("account consent reached")
    );
    assert_eq!(snapshot(&fixture.backups), backups_before);
    assert_eq!(
        read_authored_account_source(account).unwrap(),
        account_before
    );
    assert!(
        !mod_content::runtime_directory(install)
            .join(mod_content::JOURNAL)
            .exists()
    );
    refusals.push(json!({
        "path":main_manifest.strip_prefix(install).unwrap(),
        "boundary":"after account consent", "error":error.message,
        "preserved_sha256":digest_file(&main_manifest).unwrap().sha256
    }));
    replace_bytes(&main_manifest, &original, modified);
    let kept_before = manifest
        .mods
        .iter()
        .filter(|m| m.kept)
        .map(|m| {
            (
                m.manifest.id.clone(),
                snapshot(&mod_content::member_directory(install, &m.manifest.id)),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let report = install_staged_packages(&request).unwrap();
    assert!(report.backup_prune_warning.is_none());
    for member in manifest.mods.iter().filter(|m| m.kept) {
        let directory = mod_content::member_directory(install, &member.manifest.id);
        assert_eq!(snapshot(&directory), kept_before[&member.manifest.id]);
    }
    let readback = read_packages(fixture, &manifest.artifacts);
    assert_eq!(
        read_authored_account_source(account).unwrap(),
        account_before
    );
    json!({"refusals":refusals,"readback":readback,"account_preserved":true})
}

fn changed_member(path: &Path, original: &[u8]) -> Vec<u8> {
    use sundial::package_authoring::mod_content;
    if path.ends_with(mod_content::RECEIPT) {
        let mut document: serde_json::Value = serde_json::from_slice(original).unwrap();
        document["content"]["state"]["concurrent"] = true.into();
        serde_json::to_vec(&document).unwrap()
    } else if path.ends_with(mod_content::MANIFEST) {
        let mut document: serde_json::Value = serde_json::from_slice(original).unwrap();
        document["name"] = "Concurrently Edited Importer".into();
        serde_json::to_vec(&document).unwrap()
    } else {
        let mut bytes = original.to_vec();
        bytes[0] ^= 1;
        bytes
    }
}

fn keep_members(fixture: &Fixture, manifest: &mut ManifestDocument) -> Vec<PathBuf> {
    use sundial::package_authoring::mod_content;
    let install = fixture.target.parent().unwrap();
    let mut changed_inputs = Vec::new();
    for member in manifest
        .mods
        .iter_mut()
        .filter(|m| m.manifest.id != mod_content::ID)
    {
        member.kept = true;
        let directory = mod_content::member_directory(install, &member.manifest.id);
        if changed_inputs.is_empty() {
            changed_inputs.push(directory.join(mod_content::MANIFEST));
            changed_inputs.push(directory.join(mod_content::RECEIPT));
            changed_inputs.push(
                directory.join(
                    &member
                        .receipt
                        .artifacts
                        .iter()
                        .find(|a| a.path.ends_with(".pkg"))
                        .unwrap()
                        .path,
                ),
            );
            if let Some(sidecar) = member
                .receipt
                .artifacts
                .iter()
                .find(|a| a.path.ends_with(".merge"))
            {
                changed_inputs.push(directory.join(&sidecar.path));
            }
        }
        for artifact in &member.receipt.artifacts {
            let name = Path::new(&artifact.path).file_name().unwrap();
            fs::remove_file(fixture.staging.join(name)).unwrap();
        }
    }
    assert!(!changed_inputs.is_empty());
    fs::write(
        fixture.staging.join(MANIFEST_FILE_NAME),
        serde_json::to_vec(manifest).unwrap(),
    )
    .unwrap();
    changed_inputs
}

fn read_packages(fixture: &Fixture, artifacts: &[ArtifactMetadata]) -> Vec<serde_json::Value> {
    let files =
        sundial::package_authoring::mod_content::files(fixture.target.parent().unwrap()).unwrap();
    let manager =
        sundial::package_authoring::open_shadowkeep_package_manager(&fixture.target).unwrap();
    let mut readback = Vec::new();
    for artifact in artifacts {
        let path = files
            .iter()
            .find(|path| path.ends_with(&artifact.file_name))
            .unwrap();
        let digest = digest_file(path).unwrap();
        assert_eq!(digest.sha256, artifact.sha256);
        let profile = authored_package_for_file_name(&artifact.file_name).unwrap();
        let mut entries = Vec::new();
        if !profile.stock_overlay {
            let package = PackageD2PreBL::open(path.to_str().unwrap()).unwrap();
            for index in 0..package.entries().len() {
                let payload = package.read_entry(index).unwrap();
                assert_eq!(
                    manager
                        .read_tag(tiger_pkg::TagHash::new(
                            profile.package_id,
                            index.try_into().unwrap()
                        ))
                        .unwrap(),
                    payload
                );
                entries.push(crate::artifact::digest_bytes(&payload).sha256);
            }
            assert!(!entries.is_empty());
        }
        readback.push(json!({"file":artifact.file_name,"sha256":digest.sha256,"entries":entries}));
    }
    readback
}

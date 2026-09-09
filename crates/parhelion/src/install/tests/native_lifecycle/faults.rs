//! Exercise every package commit/removal boundary against a real generated batch.
use super::*;

fn assert_rolled_back(error: InstallError) {
    assert!(
        error
            .rollback
            .as_ref()
            .is_some_and(|rollback| rollback.succeeded()),
        "{error}"
    );
}

fn verify_boundaries(version: u64) {
    let fixture = NativeFixture::copy_from_environment(version);
    let count = fixture.manifest.artifacts.len();
    for boundary in 1..=count {
        assert_rolled_back(
            install_staged_packages_inner(
                &fixture.request,
                Some(boundary),
                DEFAULT_CACHE_INVALIDATION_OPS,
            )
            .unwrap_err(),
        );
        assert!(
            preview_uninstall(&fixture.request.target_packages_directory)
                .unwrap()
                .artifacts()
                .is_empty()
        );
        fixture.assert_stock_and_account_unchanged();
        eprintln!(
            "FAILURE_BOUNDARY_PASS schema={version} operation=first-install boundary={boundary}/{count}"
        );
    }
    install_staged_packages(&fixture.request).unwrap();
    fixture.assert_authored_set();
    for boundary in 1..=count {
        assert_rolled_back(
            install_staged_packages_inner(
                &fixture.request,
                Some(boundary),
                DEFAULT_CACHE_INVALIDATION_OPS,
            )
            .unwrap_err(),
        );
        fixture.assert_authored_set();
        fixture.assert_stock_and_account_unchanged();
        eprintln!(
            "FAILURE_BOUNDARY_PASS schema={version} operation=reinstall boundary={boundary}/{count}"
        );
    }
    for boundary in 1..=count {
        let plan = preview_uninstall(&fixture.request.target_packages_directory).unwrap();
        let error = uninstall::uninstall_inner(
            &plan,
            &fixture.request.backup_root,
            game_stopped,
            Some(boundary),
            DEFAULT_CACHE_INVALIDATION_OPS,
        )
        .unwrap_err();
        assert!(
            error
                .message
                .contains("the original custom package set was restored"),
            "{error}"
        );
        assert!(
            error
                .backup_directory
                .as_ref()
                .is_some_and(|path| path.is_dir())
        );
        fixture.assert_authored_set();
        fixture.assert_stock_and_account_unchanged();
        eprintln!(
            "FAILURE_BOUNDARY_PASS schema={version} operation=uninstall boundary={boundary}/{count}"
        );
    }
    let plan = preview_uninstall(&fixture.request.target_packages_directory).unwrap();
    uninstall_custom_packages(&plan, &fixture.request.backup_root, game_stopped).unwrap();
    fixture.assert_stock_and_account_unchanged();
    assert!(
        preview_uninstall(&fixture.request.target_packages_directory)
            .unwrap()
            .artifacts()
            .is_empty()
    );
}

#[test]
#[ignore = "copies native files into disposable directories, requires PARHELION_LIFECYCLE_SOURCE_PACKAGES and PARHELION_TEST_STAGED_RUN"]
fn native_every_commit_boundary_rolls_back() {
    if let Some(version) = std::env::var_os(CHILD_SCHEMA) {
        let version = version.to_str().unwrap().parse().unwrap();
        assert!(matches!(version, 6 | 8 | 16));
        verify_boundaries(version);
        return;
    }
    let root = TempDir::new().unwrap();
    for version in [6, 8, 16] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "install::tests::native_lifecycle::faults::native_every_commit_boundary_rolls_back",
                "--exact",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_SCHEMA, version.to_string())
            .env(CHILD_ROOT, root.path());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "schema {version}:\n{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        eprintln!("{}", String::from_utf8_lossy(&result.stderr));
    }
}

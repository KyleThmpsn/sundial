use super::*;
use crate::test_support::TestDirectory;

#[test]
fn empty_directories_consume_the_scan_budget() {
    let directory = TestDirectory::new("diagnostic-directory-budget");
    for index in 0..8 {
        fs::create_dir(directory.0.join(index.to_string())).unwrap();
    }
    let scan = scan_runtime_folder_with_limit(&directory.0, 3);
    assert!(scan.files.is_empty());
    assert!(scan.errors.is_empty());
    assert_eq!(scan.entries_scanned, 3);
    assert!(scan.truncated);
}

#[test]
fn exact_budget_does_not_claim_the_scan_was_truncated() {
    let directory = TestDirectory::new("diagnostic-exact-budget");
    fs::create_dir(directory.0.join("empty")).unwrap();
    fs::write(directory.0.join("runtime.bin"), b"private content").unwrap();
    let scan = scan_runtime_folder_with_limit(&directory.0, 2);
    assert_eq!(scan.files.len(), 1);
    assert_eq!(scan.entries_scanned, 2);
    assert!(!scan.truncated);
}

#[test]
fn nested_directories_share_one_budget() {
    let directory = TestDirectory::new("diagnostic-nested-budget");
    fs::create_dir_all(directory.0.join("a/b/c/d")).unwrap();
    fs::write(directory.0.join("a/b/c/d/runtime.bin"), b"private content").unwrap();
    let limited = scan_runtime_folder_with_limit(&directory.0, 3);
    assert!(limited.files.is_empty());
    assert_eq!(limited.entries_scanned, 3);
    assert!(limited.truncated);
    let complete = scan_runtime_folder_with_limit(&directory.0, 5);
    assert_eq!(complete.files.len(), 1);
    assert_eq!(complete.entries_scanned, 5);
    assert!(!complete.truncated);
}

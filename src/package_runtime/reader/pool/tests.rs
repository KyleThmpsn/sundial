use super::*;
use std::{
    fs::File,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
#[derive(Default)]
struct Counts {
    active: AtomicUsize,
    peak: AtomicUsize,
    opens: AtomicUsize,
}
struct Handle {
    _file: File,
    counts: Arc<Counts>,
}

#[test]
fn opening_one_package_does_not_block_an_existing_unrelated_reader() {
    let pool = Arc::new(Pool::new(4));
    drop(pool.acquire((1, 1), 1, || Ok(7u8)).unwrap());
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let opening = pool.clone();
    let worker = std::thread::spawn(move || {
        drop(
            opening
                .acquire((2, 2), 1, || {
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(9)
                })
                .unwrap(),
        );
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let (read_tx, read_rx) = std::sync::mpsc::channel();
    let independent = pool.clone();
    let reader = std::thread::spawn(move || {
        let lease = independent
            .acquire((1, 1), 1, || panic!("The existing reader was evicted"))
            .unwrap();
        read_tx.send(*lease.value()).unwrap();
    });
    let result = read_rx.recv_timeout(Duration::from_secs(2));
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    reader.join().unwrap();
    assert_eq!(result.unwrap(), 7);
}
impl Handle {
    fn open(path: &std::path::Path, counts: &Arc<Counts>) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| e.to_string())?;
        let active = counts.active.fetch_add(1, Ordering::SeqCst) + 1;
        counts.peak.fetch_max(active, Ordering::SeqCst);
        counts.opens.fetch_add(1, Ordering::SeqCst);
        Ok(Self {
            _file: file,
            counts: Arc::clone(counts),
        })
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.counts.active.fetch_sub(1, Ordering::SeqCst);
    }
}
#[test]
fn large_scans_reuse_readers_and_close_evicted_files() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let counts = Arc::new(Counts::default());
    let pool = Pool::new(8);
    for tag in 0..2000 {
        drop(
            pool.acquire((1, tag), 1, || Handle::open(file.path(), &counts))
                .unwrap(),
        );
        drop(
            pool.acquire((1, tag), 1, || Handle::open(file.path(), &counts))
                .unwrap(),
        );
    }
    assert_eq!(counts.opens.load(Ordering::SeqCst), 2000);
    assert!(counts.peak.load(Ordering::SeqCst) <= 8);
    pool.remove_owner(1);
    assert_eq!(counts.active.load(Ordering::SeqCst), 0);
}
#[test]
fn concurrent_readers_never_exceed_the_handle_budget() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let counts = Arc::new(Counts::default());
    let pool = Pool::new(4);
    std::thread::scope(|scope| {
        for owner in 0..16 {
            let pool = &pool;
            let counts = &counts;
            let path = file.path();
            scope.spawn(move || {
                for tag in 0..30 {
                    let _lease = pool
                        .acquire((owner, tag), 1, || Handle::open(path, counts))
                        .unwrap();
                    std::thread::sleep(Duration::from_micros(10));
                }
            });
        }
    });
    assert!(counts.peak.load(Ordering::SeqCst) <= 4);
    drop(pool);
    assert_eq!(counts.active.load(Ordering::SeqCst), 0);
}
#[test]
fn patch_weights_and_oversized_families_evict_before_opening() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let counts = Arc::new(Counts::default());
    let pool = Pool::new(4);
    let open = |weight| {
        assert_eq!(counts.active.load(Ordering::SeqCst), 0);
        (0..weight)
            .map(|_| Handle::open(file.path(), &counts))
            .collect::<Result<Vec<_>, _>>()
    };
    drop(pool.acquire((1, 1), 3, || open(3)).unwrap());
    drop(pool.acquire((1, 2), 3, || open(3)).unwrap());
    assert_eq!(counts.active.load(Ordering::SeqCst), 3);
    drop(pool.acquire((1, 3), 8, || open(8)).unwrap());
    assert_eq!(counts.active.load(Ordering::SeqCst), 8);
    drop(pool.acquire((1, 4), 1, || open(1)).unwrap());
    assert_eq!(counts.active.load(Ordering::SeqCst), 1);
    drop(pool);
    assert_eq!(counts.active.load(Ordering::SeqCst), 0);
}
#[test]
fn failed_opens_can_retry_and_new_owners_do_not_reuse_stale_data() {
    let pool = Pool::new(2);
    assert!(
        pool.acquire((1, 1), 1, || Err::<u8, _>("temporary failure".into()))
            .is_err()
    );
    assert_eq!(*pool.acquire((1, 1), 1, || Ok(7)).unwrap().value(), 7);
    assert_eq!(*pool.acquire((2, 1), 1, || Ok(9)).unwrap().value(), 9);
    pool.remove_owner(1);
    assert_eq!(*pool.acquire((1, 1), 1, || Ok(11)).unwrap().value(), 11);
}

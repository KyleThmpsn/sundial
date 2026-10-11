use super::*;

#[test]
fn payload_cache_reuses_reads_and_bounds_retention() {
    let mut cache = Payloads::new(12, 3);
    cache.insert(1, Arc::new(Payload(vec![7; 8])));
    cache.insert(2, Arc::new(Payload(vec![2; 4])));
    assert_eq!(cache.get(1).unwrap().0, vec![7; 8]);
    cache.insert(3, Arc::new(Payload(vec![3; 4])));
    assert!(
        cache.get(2).is_none(),
        "Cold payload survived at the expense of the reused one"
    );
    assert_eq!(cache.get(1).unwrap().0, vec![7; 8]);
    cache.insert(4, Arc::new(Payload(vec![4; 13])));
    assert!(cache.get(4).is_none(), "Oversized read remained retained");
    cache.clear();
    for tag in 10..30 {
        cache.insert(tag, Arc::new(Payload(vec![tag as u8])));
    }
    assert!(cache.get(10).is_none());
    assert_eq!(cache.get(29).unwrap().0, [29]);
    if let Some(output) = std::env::var_os("SUNDIAL_CACHE_VERIFICATION_DIRECTORY") {
        let output = std::path::PathBuf::from(output);
        std::fs::create_dir_all(&output).unwrap();
        crate::cache::write_json(&output.join("payloads.json"), &serde_json::json!({"last_read": cache.get(29).unwrap().0, "oversized_evicted": true, "cold_evicted": true})).unwrap();
    }
}

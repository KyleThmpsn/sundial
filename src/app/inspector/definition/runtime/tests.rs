use super::*;

fn pending(state: &mut RuntimeInspectionState, target: RuntimeTarget) -> mpsc::Sender<LoadResult> {
    let (sender, receiver) = mpsc::channel();
    state.pending = Some(PendingLoad {
        scope: state.scope.clone().unwrap(),
        generation: state.generation,
        target,
        receiver,
    });
    sender
}

#[test]
fn preparing_an_item_does_not_read_packages() {
    let mut state = RuntimeInspectionState::default();
    state.prepare(Path::new("missing-install"), 1, Arc::default());
    state.poll(&egui::Context::default());
    assert!(state.pending.is_none());
    assert!(state.cache.is_empty());
}

#[test]
fn navigation_and_reopening_discard_old_results_for_the_same_item() {
    for close in [false, true] {
        let mut state = RuntimeInspectionState::default();
        let install = Path::new("install");
        state.prepare(install, 1, Arc::default());
        let sender = pending(&mut state, RuntimeTarget::Perk(8));
        if close {
            state.clear();
        } else {
            state.prepare(install, 2, Arc::default());
        }
        state.prepare(install, 1, Arc::default());
        assert!(state.pending.is_some(), "one worker remains in flight");
        sender.send(Err("stale result".into())).unwrap();
        state.poll(&egui::Context::default());
        assert!(state.pending.is_none());
        assert!(state.cache.is_empty());
    }
}

#[test]
fn changing_installs_clears_cached_values_and_filters() {
    let mut state = RuntimeInspectionState::default();
    state.prepare(Path::new("install-a"), 1, Arc::default());
    state.remember(RuntimeTarget::Weapon(1), Err("a".into()));
    state.view.show_opaque = true;
    state.view.query = "old filter".into();
    state.prepare(Path::new("install-b"), 1, Arc::default());
    assert!(state.cache.is_empty());
    assert!(state.view.query.is_empty());
    assert!(!state.view.show_opaque);
}

#[test]
fn disconnected_reader_returns_an_actionable_error() {
    let mut state = RuntimeInspectionState::default();
    state.prepare(Path::new("install"), 1, Arc::default());
    drop(pending(&mut state, RuntimeTarget::Weapon(1)));
    state.poll(&egui::Context::default());
    assert!(state.pending.is_none());
    assert!(
        state.cache[&RuntimeTarget::Weapon(1)]
            .as_ref()
            .as_ref()
            .unwrap_err()
            .contains("retry")
    );
}

#[test]
fn cache_is_bounded_and_target_kinds_cannot_collide() {
    let mut state = RuntimeInspectionState::default();
    for index in 0..8 {
        state.remember(RuntimeTarget::Perk(index), Err(index.to_string()));
    }
    assert_eq!(state.cache.len(), CACHE_LIMIT);
    assert!(!state.cache.contains_key(&RuntimeTarget::Perk(0)));
    state.remember(RuntimeTarget::Weapon(7), Err("weapon".into()));
    assert!(state.cache.contains_key(&RuntimeTarget::Perk(7)));
    assert!(state.cache.contains_key(&RuntimeTarget::Weapon(7)));
    assert_eq!(state.cache.len(), CACHE_LIMIT);
}

#[test]
#[ignore = "requires SUNDIAL_TEST_INSTALL pointing to a Shadowkeep install; read-only"]
fn installed_weapon_and_perk_runtime_inspection() {
    let install = std::env::var_os("SUNDIAL_TEST_INSTALL").expect("set SUNDIAL_TEST_INSTALL");
    let install = Path::new(&install);
    let weapon = loader::load(install, RuntimeTarget::Weapon(285)).unwrap();
    let LoadedDetails::Weapon(graph) = weapon else {
        panic!("expected weapon graph");
    };
    assert!(graph.field_count() > 0);
    assert!(
        graph
            .fields()
            .any(|field| field.path_label == "Initial Speed Scale")
    );
    // Stock Micro-Missile, also used by Parhelion's package-chain regression test.
    let LoadedDetails::Perk(perk) = loader::load(install, RuntimeTarget::Perk(1178)).unwrap()
    else {
        panic!("expected perk details");
    };
    assert_eq!(perk.row.index, 1178);
    let action = perk.action.unwrap();
    assert!(!action.graphs.is_empty());
    assert!(action.graphs.iter().all(|graph| graph.decoded.is_ok()));
}

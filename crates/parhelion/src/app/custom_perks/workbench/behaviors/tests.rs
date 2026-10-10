//! Checks of the rows the behavior pickers offer and how they group, filter and order them.
use super::*;
use sundial::package_authoring::sandbox_perk::action::native::NodeKind as NativeNodeKind;

#[test]
fn aliases_share_one_entry_without_losing_configurations_or_weapon_restrictions() {
    let family = Family::Condition(trigger_family(Trigger::WeaponKill).unwrap());
    let rows = [
        Row {
            family: family.clone(),
            enabled: true,
            reason: "",
            title: "On Weapon Kill".into(),
            detail: String::new(),
            search: "on weapon kill".into(),
            uses: 0,
            choice: Choice::Trigger(Trigger::WeaponKill),
        },
        Row {
            family,
            enabled: true,
            reason: "",
            title: "A Kill from This Weapon".into(),
            detail: String::new(),
            search: "a kill from this weapon with extra restrictions".into(),
            uses: 0,
            choice: Choice::Condition(42),
        },
        Row {
            family: Family::Condition(trigger_family(Trigger::AnyKill).unwrap()),
            enabled: true,
            reason: "",
            title: "On Any Credited Kill".into(),
            detail: String::new(),
            search: "on any credited kill".into(),
            uses: 0,
            choice: Choice::Trigger(Trigger::AnyKill),
        },
    ];
    let groups = group_rows(
        rows.iter(),
        "",
        StockUse::Any,
        Detail::Advanced,
        Category::Any,
        Order::Suggested,
        Purpose::Trigger.leads(),
    );
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].rows.len(), 2);
    let filtered = group_rows(
        rows.iter(),
        "extra restrictions",
        StockUse::Any,
        Detail::Advanced,
        Category::Any,
        Order::Suggested,
        Purpose::Trigger.leads(),
    );
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].rows[0].key(), rows[0].key());
    assert_eq!(filtered[0].rows[1].key(), rows[1].key());
    assert_eq!(
        action_family(&Action::add_rounds(1)),
        Family::Effect(14, None)
    );
    assert_eq!(
        action_family(&Action::add_fraction(0.5)),
        Family::Effect(15, None)
    );
    assert_eq!(
        action_family(&Action::generate_orb(
            sundial::package_authoring::sandbox_perk::program::Position::Owner
        )),
        Family::Effect(5, None)
    );
}

fn row(title: &str, kind: u8, choice: Choice) -> Row {
    Row {
        family: effect_family(kind, &[]),
        enabled: true,
        reason: "",
        title: title.into(),
        detail: String::new(),
        search: title.to_lowercase(),
        uses: 0,
        choice,
    }
}

#[test]
fn the_stock_use_filter_splits_on_whether_the_game_configures_the_kind() {
    let rows = [
        row("Spawn an Effect", 3, Choice::Action(Action::add_rounds(1))),
        row("Zero Native Kind", 44, Choice::Kind(44)),
        row("Alpha Stock Effect", 10, Choice::Effect(7)),
    ];
    let titles = |stock, order| {
        group_rows(
            rows.iter(),
            "",
            stock,
            Detail::Advanced,
            Category::Any,
            order,
            Purpose::Action.leads(),
        )
        .iter()
        .map(|group| group.title.to_owned())
        .collect::<Vec<_>>()
    };
    // Suggested puts the common actions, here the stat change and the spawn, ahead of the
    // rest.
    let suggested = titles(StockUse::Any, Order::Suggested);
    assert_eq!(suggested.len(), 3);
    assert_eq!(suggested[2], "Zero Native Kind");
    // Only the bare kind list carries node kinds no stock perk configures.
    assert_eq!(
        titles(StockUse::Unused, Order::Suggested),
        ["Zero Native Kind"]
    );
    let mut used = titles(StockUse::Used, Order::Suggested);
    used.sort();
    assert_eq!(used, ["Alpha Stock Effect", "Spawn an Effect"]);
}

#[test]
fn a_group_holding_both_a_guided_row_and_stock_configurations_is_never_split() {
    // One node kind can be offered as a named action and also decoded from the stock
    // perks that configure it. Both land in one group, so the filter keeps or drops the
    // group as a whole rather than removing rows from inside it.
    let rows = [
        row("Spawn an Effect", 3, Choice::Action(Action::add_rounds(1))),
        row("Spawn an Effect", 3, Choice::Effect(7)),
        row("Spawn an Effect", 3, Choice::Effect(9)),
    ];
    let groups = group_rows(
        rows.iter(),
        "",
        StockUse::Used,
        Detail::Advanced,
        Category::Any,
        Order::Suggested,
        Purpose::Action.leads(),
    );
    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups[0].rows.len(),
        3,
        "every configuration stays available"
    );
    assert!(
        group_rows(
            rows.iter(),
            "",
            StockUse::Unused,
            Detail::Advanced,
            Category::Any,
            Order::Suggested,
            Purpose::Action.leads()
        )
        .is_empty()
    );
}

#[test]
fn the_standard_list_hides_only_the_rows_named_by_their_engine_operation() {
    let rows = [
        // Kind 3 is a guided spawn, so both its rows read in game terms.
        row(
            "Spawn an Object or Effect",
            3,
            Choice::Action(Action::add_rounds(1)),
        ),
        row("Spawn an Object or Effect", 3, Choice::Effect(1)),
        // Kind 27's traced evidence leaves its gameplay feature unresolved, so it has
        // no plain title and reads only as the operation it was traced to.
        row("Host Activation Counter", 27, Choice::Effect(2)),
    ];
    let titles = |detail| {
        group_rows(
            rows.iter(),
            "",
            StockUse::Any,
            detail,
            Category::Any,
            Order::Suggested,
            Purpose::Action.leads(),
        )
        .iter()
        .map(|group| group.title.to_owned())
        .collect::<Vec<_>>()
    };
    assert_eq!(titles(Detail::Standard), ["Spawn an Object or Effect"]);
    assert_eq!(
        titles(Detail::Advanced),
        ["Spawn an Object or Effect", "Host Activation Counter"]
    );
    // Hiding is per group, so a plain group keeps every configuration it holds.
    let plain = group_rows(
        rows.iter(),
        "",
        StockUse::Any,
        Detail::Standard,
        Category::Any,
        Order::Suggested,
        Purpose::Action.leads(),
    );
    assert_eq!(plain[0].rows.len(), 2);
}

#[test]
fn the_category_filter_narrows_to_one_concern_and_suggested_leads_with_the_common_choices() {
    let condition = |title: &str, kind: u8, uses: usize| Row {
        family: Family::Condition(ConditionFamily::Kind(kind)),
        enabled: true,
        reason: "",
        title: title.into(),
        detail: String::new(),
        search: title.to_lowercase(),
        uses,
        choice: Choice::Kind(kind),
    };
    let mut rows = comparison_rows();
    rows.push(condition("On Reloading", 19, 0));
    rows.push(condition("On a Kill", 2, 3));
    rows.push(condition("On Picking Up Ammo", 6, 1));
    let titles = |category, order| {
        group_rows(
            rows.iter(),
            "",
            StockUse::Any,
            Detail::Standard,
            category,
            order,
            Purpose::Trigger.leads(),
        )
        .iter()
        .map(|group| group.title.to_owned())
        .collect::<Vec<_>>()
    };
    // Every compared variable is a state, and the filter keeps only those.
    let states = titles(Category::States, Order::Suggested);
    assert!(
        !states.is_empty(),
        "state comparisons are absent from the picker"
    );
    assert_eq!(titles(Category::Ammo, Order::Name), ["On Picking Up Ammo"]);
    assert_eq!(titles(Category::KillsAndDamage, Order::Name), ["On a Kill"]);
    // Suggested leads with the common triggers, here reload and ammo pickup, then the rest
    // by how often the stock perks use them: the kill, then the unused compared states in
    // the order the workbench lists them.
    let suggested = titles(Category::Any, Order::Suggested);
    let mut common = suggested[..2].to_vec();
    common.sort();
    assert_eq!(common, ["On Picking Up Ammo", "On Reloading"]);
    assert_eq!(suggested[2], "On a Kill");
    assert_eq!(suggested[3..], states[..]);
    // Every condition and action kind with a plain title lands in a named category.
    for kind in 0..=44u8 {
        if program::plain_condition_title(kind).is_some() {
            let family = Family::Condition(ConditionFamily::Kind(kind));
            assert_ne!(Category::of(&family), Category::Other, "condition {kind}");
            assert!(Category::choices(Purpose::Condition).contains(&Category::of(&family)));
        }
    }
    for kind in 0..=54u8 {
        if program::plain_action_title(kind).is_some() {
            let family = effect_family(kind, &[]);
            assert_ne!(Category::of(&family), Category::Other, "action {kind}");
            assert!(Category::choices(Purpose::Action).contains(&Category::of(&family)));
        }
    }
}

/// A configuration named only by a state hash waits behind Show All. One the stock perks
/// never name is offered under its kind's plain title, except a kill, and an empty state
/// check says how it differs from kind 0's Always.
#[test]
fn default_stock_configurations_read_in_plain_terms() {
    use sundial::investment::discovery::conditions;
    use sundial::package_authoring::sandbox_perk::program::native_draft;
    let state = |key: u32| {
        let mut node = NativeNode::condition(20).unwrap();
        node.bytes[0x38] = 0;
        node.bytes[0x81] = 0;
        node.bytes[0xD4..0xD8].copy_from_slice(&key.to_le_bytes());
        let program = Program {
            trigger: Trigger::Native,
            native_trigger: Some(node),
            actions: vec![Action::add_rounds(1)],
            ..Program::default()
        };
        let payload = native_draft(&program).unwrap().graph.emit().unwrap();
        conditions::from_payload(&payload)
            .unwrap()
            .into_iter()
            .find(|condition| condition.kind == 20)
            .unwrap()
    };
    let hashed = state(0x1234_5678);
    assert!(hashed.name.is_some(), "{}", hashed.description);
    assert!(!offered_by_default(&hashed), "{}", hashed.description);
    let empty = state(0x811C_9DC5);
    assert!(offered_by_default(&empty));
    assert_eq!(empty.name.as_deref(), Some("Always"));
    let unnamed = |kind| conditions::Condition {
        family: ConditionFamily::Kind(kind),
        name: None,
        description: String::new(),
        source: String::new(),
        kind,
        requirements: Vec::new(),
        details: Vec::new(),
        bytes: Vec::new(),
    };
    assert!(offered_by_default(&unnamed(6)));
    assert!(offered_by_default(&unnamed(12)));
    assert!(!offered_by_default(&unnamed(2)));
}

/// A stock "While Super Active" covers the super_active comparison only. Super Recently
/// Active compares a different variable, and a numeric state such as nearby enemies reads
/// as a requirement, not "While", so its comparison stays.
#[test]
fn a_stock_state_hides_only_the_comparison_of_its_own_variable() {
    let states = [
        "While Super Active",
        "Meets the Nearby Enemy Count requirement",
    ]
    .into_iter()
    .collect::<std::collections::BTreeSet<_>>();
    let hidden = comparison_rows()
        .into_iter()
        .filter(|row| stock_state_covers(row, &states))
        .map(|row| row.title)
        .collect::<Vec<_>>();
    assert_eq!(hidden, ["Super Active = 1"]);
}

#[test]
fn every_compared_variable_is_a_standard_condition_that_validates() {
    use sundial::package_authoring::sandbox_perk::action::native::{self, predicate};
    let rows = comparison_rows();
    assert!(!rows.is_empty(), "no compiled native state comparisons");
    for row in &rows {
        assert!(row.plain_language(), "{}", row.title);
        assert!(row.configured_by_stock_perks(), "{}", row.title);
        let Choice::Native(node) = &row.choice else {
            panic!("{} is not a composed condition", row.title);
        };
        assert_eq!(node.kind, 20);
        let graph = native::Graph::read(&node.bytes, 0, 0x80803DCE).unwrap();
        graph.validate_node(NativeNodeKind::Condition(20)).unwrap();
        let described = predicate::describe(&graph).unwrap();
        assert!(
            described.starts_with(&row.title),
            "{described} vs {}",
            row.title
        );
        assert!(!row.detail.is_empty());
    }
    // The rows read as Standard, so the default picker view offers them.
    let groups = group_rows(
        rows.iter(),
        "",
        StockUse::Any,
        Detail::Standard,
        Category::Any,
        Order::Suggested,
        Purpose::Trigger.leads(),
    );
    assert_eq!(groups.len(), rows.len());
    // The composed nodes are distinct per variable, not one template repeated.
    let distinct = rows
        .iter()
        .filter_map(|row| match &row.choice {
            Choice::Native(node) => Some(node.bytes.clone()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(distinct.len(), rows.len());
}

#[test]
fn every_guided_action_reads_in_game_terms_and_carries_a_plain_sentence() {
    let program = Program::default();
    let actions = program::common_actions(&program, &Default::default());
    assert!(!actions.is_empty());
    for (title, summary, action) in &actions {
        assert!(
            !title.trim().is_empty() && !summary.trim().is_empty(),
            "{title}"
        );
        // A guided entry never shows the engine's traced operation name.
        if let Action::Native { node } = action {
            assert_eq!(
                program::plain_action_title(node.kind),
                Some(*title),
                "kind {} is offered as guided without a plain title",
                node.kind
            );
        }
    }
    // Every promoted kind must actually produce an editable node. A kind whose record
    // cannot be built would be silently dropped, leaving a promise with nothing behind it.
    let missing = program::PROMOTED_NATIVE_ACTIONS
        .iter()
        .filter(|kind| {
            let title = program::plain_action_title(**kind);
            title.is_none() || !actions.iter().any(|(name, _, _)| Some(*name) == title)
        })
        .map(|kind| kind.to_string())
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "promoted kinds with no guided action: {missing:?}"
    );
}

/// A promoted condition is offered to every author, not only one who found Show All, so it
/// has to read like a guided row rather than an engine kind with a number in front of it.
/// Both halves matter: the title is what a reader scans for, and the summary is what tells
/// them it is the one they want.
#[test]
fn every_promoted_condition_reads_as_a_plain_row() {
    for kind in program::PROMOTED_NATIVE_CONDITIONS {
        let node = nodes::CONDITIONS
            .iter()
            .find(|node| node.kind == kind)
            .unwrap_or_else(|| panic!("condition kind {kind} is not an engine kind"));
        assert_eq!(
            node.support,
            nodes::Support::Authorable,
            "condition kind {kind} is promoted but cannot be authored"
        );
        let title = program::plain_condition_title(kind);
        assert!(
            title.is_some(),
            "condition kind {kind} is promoted without a plain title"
        );
        assert!(
            program::plain_condition_summary(kind).is_some(),
            "condition kind {kind} is promoted without a plain summary"
        );
        // Offering a kind whose node cannot be built would leave the row doing nothing,
        // which is the failure the promoted actions are already guarded against.
        assert!(
            NativeNode::condition(kind).is_some(),
            "condition kind {kind} is promoted but builds no node"
        );
    }
}

/// Offering a condition is a promise that a perk using it builds.
///
/// Constructing the node is only the first half. The compiler writes the whole program into
/// a package, and a condition whose record it refuses turns a row someone chose into a build
/// that fails with an engine message. Every condition the picker offers is compiled here
/// against the installed packages, so the promise is checked rather than assumed.
#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES"]
fn every_offered_condition_compiles_into_a_program() {
    use sundial::package_authoring::open_shadowkeep_package_manager;
    use sundial::package_authoring::sandbox_perk::program::{
        Action, Position, Program, Trigger, compile,
    };
    let packages = crate::test_support::stock_packages();
    let manager = open_shadowkeep_package_manager(&packages).unwrap();
    let offered = nodes::CONDITIONS
        .iter()
        .filter(|node| node.support == nodes::Support::Authorable)
        .collect::<Vec<_>>();
    assert!(!offered.is_empty(), "no authorable native conditions");
    let failed = offered
        .into_iter()
        .filter_map(|node| {
            let Some(condition) = NativeNode::condition(node.kind) else {
                return Some(format!("{}: {} builds no node", node.kind, node.name));
            };
            let program = Program {
                trigger: Trigger::Native,
                native_trigger: Some(condition),
                actions: vec![Action::generate_orb(Position::Event)],
                ..Program::default()
            };
            compile(&manager, &program)
                .err()
                .map(|error| format!("{}: {} -> {error}", node.kind, node.name))
        })
        .collect::<Vec<_>>();
    assert!(
        failed.is_empty(),
        "conditions that do not build: {failed:#?}"
    );
}

/// A plain name is a promise that the workbench can place the thing it names. A kind
/// named without a buildable record would offer a row that produces nothing.
#[test]
fn every_plain_name_belongs_to_a_kind_the_workbench_can_place() {
    use sundial::package_authoring::sandbox_perk::action::native::fields;
    use sundial::package_authoring::sandbox_perk::nodes;
    use sundial::package_authoring::sandbox_perk::program::NativeNode;
    for kind in 0..=u8::MAX {
        assert_eq!(
            program::plain_action_title(kind).is_some(),
            program::plain_action_summary(kind).is_some(),
            "effect kind {kind} has an incomplete description"
        );
        assert_eq!(
            program::plain_condition_title(kind).is_some(),
            program::plain_condition_summary(kind).is_some(),
            "condition kind {kind} has an incomplete description"
        );
        if let Some(title) = program::plain_action_title(kind) {
            let node = NativeNode::effect(kind)
                .unwrap_or_else(|| panic!("{title} (effect kind {kind}) builds no record"));
            assert_eq!(node.kind, kind);
            assert!(!node.bytes.is_empty(), "{title} builds an empty record");
            let class = nodes::effect(kind).unwrap().class;
            assert!(
                fields::describe(class)
                    .unwrap()
                    .iter()
                    .any(|field| field.editable),
                "{title} has no editable field"
            );
        }
        if let Some(title) = program::plain_condition_title(kind) {
            let node = NativeNode::condition(kind)
                .unwrap_or_else(|| panic!("{title} (condition kind {kind}) builds no record"));
            assert_eq!(node.kind, kind);
            assert!(!node.bytes.is_empty(), "{title} builds an empty record");
        }
    }
}

#[test]
fn the_stock_use_order_counts_the_configurations_the_game_carries() {
    let rows = [
        row("One Configuration", 3, Choice::Effect(1)),
        row("Three Configurations", 10, Choice::Effect(2)),
        row("Three Configurations", 10, Choice::Effect(3)),
        row("Three Configurations", 10, Choice::Effect(4)),
        row("Two Configurations", 26, Choice::Effect(5)),
        row("Two Configurations", 26, Choice::Effect(6)),
    ];
    let titles = group_rows(
        rows.iter(),
        "",
        StockUse::Any,
        Detail::Advanced,
        Category::Any,
        Order::StockUse,
        &[],
    )
    .iter()
    .map(|group| group.title.to_owned())
    .collect::<Vec<_>>();
    assert_eq!(
        titles,
        [
            "Three Configurations",
            "Two Configurations",
            "One Configuration"
        ]
    );
}

#[test]
fn behavior_orders_sort_by_name_and_by_engine_kind_number() {
    let rows = [
        row("Spawn an Effect", 3, Choice::Action(Action::add_rounds(1))),
        row("Zero Native Kind", 44, Choice::Kind(44)),
        row("Alpha Stock Effect", 10, Choice::Effect(7)),
    ];
    let titles = |order| {
        group_rows(
            rows.iter(),
            "",
            StockUse::Any,
            Detail::Advanced,
            Category::Any,
            order,
            &[],
        )
        .iter()
        .map(|group| group.title.to_owned())
        .collect::<Vec<_>>()
    };
    assert_eq!(
        titles(Order::Name),
        ["Alpha Stock Effect", "Spawn an Effect", "Zero Native Kind"]
    );
    assert_eq!(
        titles(Order::Kind),
        ["Spawn an Effect", "Alpha Stock Effect", "Zero Native Kind"]
    );
    // A kill family carries no kind number, so it sorts after every numbered kind
    // instead of being dropped.
    let kill = Row {
        family: Family::Condition(trigger_family(Trigger::WeaponKill).unwrap()),
        ..row("A Kill", 0, Choice::Trigger(Trigger::WeaponKill))
    };
    assert_eq!(kill.family.kind(), None);
    let with_kill = [
        row("Spawn an Effect", 3, Choice::Action(Action::add_rounds(1))),
        kill,
    ];
    let ordered = group_rows(
        with_kill.iter(),
        "",
        StockUse::Any,
        Detail::Advanced,
        Category::Any,
        Order::Kind,
        &[],
    )
    .iter()
    .map(|group| group.title.to_owned())
    .collect::<Vec<_>>();
    assert_eq!(ordered, ["Spawn an Effect", "A Kill"]);
}

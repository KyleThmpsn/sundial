//! What the compiler reads before it converts anything: the source weapon and arm rigs, the
//! carrier and its extended native rig, and the source route of every action the carrier's state
//! table can request.
use super::*;

/// The source weapon and arms, their selector, and the carrier with its extended native rig.
pub(super) struct Sources {
    pub(super) owners: Vec<Component>,
    pub(super) hold: u32,
    pub(super) profile: u32,
    pub(super) selection_tag: u32,
    pub(super) arms_entity: u32,
    pub(super) alternatives: u64,
    pub(super) weapon_rig: source::Rig,
    pub(super) placement: std::sync::Arc<Payload>,
    pub(super) arm_skeleton_tag: u32,
    pub(super) arm_controls_tag: u32,
    pub(super) lookup_tag: u32,
    pub(super) arms: source::Rig,
    pub(super) coverage: Value,
    pub(super) controller: controller::Controller,
    /// The builtins the source weapon fixes for every request: its profile and its optic.
    pub(super) fixed: BTreeMap<u8, u32>,
    pub(super) carrier: native::Carrier,
    pub(super) extension: native::Extension,
    pub(super) native_rig: control::Rig,
}

impl Sources {
    /// Reads the source weapon named by the first part, its arm selection and the carrier of
    /// `item_tag`, and writes the carrier's rig extended with the source weapon section into
    /// `graph`.
    pub(super) fn read(
        source: &mut Reader,
        native: &mut Reader,
        parts: &[Value],
        item_tag: u32,
        graph: &Path,
    ) -> Result<Self> {
        let root = hash(
            parts.first().context("Empty source animation assembly")?,
            "entity",
        )?;
        let owners = components(source, root)?;
        let (_, weapon, wbase) = one(&owners, 0x80804A14, "weapon definition")?;
        let hold = weapon.u32(wbase + 0x310)?;
        let profile = weapon.u32(wbase + 0x314)?;
        let selection_tag = source.ref64(&weapon, wbase + 0x338)?;
        let selection = source.tag(selection_tag, Some(0x808025AA))?;
        let arms_entity = source.ref64(&selection, 0x10)?;
        let alternatives = selection.u64(0x20)?;
        let (_, weapon_skeleton, _) = one(&owners, source::SKELETON, "weapon skeleton")?;
        let (_, weapon_controls, _) = one(&owners, source::CONTROLS, "weapon controls")?;
        let (_, placement, _) = one(&owners, 0x80809F76, "weapon placement")?;
        let weapon_rig = source::Rig::read(&weapon_skeleton, &weapon_controls)?;
        let arm_owners = components(source, arms_entity)?;
        let (arm_skeleton_tag, arm_skeleton, _) =
            one(&arm_owners, source::SKELETON, "arm skeleton")?;
        let (arm_controls_tag, arm_controls, _) =
            one(&arm_owners, source::CONTROLS, "arm controls")?;
        let (lookup_tag, lookup, _) = one(&arm_owners, controller::LOOKUP, "arm action lookup")?;
        let arms = source::Rig::read(&arm_skeleton, &arm_controls)?;
        let coverage = weapon_section(&arms, &weapon_rig)?;
        let controller = controller::Controller::read(source, &lookup)?;
        ensure!(
            controller.declares(PROFILE, profile) && controller.declares(PROFILE, hold),
            "Source animation profile is absent from the arm dictionary"
        );
        let mut fixed = BTreeMap::from([(PROFILE, profile)]);
        if let Some(optic) = declared_optic(source, &weapon, wbase, &controller)? {
            fixed.insert(OPTIC, optic);
        }
        let carrier = native::Carrier::read(native, item_tag)?;
        let extension = native::extend(&carrier, &weapon_rig, graph)?;
        let native_rig = control::Rig::read(&extension.skeleton, &extension.controls)?;
        Ok(Self {
            owners,
            hold,
            profile,
            selection_tag,
            arms_entity,
            alternatives,
            weapon_rig,
            placement,
            arm_skeleton_tag,
            arm_controls_tag,
            lookup_tag,
            arms,
            coverage,
            controller,
            fixed,
            carrier,
            extension,
            native_rig,
        })
    }
}

/// Each carrier action's source route, the carrier actions the source does not name, and the
/// hand targets the converted clips may bind.
pub(super) struct Routes {
    /// Source reload state values by native reload action.
    pub(super) reloads: BTreeMap<u32, u32>,
    pub(super) routes: BTreeMap<u32, Vec<(u32, f32)>>,
    pub(super) absent: Vec<String>,
    pub(super) carrier_actions: BTreeSet<u32>,
    pub(super) carrier_markers: BTreeSet<u32>,
    /// The source placement's marker names and the carrier's.
    pub(super) targets: BTreeSet<u32>,
}

impl Routes {
    /// Routes every carrier action the source names through the source selector, with the
    /// weapon's fixed builtins and, for a reload, its source state value. Creates the graph's
    /// `animation` folder.
    pub(super) fn read(sources: &Sources, graph: &Path) -> Result<Self> {
        let controller = &sources.controller;
        let reloads = RELOADS
            .iter()
            .map(|(action, value)| (source::fnv(action), source::fnv(value)))
            .collect::<BTreeMap<_, _>>();
        let mut routes = BTreeMap::new();
        let mut absent = Vec::new();
        let carrier_actions = sources.carrier.actions()?;
        for &action in &carrier_actions {
            if !controller.names.contains_key(&action) {
                absent.push(format!("{action:08X}"));
                continue;
            }
            let mut inputs = sources.fixed.clone();
            if let Some(&value) = reloads.get(&action) {
                ensure!(
                    controller.declares(RELOAD, value),
                    "Source reload state value is absent from the arm dictionary"
                );
                inputs.insert(RELOAD, value);
            }
            routes.insert(action, controller.route(action, &inputs)?);
        }
        ensure!(!routes.is_empty(), "No native action has a source route");
        fs::create_dir_all(graph.join("animation"))?;
        let carrier_markers = crate::tiger::markers::read_native(&sources.carrier.markers.1)?
            .into_iter()
            .map(|m| m.name)
            .collect::<BTreeSet<_>>();
        let mut targets = native::placement_names(&sources.placement)?;
        targets.extend(&carrier_markers);
        Ok(Self {
            reloads,
            routes,
            absent,
            carrier_actions,
            carrier_markers,
            targets,
        })
    }
}

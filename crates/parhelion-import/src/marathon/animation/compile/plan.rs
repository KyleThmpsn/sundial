//! What the compiler reads before it converts anything: the source weapon and arm rigs, the
//! carrier and its extended native rig, and the source route of every action the carrier's state
//! table can request.
use super::*;

/// The source weapon and arms, their selector, and the carrier with its extended native rig.
pub(super) struct Sources {
    pub(super) weapon_driver: driver::Driver,
    pub(super) arm_driver: Value,
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
    pub(super) fixed_layers: layer::Fixed,
    pub(super) local_controller: Option<controller::Controller>,
    pub(super) local_error: Option<String>,
    /// The builtins the source weapon fixes for every request: its profile and its optic.
    pub(super) fixed: BTreeMap<u8, u32>,
    pub(super) carrier: native::Carrier,
    pub(super) magazine_reload: bool,
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
        let weapon_driver = driver::Driver::read(source, root, &owners)?;
        let (_, placement, _) = one(&owners, 0x80809F76, "weapon placement")?;
        let weapon_rig = source::Rig::read(&weapon_driver.skeleton.1, &weapon_driver.controls.1)?;
        let arm_owners = components(source, arms_entity)?;
        let arm_driver = driver::Driver::read(source, arms_entity, &arm_owners)?;
        let arm_skeleton_tag = arm_driver.skeleton.0;
        let arm_controls_tag = arm_driver.controls.0;
        let lookup_tag = arm_driver.lookup.0;
        let arms = source::Rig::read(&arm_driver.skeleton.1, &arm_driver.controls.1)?;
        let coverage = weapon_section(&arms, &weapon_rig)?;
        let controller = controller::Controller::read(source, &arm_driver.lookup.1)?;
        let (local_controller, local_error) =
            match controller::Controller::read(source, &weapon_driver.lookup.1) {
                Ok(local) => (Some(local), None),
                Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
                Err(error) => (None, Some(format!("Weapon lookup: {error:#}"))),
            };
        ensure!(
            controller.declares(PROFILE, profile) && controller.declares(PROFILE, hold),
            "Source animation profile is absent from the arm dictionary"
        );
        let mut fixed = BTreeMap::from([(PROFILE, profile)]);
        if let Some(optic) = declared_optic(source, &weapon, wbase, &controller)? {
            fixed.insert(OPTIC, optic);
        }
        let fixed_layers = layer::Fixed::read(
            source,
            arm_driver
                .pose
                .as_ref()
                .context("Source arm driver has no connected pose controller")?,
            &controller,
            &arms,
            &fixed,
        )?;
        let carrier = native::Carrier::read(native, item_tag)?;
        let magazine_reload =
            donor::magazine(source, root)? && native::magazine_candidate(native, item_tag)?;
        let extension = native::extend(&carrier, &weapon_rig, graph)?;
        let native_rig = control::Rig::read(&extension.skeleton, &extension.controls)?;
        Ok(Self {
            weapon_driver,
            arm_driver: arm_driver.report,
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
            fixed_layers,
            local_controller,
            local_error,
            fixed,
            carrier,
            magazine_reload,
            extension,
            native_rig,
        })
    }
}

/// Each carrier action's source route, the carrier actions the source does not name, and the
/// hand targets the converted clips may bind.
pub(super) struct Routes {
    pub(super) actions: BTreeMap<u32, selection::Tree<u32>>,
    pub(super) requests: BTreeMap<u32, Request>,
    /// Weighted choices by request identity, which is distinct from an action name.
    pub(super) routes: BTreeMap<u32, Vec<(u32, f32)>>,
    pub(super) absent: Vec<String>,
    pub(super) carrier_actions: BTreeSet<u32>,
    pub(super) carrier_markers: BTreeSet<u32>,
    /// The source placement's marker names and the carrier's.
    pub(super) targets: BTreeSet<u32>,
}

pub(super) struct Request {
    pub(super) action: u32,
    pub(super) inputs: BTreeMap<u8, u32>,
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
        let mut actions = BTreeMap::new();
        let mut requests = BTreeMap::new();
        let mut absent = Vec::new();
        let mut carrier_actions = sources.carrier.actions()?;
        if sources.magazine_reload {
            carrier_actions.extend(RELOADS.iter().map(|(name, _)| source::fnv(name)));
        }
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
            let mut dependencies = controller.inputs(action)?;
            dependencies.extend(sources.fixed_layers.dimensions());
            if let Some(local) = &sources.local_controller {
                // The local controller may change while the arm descriptor remains the same.
                // Include the idle fallback's inputs as well as the named action's inputs.
                for name in [action, source::fnv("idle")] {
                    if local.names.contains_key(&name) {
                        dependencies.extend(local.inputs(name)?);
                    }
                }
            }
            let dimensions = dependencies
                .into_iter()
                .filter(|b| !inputs.contains_key(b) && selection::names(*b).is_some())
                .collect::<Vec<_>>();
            let tree = selection::expand(&dimensions, &mut inputs, &mut |inputs| {
                let id = u32::try_from(requests.len())?;
                requests.insert(
                    id,
                    Request {
                        action,
                        inputs: inputs.clone(),
                    },
                );
                routes.insert(id, controller.route(action, inputs)?);
                Ok(id)
            })?;
            actions.insert(action, tree);
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
            actions,
            requests,
            routes,
            absent,
            carrier_actions,
            carrier_markers,
            targets,
        })
    }
}

//! Explicit third-person character selection and package-backed animation export. A shared action
//! bank is not enough to choose a runner rig, so callers provide the actual source character entity.
use super::*;

/// Export the connected character controller, its rig, all bank clips and weapon-profile routes.
/// Native clip envelopes are inspectable candidates. No character profile is installed by this API.
pub fn inspect(source: &mut Reader, entity: u32, profile: u32, directory: &Path) -> Result<Value> {
    let owners = components(source, entity)?;
    let driver = driver::Driver::read(source, entity, &owners)?;
    let controller = controller::Controller::read(source, &driver.lookup.1)?;
    ensure!(
        controller.declares(3, profile),
        "Character controller does not declare the selected weapon profile"
    );
    fs::create_dir_all(directory.join("source-animation"))?;
    let bank = source.tag(controller.bank_tag, Some(0x80803BE9))?;
    let mut clips = BTreeMap::new();
    export_clips(source, &bank, directory, &mut clips)?;
    let definition = driver.lookup.2;
    let mut files = BTreeMap::new();
    for (name, tag, payload) in [
        ("lookup", driver.lookup.0, driver.lookup.1.clone()),
        ("bank", controller.bank_tag, bank),
        ("skeleton", driver.skeleton.0, driver.skeleton.1.clone()),
        ("controls", driver.controls.0, driver.controls.1.clone()),
        (
            "dictionary",
            driver.lookup.1.u32(definition + 0xEC)?,
            source.tag(driver.lookup.1.u32(definition + 0xEC)?, Some(0x8080ACBF))?,
        ),
        (
            "states",
            driver.lookup.1.u32(definition + 0xF4)?,
            source.tag(driver.lookup.1.u32(definition + 0xF4)?, Some(0x80803950))?,
        ),
    ] {
        files.insert(name, save(directory, tag, name, &payload)?);
    }
    let mut routes = BTreeMap::new();
    for &action in controller.names.keys() {
        let fixed = BTreeMap::from([(3, profile)]);
        let attempt = (|| -> Result<Value> {
            let default = controller.route(action, &fixed)?;
            let adaptation = (|| -> Result<Value> {
                let dependencies = controller.inputs(action)?;
                let dimensions = dependencies
                    .iter()
                    .copied()
                    .filter(|b| !fixed.contains_key(b) && selection::names(*b).is_some())
                    .collect::<Vec<_>>();
                let choices = selection::expand(&dimensions, &mut fixed.clone(), &mut |inputs| {
                    controller.route(action, inputs)
                })?
                .reduced();
                Ok(json!({"status":"selected","choices":choices,
                    "unadapted_inputs":dependencies.into_iter().filter(|b| !fixed.contains_key(b) && selection::names(*b).is_none()).collect::<Vec<_>>()}))
            })();
            let adaptation = match adaptation {
                Ok(value) => value,
                Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
                Err(error) => json!({"status":"untranslated","reason":format!("{error:#}")}),
            };
            Ok(
                json!({"status":"selected","default_names":default.iter().map(|(d,w)| (controller.descriptors[*d as usize].name,*w)).collect::<Vec<_>>(),
                "adaptation":adaptation,"fixed_inputs":fixed}),
            )
        })();
        let value = match attempt {
            Ok(value) => value,
            Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
            Err(error) => json!({"status":"unselected","reason":format!("{error:#}")}),
        };
        routes.insert(action.to_string(), value);
    }
    let controls = &driver.controls.1;
    let rows = controls.array(driver.controls.2 + 0xA8, 72, Some(0x8080AE76))?;
    let mut modes = BTreeMap::<u8, usize>::new();
    let mut compositions = BTreeMap::<u8, usize>::new();
    for row in rows {
        *modes.entry(controls.u8(row + 8)?).or_default() += 1;
        *compositions.entry(controls.u8(row + 0x30)?).or_default() += 1;
    }
    let rig = match source::Rig::read(&driver.skeleton.1, controls) {
        Ok(rig) => {
            json!({"status":"source_fk","bones":rig.names.len(),"controls":rig.controls.len(),"extensions":rig.extensions})
        }
        Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
        Err(error) => json!({"status":"untranslated","reason":format!("{error:#}")}),
    };
    Ok(
        json!({"revision":1,"perspective":"third_person","status":"source_export",
        "selection":{"entity":entity,"profile":profile,"driver":driver.report,"bank":controller.bank_tag,"routes":routes},
        "files":files,"rig":rig,"control_parent_modes":modes,"control_compositions":compositions,
        "descriptors":controller.descriptors.iter().enumerate().map(|(i,d)| json!({"index":i,"name":d.name,"rate":d.rate,"clip":d.clip.map(|c|controller.clips[c])})).collect::<Vec<_>>(),
        "clips":clips.into_values().collect::<Vec<_>>(),"gameplay_verified":false,
        "limits":"Explicit source character only. Shared bank membership does not select a runner. Native input adaptation is not source producer recovery. Full character FK, aim frames, split position controls, locomotion layers and native character installation remain untranslated."}),
    )
}

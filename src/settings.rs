use super::*;

pub(super) fn load_installed_sunrise_defaults(install_path: &Path) -> Result<Value, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::FreeLibrary,
        System::LibraryLoader::{
            FindResourceW, LOAD_LIBRARY_AS_DATAFILE_EXCLUSIVE, LoadLibraryExW, LoadResource,
            LockResource, SizeofResource,
        },
    };

    const DEFAULT_SETTINGS_RESOURCE: *const u16 = 101usize as *const u16;
    const RESOURCE_DATA_TYPE: *const u16 = 10usize as *const u16;

    let module_path = install_path.join(r"bin\x64\steam_api64.dll");
    let wide_path: Vec<u16> = module_path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // Loading as a data file reads resources without running Project Sunrise's DLL entry point.
    let module = unsafe {
        LoadLibraryExW(
            wide_path.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_AS_DATAFILE_EXCLUSIVE,
        )
    };
    if module.is_null() {
        return Err(format!(
            "Could not read Project Sunrise's bundled defaults from {}: {}",
            module_path.display(),
            io::Error::last_os_error()
        ));
    }

    let result = (|| {
        // Resource 101 is IDR_DEFAULT_SETTINGS in every supported Sunrise release.
        let resource =
            unsafe { FindResourceW(module, DEFAULT_SETTINGS_RESOURCE, RESOURCE_DATA_TYPE) };
        if resource.is_null() {
            return Err(format!(
                "The installed Project Sunrise module does not contain its default settings resource: {}",
                module_path.display()
            ));
        }
        let size = unsafe { SizeofResource(module, resource) } as usize;
        let loaded = unsafe { LoadResource(module, resource) };
        let bytes = if loaded.is_null() {
            std::ptr::null()
        } else {
            unsafe { LockResource(loaded) }.cast::<u8>()
        };
        if size == 0 || bytes.is_null() {
            return Err("The installed Project Sunrise default settings resource is empty".into());
        }
        // The resource remains valid while the data-file module is loaded; clone before releasing it.
        let encoded = unsafe { std::slice::from_raw_parts(bytes, size) }.to_vec();
        let document: Value = serde_json::from_slice(&encoded).map_err(|error| {
            format!("Project Sunrise's bundled defaults are invalid JSON: {error}")
        })?;
        if game_settings::schema_version(&document).is_none() {
            return Err("Project Sunrise's bundled defaults have no valid schema version".into());
        }
        validate_document(&document).map_err(|error| {
            format!("Project Sunrise's bundled defaults contain an unexpected setting: {error}")
        })?;
        Ok(document)
    })();
    unsafe {
        FreeLibrary(module);
    }
    result
}

pub(super) fn load_json(path: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            format!(
                "No Project Sunrise settings.json was found in the selected installation. Expected: {}. Choose the Destiny 2 Shadowkeep folder containing destiny2.exe and the bin folder, and confirm Project Sunrise is installed there",
                path.display()
            )
        } else {
            format!("Could not read {}: {error}", path.display())
        }
    })?;
    serde_json::from_str(&raw).map_err(|e| format!("Invalid JSON in {}: {e}", path.display()))
}

pub(super) fn verify_source_unchanged(path: &Path, expected: &Value) -> Result<(), String> {
    let current = load_json(path)?;
    if current == *expected {
        Ok(())
    } else {
        Err("settings.json changed outside Sundial after it was loaded. Reload before saving so newer data is not overwritten".into())
    }
}

pub(super) fn save_json(path: &Path, document: &Value) -> Result<PathBuf, String> {
    let backup_root = preferences_path()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .ok_or("Could not locate the local backup folder")?
        .join("backups");
    save_json_with_backup_root(path, document, &backup_root)
}

pub(super) fn save_json_with_backup_root(
    path: &Path,
    document: &Value,
    backup_root: &Path,
) -> Result<PathBuf, String> {
    let mut encoded = encode_settings(document)?;
    encoded.push('\n');
    if encoded.len() > MAX_SETTINGS_BYTES {
        return Err(format!(
            "The encoded settings would be {} bytes; Sunrise requires less than {} bytes",
            encoded.len(),
            MAX_SETTINGS_BYTES + 1
        ));
    }

    fs::create_dir_all(backup_root)
        .map_err(|e| format!("Could not create {}: {e}", backup_root.display()))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("Could not create backup timestamp: {e}"))?
        .as_nanos();
    let backup = backup_root.join(format!("settings-{timestamp}-{}.json", std::process::id()));
    create_backup(path, &backup)?;

    storage::replace_file(path, encoded.as_bytes())
        .map_err(|e| format!("Could not safely replace {}: {e}", path.display()))?;
    let verification = load_json(path).and_then(|saved| {
        if saved == *document {
            Ok(())
        } else {
            Err("the saved document did not match the requested settings".to_owned())
        }
    });
    if let Err(error) = verification {
        let restore = fs::read(&backup)
            .and_then(|contents| storage::replace_file(path, &contents))
            .map_err(|restore_error| restore_error.to_string());
        return match restore {
            Ok(()) => Err(format!(
                "Could not verify the saved settings ({error}); the original file was restored"
            )),
            Err(restore_error) => Err(format!(
                "Could not verify the saved settings ({error}), and restoring the backup failed: {restore_error}. The backup is at {}",
                backup.display()
            )),
        };
    }
    Ok(backup)
}

pub(super) fn create_backup(source: &Path, destination: &Path) -> Result<(), String> {
    let mut source_file = fs::File::open(source)
        .map_err(|e| format!("Could not open {} for backup: {e}", source.display()))?;
    let mut backup_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|e| format!("Could not create {}: {e}", destination.display()))?;
    if let Err(error) =
        io::copy(&mut source_file, &mut backup_file).and_then(|_| backup_file.sync_all())
    {
        drop(backup_file);
        let _ = fs::remove_file(destination);
        return Err(format!(
            "Could not create {}: {error}",
            destination.display()
        ));
    }
    Ok(())
}

pub(super) fn create_adjacent_backup(source: &Path) -> Result<PathBuf, String> {
    let file_name = source
        .file_name()
        .ok_or_else(|| format!("{} has no file name", source.display()))?
        .to_string_lossy();
    let destination = source.with_file_name(format!("{file_name}.bak"));
    let source_contents = fs::read(source)
        .map_err(|e| format!("Could not read {} for backup: {e}", source.display()))?;

    if destination.exists() {
        let existing = fs::read(&destination)
            .map_err(|e| format!("Could not read {}: {e}", destination.display()))?;
        if existing == source_contents {
            return Ok(destination);
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| format!("Could not create backup timestamp: {e}"))?
            .as_nanos();
        let archived = source.with_file_name(format!("{file_name}.bak.previous-{timestamp}"));
        create_backup(&destination, &archived)?;
        storage::replace_file(&destination, &source_contents).map_err(|e| {
            format!(
                "Could not update {} after preserving its previous contents at {}: {e}",
                destination.display(),
                archived.display()
            )
        })?;
    } else {
        create_backup(source, &destination)?;
    }

    let copied = fs::read(&destination)
        .map_err(|e| format!("Could not verify {}: {e}", destination.display()))?;
    if copied != source_contents {
        return Err(format!(
            "The safety copy at {} did not match the source",
            destination.display()
        ));
    }
    Ok(destination)
}

pub(super) fn encode_settings(document: &Value) -> Result<String, String> {
    fn write_value(value: &Value, indent: usize, output: &mut String) -> Result<(), String> {
        match value {
            Value::Object(object) if !object.is_empty() => {
                output.push_str("{\n");
                for (index, (key, child)) in object.iter().enumerate() {
                    output.push_str(&" ".repeat(indent + 2));
                    output.push_str(
                        &serde_json::to_string(key)
                            .map_err(|e| format!("Could not encode setting name: {e}"))?,
                    );
                    output.push_str(": ");
                    write_value(child, indent + 2, output)?;
                    if index + 1 != object.len() {
                        output.push(',');
                    }
                    output.push('\n');
                }
                output.push_str(&" ".repeat(indent));
                output.push('}');
            }
            Value::Array(_) => output.push_str(
                &serde_json::to_string(value)
                    .map_err(|e| format!("Could not encode settings array: {e}"))?,
            ),
            _ => output.push_str(
                &serde_json::to_string(value)
                    .map_err(|e| format!("Could not encode setting: {e}"))?,
            ),
        }
        Ok(())
    }

    let mut output = String::new();
    write_value(document, 0, &mut output)?;
    Ok(output)
}

pub(super) fn validate_document(document: &Value) -> Result<(), String> {
    game_settings::validate(document)?;
    validate_characters(document)
}

pub(super) fn validate_characters(document: &Value) -> Result<(), String> {
    const MAX_CHARACTERS: usize = 3;
    const MAX_PLUGS: usize = 12;
    const NO_DEFINITION_HASH: u64 = 0x811C_9DC5;

    let Some(characters_value) = document.pointer("/state/characters") else {
        return Ok(());
    };
    let characters = characters_value
        .as_array()
        .ok_or("state.characters must be an array")?;
    if characters.len() > MAX_CHARACTERS {
        return Err(format!(
            "state.characters cannot contain more than {MAX_CHARACTERS} characters"
        ));
    }
    for (character_index, character) in characters.iter().enumerate() {
        let number = character_index + 1;
        let character = character
            .as_object()
            .ok_or_else(|| format!("Character {number} must be an object"))?;
        character
            .get("soid")
            .and_then(parse_unsigned_value)
            .filter(|soid| *soid != 0)
            .ok_or_else(|| format!("Character {number} has an invalid SOID"))?;

        let optional_bounded = |key: &str, label: &str, maximum: u64| {
            let Some(value) = character.get(key) else {
                return Ok(());
            };
            value
                .as_u64()
                .filter(|value| *value <= maximum)
                .map(|_| ())
                .ok_or_else(|| format!("Character {number} has an invalid {label}"))
        };
        optional_bounded("class", "class", 2)?;
        optional_bounded("race", "race", 2)?;
        optional_bounded("gender", "gender", 1)?;
        optional_bounded("level", "level (expected 0 to 255)", u8::MAX.into())?;
        for (key, label) in [
            ("movement_ability", "movement ability"),
            ("grenade_ability", "grenade ability"),
            ("super_ability", "super ability"),
            ("melee_ability", "melee ability"),
            ("class_ability", "class ability"),
        ] {
            optional_bounded(key, label, 63)?;
        }

        let Some(equipment_value) = character.get("equipment") else {
            continue;
        };
        let equipment = equipment_value
            .as_object()
            .ok_or_else(|| format!("Character {number} equipment must be an object"))?;

        if let Some(issue) = character_ability_issue(character) {
            return Err(format!("Character {number} {issue}"));
        }
        for slot in equipment.keys() {
            if !SLOTS.iter().any(|(known, _, _)| known == slot) {
                return Err(format!(
                    "Character {number} has an unknown equipment slot: {slot}"
                ));
            }
        }
        for &(slot, label, _) in SLOTS {
            let Some(equipped_value) = equipment.get(slot) else {
                continue;
            };
            if equipped_value.is_null() {
                continue;
            }
            let equipped = equipped_value
                .as_object()
                .ok_or_else(|| format!("Character {number} {label} must be an object or null"))?;
            equipped
                .get("definition_hash")
                .and_then(parse_unsigned_value)
                .filter(|hash| u32::try_from(*hash).is_ok() && *hash != NO_DEFINITION_HASH)
                .ok_or_else(|| {
                    format!("Character {number} {label} has an invalid definition hash")
                })?;
            equipped
                .get("instance_soid")
                .and_then(parse_unsigned_value)
                .filter(|soid| *soid != 0)
                .ok_or_else(|| {
                    format!("Character {number} {label} has an invalid instance SOID")
                })?;
            equipped
                .get("level")
                .and_then(Value::as_i64)
                .filter(|level| (0..=i64::from(i32::MAX)).contains(level))
                .ok_or_else(|| format!("Character {number} {label} has an invalid item level"))?;
            equipped
                .get("quantity")
                .and_then(Value::as_i64)
                .filter(|quantity| (1..=i64::from(i32::MAX)).contains(quantity))
                .ok_or_else(|| format!("Character {number} {label} has an invalid quantity"))?;

            match equipped.get("plugs") {
                Some(Value::Null) => {}
                Some(Value::Array(plugs)) => {
                    if plugs.len() > MAX_PLUGS {
                        return Err(format!(
                            "Character {number} {label} cannot contain more than {MAX_PLUGS} plugs"
                        ));
                    }
                    for plug in plugs {
                        if !plug.is_null()
                            && !parse_unsigned_value(plug).is_some_and(|hash| {
                                u32::try_from(hash).is_ok() && hash != NO_DEFINITION_HASH
                            })
                        {
                            return Err(format!(
                                "Character {number} {label} contains an invalid plug hash"
                            ));
                        }
                    }
                }
                _ => {
                    return Err(format!(
                        "Character {number} {label} plugs must be null or an array"
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(super) fn character_ability_issue(
    character: &serde_json::Map<String, Value>,
) -> Option<String> {
    let subclass_hash = character
        .get("equipment")?
        .as_object()?
        .get("subclass")?
        .as_object()?
        .get("definition_hash")
        .and_then(parse_unsigned_value)?;
    let (subclass_name, middle_super) = shadowkeep_subclass_rules(subclass_hash)?;

    for (key, range, label) in [
        ("movement_ability", 4..=6, "movement ability"),
        ("grenade_ability", 7..=9, "grenade ability"),
        ("class_ability", 2..=3, "class ability"),
    ] {
        if let Some(value) = character.get(key).and_then(Value::as_u64)
            && !range.contains(&value)
        {
            return Some(format!(
                "has an unsupported {label} entry {value} for {subclass_name}"
            ));
        }
    }

    let (Some(super_ability), Some(melee_ability)) = (
        character.get("super_ability").and_then(Value::as_u64),
        character.get("melee_ability").and_then(Value::as_u64),
    ) else {
        return None;
    };
    let supported = [(10, 11), (10, 15), (middle_super, 21)];
    (!supported.contains(&(super_ability, melee_ability))).then(|| {
        format!(
            "has an unsupported super and melee combination ({super_ability}/{melee_ability}) for {subclass_name}; expected 10/11, 10/15, or {middle_super}/21"
        )
    })
}

pub(super) fn repair_known_ability_pairs(document: &mut Value) -> usize {
    let Some(characters) = document
        .pointer_mut("/state/characters")
        .and_then(Value::as_array_mut)
    else {
        return 0;
    };

    let mut repaired = 0;
    for character in characters {
        let Some(character) = character.as_object_mut() else {
            continue;
        };
        let Some(subclass_hash) = character
            .get("equipment")
            .and_then(Value::as_object)
            .and_then(|equipment| equipment.get("subclass"))
            .and_then(Value::as_object)
            .and_then(|subclass| subclass.get("definition_hash"))
            .and_then(parse_unsigned_value)
        else {
            continue;
        };
        let Some((_, middle_super)) = shadowkeep_subclass_rules(subclass_hash) else {
            continue;
        };
        let (Some(super_ability), Some(melee_ability)) = (
            character.get("super_ability").and_then(Value::as_u64),
            character.get("melee_ability").and_then(Value::as_u64),
        ) else {
            continue;
        };
        let supported = [(10, 11), (10, 15), (middle_super, 21)];
        if supported.contains(&(super_ability, melee_ability)) {
            continue;
        }

        // The melee entry identifies the tree for every Shadowkeep subclass.
        // Prefer it when recovering a mismatched pair, then use a distinctive
        // middle-tree super as a fallback before returning to the top tree.
        let corrected = match melee_ability {
            11 => (10, 11),
            15 => (10, 15),
            21 => (middle_super, 21),
            _ if super_ability == 20 => (middle_super, 21),
            _ => (10, 11),
        };
        character.insert("super_ability".into(), Value::from(corrected.0));
        character.insert("melee_ability".into(), Value::from(corrected.1));
        repaired += 1;
    }
    repaired
}

fn shadowkeep_subclass_rules(subclass_hash: u64) -> Option<(&'static str, u64)> {
    Some(match subclass_hash {
        // Arcstrider and Sentinel route their guard supers through the
        // attunement selected by the melee entry while retaining entry 10.
        0x4F91_DC97 => ("Arcstrider", 10),
        0xC99B_33E9 => ("Sentinel", 10),
        // The other seven Shadowkeep subclasses carry a distinct middle-tree
        // super at entry 20.
        0xB055_4739 => ("Striker", 20),
        0xB920_CE9A => ("Sunbreaker", 20),
        0xD8B8_D1FC => ("Gunslinger", 20),
        0xC048_3D8B => ("Nightstalker", 20),
        0xCF88_FEA5 => ("Dawnblade", 20),
        0x686A_154A => ("Stormcaller", 20),
        0xE7BC_88B0 => ("Voidwalker", 20),
        _ => return None,
    })
}

pub(super) fn preferences_path() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("Sundial").join("paths.json"))
}

pub(super) fn catalog_path() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("Sundial").join("catalog").join("d2sk-86657.json"))
}

pub(super) fn settings_path_for_install(install: &Path, layout: SettingsLayout) -> PathBuf {
    install.join(layout.relative_path())
}

pub(super) fn resolve_settings_path(
    install: &Path,
    preferred_layout: Option<SettingsLayout>,
) -> SettingsPathResolution {
    if let Some(layout) = preferred_layout {
        let path = settings_path_for_install(install, layout);
        if path.is_file() {
            return SettingsPathResolution::Found(layout, path);
        }
    }

    let existing = SettingsLayout::ALL
        .into_iter()
        .filter_map(|layout| {
            let path = settings_path_for_install(install, layout);
            path.is_file().then_some((layout, path))
        })
        .collect::<Vec<_>>();
    match existing.as_slice() {
        [] => SettingsPathResolution::Missing,
        [(layout, path)] => SettingsPathResolution::Found(*layout, path.clone()),
        _ => SettingsPathResolution::Ambiguous,
    }
}

pub(super) fn missing_settings_message(install: &Path) -> String {
    let root = settings_path_for_install(install, SettingsLayout::Root);
    let bin_x64 = settings_path_for_install(install, SettingsLayout::BinX64);
    format!(
        "No Project Sunrise settings.json was found in the selected installation. Checked {} and {}. Choose the Destiny 2 Shadowkeep folder containing destiny2.exe and confirm Project Sunrise is installed there",
        root.display(),
        bin_x64.display()
    )
}

pub(super) fn saved_install() -> Option<InstallSelection> {
    let path = preferences_path()?;
    let raw = fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<Value>(&raw).ok()?;
    let install_path = value
        .get("install")
        .and_then(Value::as_str)
        .map(PathBuf::from)?;
    let preferred_layout = value
        .get("settings_layout")
        .and_then(Value::as_str)
        .and_then(SettingsLayout::from_preference);
    Some(InstallSelection {
        install_path,
        preferred_layout,
    })
}

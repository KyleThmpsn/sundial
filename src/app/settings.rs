use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use pelite::resources::Name;
use serde_json::Value;

use crate::{game_settings, paths, storage};

use crate::hash::parse_unsigned_value;

use super::{Preferences, SLOTS, SettingsLayout, SettingsPathResolution, inventory, progression};

const SUNRISE_MODULE_RELATIVE_PATH: &str = "bin/x64/steam_api64.dll";

pub(super) fn detect_sunrise_version(install_path: &Path) -> String {
    installed_sunrise_module_version(install_path).unwrap_or_else(|| "Not detected".into())
}

fn installed_sunrise_module_version(install_path: &Path) -> Option<String> {
    let module_path = install_path.join(SUNRISE_MODULE_RELATIVE_PATH);
    let bytes = fs::read(module_path).ok()?;
    let image = pelite::PeFile::from_bytes(&bytes).ok()?;
    let version_info = image.resources().ok()?.version_info().ok()?.file_info();
    let is_sunrise = version_info.strings.values().any(|strings| {
        strings.iter().any(|(key, value)| {
            (key.eq_ignore_ascii_case("ProductName") || key.eq_ignore_ascii_case("FileDescription"))
                && value.trim().eq_ignore_ascii_case("Sunrise")
        })
    });
    if !is_sunrise {
        return None;
    }
    let fixed = version_info.fixed?;
    (fixed.dwSignature == pelite::image::VS_FIXEDFILEINFO_SIGNATURE)
        .then(|| normalize_sunrise_version(&fixed.dwProductVersion.to_string()))?
}

pub(super) fn normalize_sunrise_version(version: &str) -> Option<String> {
    let mut components = version
        .trim()
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if components.len() < 2 || components.len() > 4 {
        return None;
    }
    while components.len() > 2 && components.last() == Some(&0) {
        components.pop();
    }
    Some(
        components
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join("."),
    )
}

pub(super) fn load_installed_sunrise_defaults(install_path: &Path) -> Result<Value, String> {
    let module_path = install_path.join(SUNRISE_MODULE_RELATIVE_PATH);
    let bytes = fs::read(&module_path).map_err(|error| {
        format!(
            "Could not read Project Sunrise's bundled defaults from {}: {error}",
            module_path.display()
        )
    })?;
    let image = pelite::PeFile::from_bytes(&bytes).map_err(|error| {
        format!(
            "The installed Project Sunrise module is not a valid PE file ({}): {error}",
            module_path.display()
        )
    })?;
    let resources = image.resources().map_err(|error| {
        format!(
            "Could not read resources from the installed Project Sunrise module ({}): {error}",
            module_path.display()
        )
    })?;
    // Resource 101 is IDR_DEFAULT_SETTINGS in every supported Sunrise release.
    let encoded = resources
        .find_resource(&[Name::Id(10), Name::Id(101)])
        .map_err(|_| {
            format!(
                "The installed Project Sunrise module does not contain its default settings resource: {}",
                module_path.display()
            )
        })?;
    if encoded.is_empty() {
        return Err("The installed Project Sunrise default settings resource is empty".into());
    }
    let mut document: Value = serde_json::from_slice(encoded)
        .map_err(|error| format!("Project Sunrise's bundled defaults are invalid JSON: {error}"))?;
    game_settings::ensure_schema_v8_preferences(&mut document);
    if game_settings::schema_version(&document).is_none() {
        return Err("Project Sunrise's bundled defaults have no valid schema version".into());
    }
    validate_document(&document).map_err(|error| {
        format!("Project Sunrise's bundled defaults contain an unexpected setting: {error}")
    })?;
    Ok(document)
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
    let mut document: Value = serde_json::from_str(&raw)
        .map_err(|e| format!("Invalid JSON in {}: {e}", path.display()))?;
    game_settings::ensure_schema_v8_preferences(&mut document);
    Ok(document)
}

pub(super) fn verify_source_unchanged(path: &Path, expected: &Value) -> Result<(), String> {
    let current = load_json(path)?;
    if current == *expected {
        Ok(())
    } else {
        Err("settings.json changed outside Sundial after it was loaded. Reload before saving so newer data is not overwritten".into())
    }
}

pub(super) struct SaveJsonResult {
    pub(super) backup: PathBuf,
    pub(super) encoded_bytes: usize,
    pub(super) size_limit_bytes: usize,
    pub(super) compacted: bool,
    pub(super) exceeds_size_limit: bool,
}

pub(super) struct PreparedSettings {
    encoded: String,
    pub(super) encoded_bytes: usize,
    pub(super) size_limit_bytes: usize,
    pub(super) compacted: bool,
    pub(super) exceeds_size_limit: bool,
}

#[derive(Clone, Copy)]
struct SettingsSizeLimitTier {
    through_schema: u64,
    bytes: usize,
}

// This records the lowest cap known to have shipped with each schema. Schema 5 briefly shipped
// with 128 KiB before Sunrise raised the cap without changing the schema, so keeping it in the
// 128 KiB tier preserves compatibility with every v5 build. Unknown future schemas inherit the
// latest known cap until a new boundary is added here.
const SETTINGS_SIZE_LIMITS: &[SettingsSizeLimitTier] = &[
    SettingsSizeLimitTier {
        through_schema: 3,
        bytes: 64 * 1024,
    },
    SettingsSizeLimitTier {
        through_schema: 5,
        bytes: 128 * 1024,
    },
    SettingsSizeLimitTier {
        through_schema: u64::MAX,
        bytes: 1024 * 1024,
    },
];

pub(super) fn settings_size_limit_for_schema(schema: Option<u64>) -> usize {
    let schema = schema.unwrap_or_default();
    SETTINGS_SIZE_LIMITS
        .iter()
        .find(|tier| schema <= tier.through_schema)
        .expect("the final settings-size tier must cover every schema")
        .bytes
}

pub(super) fn prepare_settings(document: &Value) -> Result<PreparedSettings, String> {
    let size_limit_bytes = settings_size_limit_for_schema(game_settings::schema_version(document));
    let mut encoded = encode_settings(document)?;
    encoded.push_str("\r\n");
    let compacted = encoded.len() > size_limit_bytes;
    if compacted {
        encoded = serde_json::to_string(document)
            .map_err(|e| format!("Could not compact settings JSON: {e}"))?;
        encoded.push_str("\r\n");
    }
    let encoded_bytes = encoded.len();
    Ok(PreparedSettings {
        encoded,
        encoded_bytes,
        size_limit_bytes,
        compacted,
        exceeds_size_limit: encoded_bytes > size_limit_bytes,
    })
}

pub(super) fn save_json(path: &Path, document: &Value) -> Result<SaveJsonResult, String> {
    let backup_root = backups_path().ok_or("Could not locate the local backup folder")?;
    save_json_with_backup_root(path, document, &backup_root)
}

pub(super) fn save_json_with_backup_root(
    path: &Path,
    document: &Value,
    backup_root: &Path,
) -> Result<SaveJsonResult, String> {
    let prepared = prepare_settings(document)?;

    fs::create_dir_all(backup_root)
        .map_err(|e| format!("Could not create {}: {e}", backup_root.display()))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("Could not create backup timestamp: {e}"))?
        .as_nanos();
    let schema = backup_schema_label(path);
    let backup = backup_root.join(format!("settings-{schema}-{timestamp}.json"));
    create_backup(path, &backup)?;

    storage::replace_file(path, prepared.encoded.as_bytes())
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
    Ok(SaveJsonResult {
        backup,
        encoded_bytes: prepared.encoded_bytes,
        size_limit_bytes: prepared.size_limit_bytes,
        compacted: prepared.compacted,
        exceeds_size_limit: prepared.exceeds_size_limit,
    })
}

fn backup_schema_label(source: &Path) -> String {
    fs::read(source)
        .ok()
        .and_then(|contents| serde_json::from_slice::<Value>(&contents).ok())
        .and_then(|document| game_settings::schema_version(&document))
        .map_or_else(|| "v0".to_owned(), |schema| format!("v{schema}"))
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
    const MAX_INLINE_WIDTH: usize = 80;

    fn contains_object(value: &Value) -> bool {
        match value {
            Value::Object(_) => true,
            Value::Array(array) => array.iter().any(contains_object),
            _ => false,
        }
    }

    fn current_column(output: &str) -> usize {
        output
            .rsplit_once('\n')
            .map_or(output.len(), |(_, line)| line.len())
    }

    fn is_dense_table(path: &[String]) -> bool {
        matches!(path, [state, section, _] if state == "state" && (section == "investment" || section == "unlocks"))
    }

    fn is_entitlements(path: &[String]) -> bool {
        matches!(path, [server, entitlements] if server == "server" && entitlements == "entitlements")
    }

    fn is_key_binding(path: &[String]) -> bool {
        matches!(path, [state, account, settings, bindings, _]
            if state == "state"
                && account == "account"
                && settings == "settings"
                && bindings == "key_bindings")
    }

    fn is_profile_items(path: &[String]) -> bool {
        matches!(path, [state, account, items]
            if state == "state" && account == "account" && items == "profile_items")
    }

    fn write_inline(value: &Value, spaces: bool, output: &mut String) -> Result<(), String> {
        let separator = if spaces { ", " } else { "," };
        match value {
            Value::Object(object) => {
                output.push('{');
                if spaces && !object.is_empty() {
                    output.push(' ');
                }
                for (index, (key, child)) in object.iter().enumerate() {
                    if index != 0 {
                        output.push_str(separator);
                    }
                    output.push_str(
                        &serde_json::to_string(key)
                            .map_err(|e| format!("Could not encode setting name: {e}"))?,
                    );
                    output.push_str(if spaces { ": " } else { ":" });
                    write_inline(child, spaces, output)?;
                }
                if spaces && !object.is_empty() {
                    output.push(' ');
                }
                output.push('}');
            }
            Value::Array(array) => {
                output.push('[');
                for (index, child) in array.iter().enumerate() {
                    if index != 0 {
                        output.push_str(separator);
                    }
                    write_inline(child, spaces, output)?;
                }
                output.push(']');
            }
            _ => output.push_str(
                &serde_json::to_string(value)
                    .map_err(|e| format!("Could not encode setting: {e}"))?,
            ),
        }
        Ok(())
    }

    fn write_dense_table(value: &Value, output: &mut String) -> Result<(), String> {
        let Value::Array(array) = value else {
            return write_inline(value, false, output);
        };
        output.push('[');
        for (index, child) in array.iter().enumerate() {
            if index != 0 {
                // Sunrise separates rows in its dense pair tables, while keeping each row compact.
                output.push_str(if child.is_array() { ", " } else { "," });
            }
            write_inline(child, false, output)?;
        }
        output.push(']');
        Ok(())
    }

    fn write_profile_items(
        array: &[Value],
        indent: usize,
        output: &mut String,
    ) -> Result<(), String> {
        output.push_str("[\n");
        for (index, child) in array.iter().enumerate() {
            let object = child
                .as_object()
                .ok_or("Sunrise profile_items entries must be objects")?;
            output.push_str(&" ".repeat(indent));
            output.push_str("{\n");
            for (field_index, (key, value)) in object.iter().enumerate() {
                output.push_str(&" ".repeat(indent));
                output.push_str(
                    &serde_json::to_string(key)
                        .map_err(|e| format!("Could not encode setting name: {e}"))?,
                );
                output.push_str(": ");
                write_inline(value, true, output)?;
                if field_index + 1 != object.len() {
                    output.push(',');
                }
                output.push('\n');
            }
            output.push_str(&" ".repeat(indent));
            output.push('}');
            if index + 1 != array.len() {
                output.push(',');
            }
            output.push('\n');
        }
        output.push_str(&" ".repeat(indent));
        output.push(']');
        Ok(())
    }

    fn write_value(
        value: &Value,
        indent: usize,
        path: &mut Vec<String>,
        legacy_profile_items: bool,
        output: &mut String,
    ) -> Result<(), String> {
        match value {
            Value::Object(_) if is_key_binding(path) => write_inline(value, true, output)?,
            Value::Object(object) if !object.is_empty() => {
                output.push_str("{\n");
                for (index, (key, child)) in object.iter().enumerate() {
                    output.push_str(&" ".repeat(indent + 2));
                    output.push_str(
                        &serde_json::to_string(key)
                            .map_err(|e| format!("Could not encode setting name: {e}"))?,
                    );
                    output.push_str(": ");
                    path.push(key.clone());
                    write_value(child, indent + 2, path, legacy_profile_items, output)?;
                    path.pop();
                    if index + 1 != object.len() {
                        output.push(',');
                    }
                    output.push('\n');
                }
                output.push_str(&" ".repeat(indent));
                output.push('}');
            }
            Value::Array(_) if is_dense_table(path) => write_dense_table(value, output)?,
            Value::Array(array)
                if legacy_profile_items
                    && is_profile_items(path)
                    && !array.is_empty()
                    && array.iter().all(Value::is_object) =>
            {
                write_profile_items(array, indent, output)?;
            }
            Value::Array(array) if array.iter().any(contains_object) => {
                output.push_str("[\n");
                for (index, child) in array.iter().enumerate() {
                    output.push_str(&" ".repeat(indent + 2));
                    if is_entitlements(path) {
                        write_inline(child, true, output)?;
                    } else {
                        write_value(child, indent + 2, path, legacy_profile_items, output)?;
                    }
                    if index + 1 != array.len() {
                        output.push(',');
                    }
                    output.push('\n');
                }
                output.push_str(&" ".repeat(indent));
                output.push(']');
            }
            Value::Array(array) => {
                let mut inline = String::new();
                write_inline(value, true, &mut inline)?;
                if array.is_empty() || current_column(output) + inline.len() <= MAX_INLINE_WIDTH {
                    output.push_str(&inline);
                } else {
                    output.push_str("[\n");
                    for (index, child) in array.iter().enumerate() {
                        output.push_str(&" ".repeat(indent + 2));
                        write_value(child, indent + 2, path, legacy_profile_items, output)?;
                        if index + 1 != array.len() {
                            output.push(',');
                        }
                        output.push('\n');
                    }
                    output.push_str(&" ".repeat(indent));
                    output.push(']');
                }
            }
            _ => output.push_str(
                &serde_json::to_string(value)
                    .map_err(|e| format!("Could not encode setting: {e}"))?,
            ),
        }
        Ok(())
    }

    let mut output = String::new();
    let legacy_profile_items =
        matches!(game_settings::schema_version(document), None | Some(0..=3));
    write_value(
        document,
        0,
        &mut Vec::new(),
        legacy_profile_items,
        &mut output,
    )?;
    Ok(output.replace('\n', "\r\n"))
}

pub(super) fn validate_document(document: &Value) -> Result<(), String> {
    game_settings::validate(document)?;
    progression::validate(document)?;
    validate_characters(document)?;
    inventory::validate_document_items(document).map_err(|error| error.to_string())
}

pub(super) fn validate_characters(document: &Value) -> Result<(), String> {
    const MAX_CHARACTERS: usize = 3;
    const MAX_PLUGS: usize = 12;
    const NO_DEFINITION_HASH: u64 = 0x811C_9DC5;
    let mode = inventory::schema_mode(document);

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
        for (key, label) in [
            ("accepted", "accepted state"),
            ("preview_available", "preview availability"),
            ("content_bypass", "content bypass state"),
        ] {
            if character.get(key).is_some_and(|value| !value.is_boolean()) {
                return Err(format!("Character {number} has an invalid {label}"));
            }
        }
        if character.get("appearance_value").is_some_and(|value| {
            value
                .as_f64()
                .is_none_or(|value| !(value as f32).is_finite())
        }) {
            return Err(format!(
                "Character {number} has an invalid appearance value"
            ));
        }
        if character
            .get("last_orbited_destination")
            .is_some_and(|value| {
                parse_unsigned_value(value).is_none_or(|value| value > u64::from(u32::MAX))
            })
        {
            return Err(format!(
                "Character {number} has an invalid last orbited destination"
            ));
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
            if !mode.is_future() && !SLOTS.iter().any(|(known, _, _)| known == slot) {
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
            if let Some(flags) = equipped.get("flags") {
                if !mode.supports_equipment_flags() {
                    return Err(format!(
                        "Character {number} {label} flags require settings schema {} or newer",
                        inventory::EQUIPMENT_FLAGS_SCHEMA_VERSION
                    ));
                }
                if parse_unsigned_value(flags)
                    .is_none_or(|flags| flags > u64::from(inventory::INVENTORY_FLAG_MASK))
                {
                    return Err(format!(
                        "Character {number} {label} flags must be between 0 and {}",
                        inventory::INVENTORY_FLAG_MASK
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

const fn shadowkeep_subclass_rules(subclass_hash: u64) -> Option<(&'static str, u64)> {
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
    paths::config_dir().map(|path| path.join("preferences.json"))
}

pub(super) fn backups_path() -> Option<PathBuf> {
    paths::data_dir().map(|path| path.join("backups"))
}

fn legacy_preferences_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        paths::data_dir().map(|path| path.join("paths.json"))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

pub(super) fn catalog_path() -> Option<PathBuf> {
    paths::cache_dir().map(|path| path.join("catalog").join("d2sk-86657.json"))
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

pub(super) fn load_preferences() -> Preferences {
    [preferences_path(), legacy_preferences_path()]
        .into_iter()
        .flatten()
        .find_map(|path| {
            fs::read_to_string(path)
                .ok()
                .and_then(|raw| serde_json::from_str(&raw).ok())
        })
        .unwrap_or_default()
}

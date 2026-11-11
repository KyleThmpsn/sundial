//! Appearance changes follow the existing document revision, transaction and recovery boundary.
use super::*;
use crate::character::Appearance;

impl SqliteAccountDocument {
    pub(crate) fn supports_appearance(&self) -> bool {
        self.schema.has_appearance()
    }

    pub(crate) fn appearance(&self, index: usize) -> Result<Option<Appearance>, String> {
        let character = self
            .characters
            .characters()
            .get(index)
            .ok_or("The selected character no longer exists")?;
        if let Some(value) = self.pending_appearances.get(&character.id) {
            return Ok(Some(value.clone()));
        }
        let native = self
            .character_persistence
            .get(&character.id)
            .ok_or("Missing character persistence metadata")?;
        let Some((race, gender)) = native.appearance_identity else {
            return Ok(None);
        };
        Appearance::from_saved(
            race,
            gender,
            native
                .appearance_header
                .ok_or("Missing native appearance header")?,
        )
        .map(Some)
    }

    pub(crate) fn set_appearance(
        &mut self,
        index: usize,
        value: Appearance,
    ) -> Result<bool, String> {
        if !self.supports_appearance() {
            return Err(
                "Appearance editing requires a Sunrise SQLite schema v3 or later account".into(),
            );
        }
        if self.appearance(index)?.as_ref() == Some(&value) {
            return Ok(false);
        }
        let character = self
            .characters
            .characters()
            .get(index)
            .ok_or("The selected character no longer exists")?;
        let id = character.id;
        let metadata = character
            .metadata
            .ok_or("The character's identity is unavailable")?;
        self.characters
            .apply(
                Self::character_capabilities(),
                sundial_account::CharacterCommand::UpdateMetadata {
                    character_id: id,
                    update: sundial_account::CharacterMetadataUpdate::SetAppearanceAndClass {
                        race: value.race,
                        gender: value.gender,
                        class_type: metadata.class_type,
                    },
                },
            )
            .map_err(|e| e.to_string())?;
        let native = &self.character_persistence[&id];
        if native.appearance_identity == Some((value.race, value.gender))
            && native.appearance_header.as_ref().map(|h| h.as_slice()) == Some(value.header())
        {
            self.pending_appearances.remove(&id);
        } else {
            self.pending_appearances.insert(id, value);
        }
        Ok(true)
    }

    pub(crate) fn appearance_summary(&self) -> serde_json::Value {
        let characters = (0..self.characters.characters().len())
            .map(|index| match self.appearance(index).ok().flatten() {
                Some(appearance) => serde_json::json!({
                    "race": appearance.race,
                    "gender": appearance.gender,
                    "header": appearance.header(),
                }),
                None => serde_json::Value::Null,
            })
            .collect();
        serde_json::Value::Array(characters)
    }

    pub(in crate::persistence::sqlite_account) fn appearance_for_write(
        &self,
        id: EntityId,
    ) -> Option<(&[u8], bool)> {
        if let Some(value) = self.pending_appearances.get(&id) {
            return Some((value.header(), true));
        }
        let native = self.character_persistence.get(&id)?;
        Some((
            native.appearance_header.as_ref()?.as_slice(),
            native.appearance_identity.is_some(),
        ))
    }
}

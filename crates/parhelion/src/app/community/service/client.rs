//! Bounded requests to the community service.
use super::{Catalog, Downloaded, ENDPOINT, Entry, MAX_RECIPE_BYTES, Submission};
use std::{io::Read, time::Duration};

#[derive(Clone)]
pub(crate) struct Client {
    agent: ureq::Agent,
    endpoint: String,
}

impl Client {
    pub fn new() -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(45)))
                .max_redirects(0)
                .https_only(true)
                .build()
                .into(),
            endpoint: ENDPOINT.into(),
        }
    }

    fn read(&self, path: &str, limit: usize) -> Result<Vec<u8>, String> {
        let mut response = self
            .agent
            .get(format!("{}/{path}", self.endpoint))
            .call()
            .map_err(|error| format!("Community request failed: {error}"))?;
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > limit {
            return Err("Community response exceeds the size limit".into());
        }
        Ok(bytes)
    }

    pub fn catalog(&self) -> Result<Catalog, String> {
        let catalog: Catalog = serde_json::from_slice(&self.read("catalog", 16 * 1024 * 1024)?)
            .map_err(|error| error.to_string())?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn download(&self, entry: &Entry) -> Result<Downloaded, String> {
        Catalog {
            schema: 1,
            recipes: vec![entry.clone()],
        }
        .validate()?;
        let bytes = self.read(
            &format!("{}?sha256={}", entry.download, entry.sha256),
            MAX_RECIPE_BYTES,
        )?;
        Downloaded::checked(entry.clone(), &bytes)
    }

    pub fn record_download(&self, entry: &Entry) -> Result<(), String> {
        Catalog {
            schema: 1,
            recipes: vec![entry.clone()],
        }
        .validate()?;
        let payload = serde_json::to_vec(&serde_json::json!({"sha256": entry.sha256}))
            .map_err(|error| error.to_string())?;
        self.agent
            .post(format!(
                "{}/recipes/{}/downloads",
                self.endpoint, entry.listing.id
            ))
            .header("Content-Type", "application/json")
            .send(payload.as_slice())
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn submit(&self, submission: &Submission) -> Result<String, String> {
        submission.listing.validate()?;
        submission
            .recipe
            .validate()
            .map_err(|error| error.to_string())?;
        let recipe = submission
            .recipe
            .to_json_pretty()
            .map_err(|error| error.to_string())?;
        if recipe.len() > MAX_RECIPE_BYTES {
            return Err("Recipe exceeds the upload size limit".into());
        }
        let payload = serde_json::to_vec(
            &serde_json::json!({"listing": submission.listing, "recipe": recipe}),
        )
        .map_err(|error| error.to_string())?;
        let mut response = self
            .agent
            .post(format!("{}/recipes", self.endpoint))
            .header("Content-Type", "application/json")
            .send(payload.as_slice())
            .map_err(|error| format!("Recipe upload failed: {error}"))?;
        let bytes = response
            .body_mut()
            .with_config()
            .limit(4096)
            .read_to_vec()
            .map_err(|error| error.to_string())?;
        let result: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        result
            .get("id")
            .and_then(|value| value.as_str())
            .map(str::to_owned)
            .ok_or_else(|| "The server did not return a submission reference".into())
    }
}

#[cfg(test)]
mod tests;

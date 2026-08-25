use std::time::Duration;

const NETWORK_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn get(url: &str, max_response_bytes: usize) -> Result<Vec<u8>, String> {
    let read_limit = u64::try_from(max_response_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(NETWORK_TIMEOUT))
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = agent
        .get(url)
        .header(
            "User-Agent",
            &format!("Sundial/{}", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .map_err(|error| format!("Request failed: {error}"))?;
    let response = response
        .body_mut()
        .with_config()
        .limit(read_limit)
        .read_to_vec()
        .map_err(|error| format!("Could not read the response: {error}"))?;
    if response.len() > max_response_bytes {
        return Err(format!(
            "The response exceeded the {max_response_bytes}-byte safety limit"
        ));
    }
    Ok(response)
}

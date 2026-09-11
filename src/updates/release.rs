//! Release metadata is pinned before downloading or showing release notes.
use serde::Deserialize;
use std::cmp::Ordering;

pub(super) const MAX_DOWNLOAD_BYTES: u64 = 256 * 1024 * 1024;
#[cfg(test)]
const DOWNLOAD_ROOT: &str = "https://github.com/kylethmpsn/sundial/releases/download/";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Release {
    pub version: String,
    pub notes: String,
    pub(super) asset: Result<Asset, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Asset {
    pub url: String,
    pub size: u64,
    pub digest: String,
    pub kind: ArchiveKind,
    pub member: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ArchiveKind {
    Zip,
    TarGz,
}

#[derive(Deserialize)]
struct Response {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

pub(super) fn parse(body: &[u8], current: &str) -> Result<Option<Release>, String> {
    let response: Response = serde_json::from_slice(body)
        .map_err(|error| format!("GitHub returned an invalid release response: {error}"))?;
    if response.draft || response.prerelease || !version_is_newer(current, &response.tag_name) {
        return Ok(None);
    }
    let asset = select_asset(&response);
    Ok(Some(Release {
        version: response.tag_name,
        notes: response
            .body
            .filter(|body| !body.trim().is_empty())
            .unwrap_or_else(|| "No release notes were provided for this version.".into()),
        asset,
    }))
}

fn select_asset(release: &Response) -> Result<Asset, String> {
    let (name, kind, member) = package(&release.tag_name)?;
    let matches: Vec<_> = release
        .assets
        .iter()
        .filter(|asset| asset.name == name)
        .collect();
    let [asset] = matches.as_slice() else {
        return Err(
            "This release does not provide a unique update download for this platform.".into(),
        );
    };
    let expected = format!("{}/{name}", release.tag_name);
    let valid_url = asset
        .browser_download_url
        .rsplit_once("/releases/download/")
        .is_some_and(|(repository, path)| {
            repository.eq_ignore_ascii_case("https://github.com/kylethmpsn/sundial")
                && path == expected
        });
    if !valid_url {
        return Err("The update download is not from the expected Sundial release.".into());
    }
    if asset.size == 0 || asset.size > MAX_DOWNLOAD_BYTES {
        return Err("The update download has an unsupported size.".into());
    }
    let digest = asset
        .digest
        .as_deref()
        .and_then(|value| value.strip_prefix("sha256:"))
        .filter(|value| valid_digest(value))
        .ok_or("This release has no SHA-256 checksum. In-app updating is unavailable for it.")?;
    Ok(Asset {
        url: asset.browser_download_url.clone(),
        size: asset.size,
        digest: digest.to_ascii_lowercase(),
        kind,
        member,
    })
}

pub(super) fn package(version: &str) -> Result<(String, ArchiveKind, String), String> {
    if !cfg!(target_arch = "x86_64") {
        return Err("In-app updating is not available for this CPU architecture.".into());
    }
    let version = version.strip_prefix('v').unwrap_or(version);
    let platform = if cfg!(windows) { "windows" } else { "linux" };
    let folder = format!("Sundial-v{version}-{platform}-x86_64");
    Ok(if cfg!(windows) {
        (
            format!("{folder}.zip"),
            ArchiveKind::Zip,
            format!("{folder}/sundial.exe"),
        )
    } else {
        (
            format!("{folder}.tar.gz"),
            ArchiveKind::TarGz,
            format!("{folder}/sundial"),
        )
    })
}

pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn version_is_newer(current: &str, candidate: &str) -> bool {
    compare_versions(candidate, current) == Some(Ordering::Greater)
}

pub(super) fn version_components(version: &str) -> Option<Vec<u64>> {
    let version = version.strip_prefix('v').unwrap_or(version);
    if version.is_empty() {
        return None;
    }
    version
        .split('.')
        .map(|part| {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                None
            } else {
                part.parse().ok()
            }
        })
        .collect()
}

pub(super) fn versions_match(left: &str, right: &str) -> bool {
    compare_versions(left, right) == Some(Ordering::Equal)
}

fn compare_versions(left: &str, right: &str) -> Option<Ordering> {
    let mut left = version_components(left)?;
    let mut right = version_components(right)?;
    let width = left.len().max(right.len());
    left.resize(width, 0);
    right.resize(width, 0);
    Some(left.cmp(&right))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn release_selection_pins_notes_asset_and_digest_and_rejects_unsafe_metadata() {
        let (name, _, _) = package("v0.5.0").unwrap();
        let original = json!({"tag_name":"v0.5.0", "body":"## Changes\n- Native updating",
            "assets":[{"name":name, "browser_download_url":format!("{DOWNLOAD_ROOT}v0.5.0/{name}"),
            "size":1024, "digest":format!("sha256:{}", "a".repeat(64))}]});
        let parse_value =
            |value: &serde_json::Value| parse(&serde_json::to_vec(value).unwrap(), "0.4.1");
        let release = parse_value(&original).unwrap().unwrap();
        assert_eq!(release.notes, "## Changes\n- Native updating");
        assert_eq!(release.asset.unwrap().size, 1024);
        for (key, value) in [
            (
                "browser_download_url",
                json!("https://example.com/sundial.exe"),
            ),
            ("digest", json!(null)),
            ("digest", json!("sha256:invalid")),
            ("size", json!(0)),
            ("size", json!(MAX_DOWNLOAD_BYTES + 1)),
        ] {
            let mut invalid = original.clone();
            invalid["assets"][0][key] = value;
            assert!(
                parse_value(&invalid).unwrap().unwrap().asset.is_err(),
                "{key}"
            );
        }
        let mut duplicate = original.clone();
        duplicate["assets"]
            .as_array_mut()
            .unwrap()
            .push(original["assets"][0].clone());
        assert!(parse_value(&duplicate).unwrap().unwrap().asset.is_err());
        for (key, value) in [
            ("draft", json!(true)),
            ("prerelease", json!(true)),
            ("tag_name", json!("v0.4.1")),
            ("tag_name", json!("../v0.5")),
        ] {
            let mut ignored = original.clone();
            ignored[key] = value;
            assert!(parse_value(&ignored).unwrap().is_none(), "{key}");
        }
        assert!(parse(b"{}", "0.4.1").is_err());
    }

    #[test]
    fn version_comparison_handles_existing_sundial_version_numbers() {
        for (current, next, newer) in [
            ("0.2", "v0.2.1", true),
            ("0.2.1", "v0.2.1.1", true),
            ("0.2.9", "v0.3", true),
            ("0.2.1", "v0.2.1", false),
            ("0.2.1", "v0.2", false),
            ("0.2.1", "not-a-version", false),
        ] {
            assert_eq!(
                version_is_newer(current, next),
                newer,
                "{current} -> {next}"
            );
        }
        assert!(versions_match("0.5.0", "v0.5"));
        assert!(versions_match("0.5.0", "v0.5.0.0"));
        assert!(!versions_match("0.5.1", "v0.5"));
        assert!(!versions_match("invalid", "invalid"));
    }
}

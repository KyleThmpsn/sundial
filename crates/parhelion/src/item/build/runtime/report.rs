//! Readable runtime summaries and optional resource-placement diagnostics.
use super::*;
use std::time::Duration;

pub(super) fn cache_miss(name: &str, reason: &cache::Reason) -> String {
    if matches!(reason, cache::Reason::CompilerChanged) {
        format!("Rebuilding cached resources for {name}: compiler or app version changed.")
    } else {
        format!("Building resources for {name}: {}.", plain(reason))
    }
}

pub(super) fn cache_not_saved(name: &str, reason: &cache::Reason) -> String {
    format!(
        "Could not cache resources for {name}: {}. The build can continue.",
        plain(reason)
    )
}

/// Why the saved resources were passed over, or weren't saved, as the build log says it. The
/// detailed runtime output keeps the tagged reason with its details.
fn plain(reason: &cache::Reason) -> String {
    use cache::Reason;
    #[cfg(feature = "d2-model-importer")]
    let file = |path: &std::path::Path| {
        path.file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned()
    };
    match reason {
        Reason::Disabled => "the runtime cache is turned off".into(),
        Reason::NoDirectory => "no cache folder is available".into(),
        Reason::Directory(_) => "the cache folder can't be used".into(),
        Reason::Locked(_) => "another build is using the cache".into(),
        Reason::Missing => "nothing is saved for it yet".into(),
        Reason::CompilerChanged => "the compiler or app version changed".into(),
        Reason::SourceChanged => "the game files or the packages this build reads changed".into(),
        Reason::RecipeChanged => "the recipe or a donor it uses changed".into(),
        Reason::IdentityChanged => "an older version saved them".into(),
        Reason::NativeChanged(tag) => format!("a game file it reads changed ({tag:08X})"),
        Reason::NativeUnavailable(tag) => format!("a game file it reads can't be read ({tag:08X})"),
        #[cfg(feature = "d2-model-importer")]
        Reason::ImportChanged(path) => format!("an imported file changed ({})", file(path)),
        #[cfg(feature = "d2-model-importer")]
        Reason::ImportUnavailable(path) => {
            format!("an imported file is missing or unreadable ({})", file(path))
        }
        Reason::Damaged(_) => "the saved copy is damaged".into(),
        Reason::UnsupportedRelocation(_) => {
            "this build places it elsewhere, and its saved resources only fit their first place"
                .into()
        }
        Reason::SharedAnimationChanged => {
            "it shares animations with the build's other items differently".into()
        }
        Reason::RelocationRejected => {
            "its saved resources can't be moved to where this build places it".into()
        }
        #[cfg(feature = "d2-model-importer")]
        Reason::UncacheableImport => "its imported files can't be fingerprinted".into(),
        Reason::UntrackedReads => "the game data it read couldn't all be tracked".into(),
        Reason::UnsupportedFragment => "its resources can't be saved as one piece".into(),
        Reason::TooLarge => "its resources are over the 512 MiB limit".into(),
        Reason::Write(_) => "the cache file couldn't be written".into(),
    }
}

pub(super) fn compiled(name: &str, compilation: Duration, cache_save: Option<Duration>) -> String {
    let mut message = format!(
        "Compiled resources for {name} in {:.2}s.",
        compilation.as_secs_f64()
    );
    if let Some(duration) = cache_save {
        message.push_str(&format!(
            " Saved to cache in {:.2}s.",
            duration.as_secs_f64()
        ));
    }
    message
}

/// The entries a weapon's runtime placed, as tag ranges, so the build log can attribute resident
/// bytes to items. A node another weapon placed first and this one shares counts for that weapon.
pub(super) fn placement(
    name: &str,
    allocator: AppendedTagAllocator,
    weapon: std::ops::Range<usize>,
    assets: &[TagHash],
) -> String {
    let mut tags = weapon
        .filter_map(|index| {
            allocator
                .assigned_tag(index, "Runtime placement", "weapon runtime")
                .ok()
        })
        .chain(assets.iter().copied())
        .map(|tag| tag.0)
        .collect::<Vec<_>>();
    tags.sort_unstable();
    tags.dedup();
    if tags.is_empty() {
        return format!("No runtime entries added for {name}.");
    }
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for tag in &tags {
        match ranges.last_mut() {
            Some((_, end)) if *end + 1 == *tag => *end = *tag,
            _ => ranges.push((*tag, *tag)),
        }
    }
    let ranges = ranges
        .iter()
        .map(|(first, last)| {
            if first == last {
                format!("{first:08X}")
            } else {
                format!("{first:08X}-{last:08X}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("Runtime entries for {name}: {} in {ranges}.", tags.len())
}

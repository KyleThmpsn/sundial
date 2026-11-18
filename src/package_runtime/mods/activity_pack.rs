//! The activity SDK pack a mod ships beside its manifest. Sunrise maps it at boot and resolves
//! the activities the mod adds through its rows.

pub const FILE_NAME: &str = "activity_sdk.pack";
/// A pack carries only its own maps' rows. The installed pack, which holds every stock scenario,
/// stays under 300 MiB.
pub const BYTE_LIMIT: u64 = 64 * 1024 * 1024;

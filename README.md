# Sundial

<img src="assets/sundial-alt.png" alt="Sundial logo" width="160">

Sundial is a character, inventory, and settings editor for
[Project Sunrise](https://github.com/stanuwu/Sunrise).
It edits `settings.json` and reads item names, icons, and stats from your installed
Destiny 2 packages. No game assets are bundled.

- Edit characters, equipment, subclasses, and inventories.
- Search weapons, armor, cosmetics, and perks.
- Randomize loadouts and adjust armor stats.
- Change game settings, progression, and Collections.
- Use the JSON editor for settings not covered by the guided interface.
- Create custom weapons with Parhelion.

## Compatibility

- Destiny 2 Shadowkeep build `86657.20.08.23`
- Project Sunrise (up to settings schema v16)
- Windows 10 or Linux x86-64 (glibc 2.35 or later)

Newer settings schemas may be opened with a warning, but future compatibility is not guaranteed.

## Getting started

1. Download the build for your platform from [Releases](https://github.com/kylethmpsn/sundial/releases).
2. Run `sundial.exe` on Windows or `sundial` on Linux.
3. Choose the Destiny 2 installation you use with Project Sunrise.

The first launch scans the game packages. Later launches use a cached catalog.
Sundial rebuilds it when the package set changes. You can also rebuild it under
**Preferences > Sunrise**

On Linux, the first package scan downloads a hash-verified decompression helper
(`liblinoodle3.so`). The Linux archive also includes an
optional `install.sh` for adding Sundial to your application launcher.

Close Destiny 2 before saving account changes, restoring backups, or installing
custom packages. Relaunch the game afterward to load your changes.

## Parhelion

Parhelion is an experimental custom investment global package builder bundled
with Sundial (basically, a weapons workbench). It builds custom weapons by combining
existing items, perks, stats, and appearance sources. Enable it under
**Preferences > Editing > Experimental**, then choose **Open Parhelion**.
Recipes can be saved and shared with others as JSON files.

Each install replaces the previous Parhelion package set, so include every recipe
you want to keep. Some combinations may not work and can freeze or crash the game.
Stock packages are not modified. Use **Uninstall Custom Packages…** to remove
Parhelion's installed package set. You can also remove its items and progression
from the selected account. Don't manually delete or mix individual package files.

See the [Parhelion README](crates/parhelion/README.md) for more about the authoring
workflow.

## Backups and local files

Sundial backs up settings before saving, checks for outside changes, and preserves
unknown JSON fields. Backups are kept separately for each installation. Open the
backup folder or reset settings under **Preferences > Saving & recovery**.

| Data | Windows | Linux |
| --- | --- | --- |
| Preferences | `%LOCALAPPDATA%\Sundial\preferences.json` | `${XDG_CONFIG_HOME:-~/.config}/sundial/preferences.json` |
| Backups | `%LOCALAPPDATA%\Sundial\backups` | `${XDG_DATA_HOME:-~/.local/share}/sundial/backups` |
| Catalog cache | `%LOCALAPPDATA%\Sundial\catalog` | `${XDG_CACHE_HOME:-~/.cache}/sundial/catalog` |
| Parhelion | `%LOCALAPPDATA%\Sundial\parhelion` | `${XDG_DATA_HOME:-~/.local/share}/sundial/parhelion` |
| Activity logs | `%LOCALAPPDATA%\Sundial\logs` | `${XDG_DATA_HOME:-~/.local/share}/sundial/logs` |

## Frequently asked questions

### Can I use Sundial with the current live version of Destiny 2?

No. Sundial supports the Project Sunrise Shadowkeep version listed under
**Compatibility**, not the current live game.

### Does Sundial change my loadout while Destiny 2 is running?

No. After saving changes, fully exit Destiny 2 to the desktop and relaunch it
for Project Sunrise to load them. Close the game before saving to avoid conflicts
with its own settings writes. For live in-game equipment editing, check out
[Sunrise Gear Editor](https://github.com/WalterGerig/SunriseGearEditor), a separate
project.

### What do the plug-safety levels do?

These control how broadly Sundial searches for plugs, not whether an experimental
combination is guaranteed to work:

| Mode | Choices shown |
| --- | --- |
| Compatible | Plugs listed as supported by the item. |
| Socket + gear type (default) | Plugs matching both the socket and the kind of gear. |
| Socket type | Plugs matching the socket, even if not supported by this item. |
| Gear type | Plugs found on the same broad kind of gear, regardless of socket. |
| All | Every discovered plug, regardless of compatibility. |

Broader choices can cause loading failures or crashes. Sundial warns before
enabling **All**. Backups are still created when saving experimental selections.

### Can I undo a change after saving?

Sundial saves a timestamped backup before writing changes. Use
**Preferences > Saving & recovery > Browse backups…** to find earlier copies for
your installation. Close Destiny before restoring one. Resetting to Sunrise
defaults also backs up your current settings first.

Settings backups and Parhelion package backups are separate. Restoring settings
does not undo an installed custom package set. See
[Parhelion recovery](crates/parhelion/README.md#backups-and-recovery).

### Why is the first launch slower?

Sundial builds its catalog from your existing game packages, then caches it.
It does not download a Destiny manifest or game assets. On Linux, the first scan
also downloads the verified decompression helper described above.

### What should I include when reporting a problem?

Describe the steps, Sundial/Sunrise versions, and any error message or screenshot.
A copy of your `settings.json` may help reproduce issues, but remove private
account details and server credentials before sharing it publicly.
For a custom weapon, include its recipe and explain what happens in-game.

Report bugs through [GitHub Issues](https://github.com/kylethmpsn/sundial/issues).
You can also reach me on Discord or Twitter/X as `kylethmpsn`.

## Building

Requires Rust 1.88 or later.

```sh
cargo build --release --locked -p sundial-suite
```

The executable is `target/release/sundial.exe` on Windows or
`target/release/sundial` on Linux.

## Credits and license

[tiger-pkg](https://github.com/v4nguard/tiger-pkg) provides the Destiny 2
package reader. This project would not be possible without it. Package-layout
research was also informed by Sunrise and Charm.

[Kjam's Panoptes fork](https://github.com/Kjam0678/panoptes/) inspired the
socket-grid layout and Randomize Loadout features. xSkullHD contributed the
original Random Item design and work on armor-stat targeting. Nox helped research
unnamed armor plugs and their stat allocations.

Solus created the Project Sunrise logo used in Parhelion's badge and inspired
its watermark.

Licensed under GPL-3.0-only. Third-party notices are included in release builds.
This project is not affiliated with Bungie Inc. or Sony Interactive Entertainment.
Destiny 2 and its related IP are property of Bungie Inc.
Built with AI assistance and human review.

Project Sunrise is a separate project. Please direct support for its development
to [stanuwu](https://github.com/stanuwu/Sunrise).

# [Sundial](https://github.com/kylethmpsn/sundial)

<img src="assets/sundial-alt.png" alt="Sundial logo" width="160">

Sundial is a quick, simple GUI for editing
[Project Sunrise](https://github.com/stanuwu/Sunrise)'s characters, loadouts,
and more in `settings.json`.

Edit any of the three default characters' weapons, subclass abilities, armor,
Ghost shells, Sparrows, ships, emblems, character properties, and more. You can
also adjust the plugs and perks installed on supported items, equip exotics in
every slot, or use the unsafe selection modes to experiment with unsupported
combinations. (Never forget the Craftening!)

Edit the player name and game defaults from the Game settings page. Project
Sunrise applies these settings when loading into Destiny 2, and Sundial limits
guided values to the accepted ranges.

Sundial reads item and ability data from your installed Shadowkeep packages to
provide searchable and browsable choices for each equipment slot, subclass
ability, and item socket. A small embedded table supplies names for armor plugs
that the game files leave generic. No full manifest database or Destiny assets
are bundled.

## Compatibility

Sundial is designed for:

- Project Sunrise 0.1 through 0.3.2 (schema v6), with settings support through
  schema v8, using Destiny 2 Shadowkeep build `86657.20.08.23`

Sundial handles the known differences between Sunrise settings schemas.
Newer schemas display a warning and may be opened with caution. Sundial keeps
recognized fields editable and preserves unrecognized JSON, but future
compatibility is not guaranteed.

## Features

- Guided character properties, subclasses, attunements, and attunement-aware
  ability choices
- Search and browse for equipment, plugs, and perks, with options to show dummy
  items or equip plugs not normally allowed on an item
- Character and profile (shared) inventory editing, including moving exact item
  instances between character inventory and equipment slots
- Remove equipped weapons completely (great for screenshots!)
- Automatic subclass, ability, and armor defaults when changing class
- Player name editing, named key-binding editing, and guided
  controls for supported game settings
- A straightforward JSON editor for anything not covered by the guided interface
- Preservation of unrecognized data, with warnings and extra safety copies for
  unexpected settings
- Automatic backups, version-matched Sunrise default restoration, and a locally
  cached catalog for faster startup

The catalog is stored at:

`%LOCALAPPDATA%\Sundial\catalog\d2sk-86657.json`

Sundial automatically rebuilds it after an app update or if the installed
package files change. You can also rebuild it manually from
**Preferences > Paths**.

## Usage

Download `sundial.exe` or `Sundial-v0.3.3-windows-x86_64.zip` from the GitHub
[Releases](https://github.com/kylethmpsn/sundial/releases) page and run it. Or,
build it yourself from source using the instructions below.

On first launch, select the root of the Destiny 2 installation you use for
Project Sunrise. Sundial only reads the game's package files. It writes the
selected Sunrise settings file and stores its own cache, preferences, and
backups under `%LOCALAPPDATA%\Sundial`.

Before each save, Sundial confirms the source file has not changed and creates
a timestamped backup. Unexpected files also receive a same-folder
`settings.json.bak` safety copy. Unrelated JSON fields are preserved.

## Building from source

Build the standalone executable with Rust/Cargo:

```powershell
cargo build --release
```

The executable is written to `target\release\sundial.exe`.

## Frequently asked questions

### Can I use Sundial with the current live version of Destiny 2?

No. Sundial is designed for Project Sunrise 0.1 through 0.3.2 (schema v6), with
settings support through schema v8, using Destiny 2 Shadowkeep build
`86657.20.08.23`. Select only the Shadowkeep installation used with Project
Sunrise.

### Does Sundial change my loadout while Destiny 2 is running?

No. After saving changes, fully exit Destiny 2 to the desktop and relaunch it
for Project Sunrise to load them.

### What do the unsafe plug-selection modes do?

Unsafe mode shows every plug matching the socket type. “Really unsafe” mode
allows any discovered plug in any socket, greatly increasing the risk of
loading failures or crashes. Sundial warns once before enabling it. Every save
is backed up, and **Preferences > Recovery** can recover the defaults.

### Why does Destiny 2 send me to character creation?

Sunrise may do this when a character contains an invalid or incompatible
configuration, especially a mismatched subclass, attunement, super, or melee
combination. Fully exit Destiny 2, open the file in Sundial, and save it again;
Sundial repairs the known ability pairings during save. If the problem remains,
reselect that character's class, subclass, and attunement before saving.

If all else fails, use **Preferences > Recovery** to restore the Sunrise
defaults. Earlier saves remain available under `%LOCALAPPDATA%\Sundial\backups`.

### Can I undo a change after saving?

Sundial creates a timestamped backup before every save. Backups are stored in
`%LOCALAPPDATA%\Sundial\backups`. Unexpected files also receive a
`settings.json.bak` beside the original.

### Why does the first launch take longer, and does Sundial download Destiny data?

Sundial scans your existing Shadowkeep packages to build a local catalog; it
does not download or include Destiny game data. Later launches use the cached
catalog unless the package files change or you rebuild it. Sundial includes a
small definition list for armor plugs whose manifest names are missing or
generic, including derived stat allocations.

### What should I do if I find a weird edge case?

Please send me a copy of the affected `settings.json` if you are comfortable
sharing it. There are many possible character and loadout combinations that
cannot all be anticipated, and a real example may help reproduce the problem
and fix it for future releases. You can reach me on Discord or Twitter/X at
`kylethmpsn`.

## Credits and licensing

[tiger-pkg](https://github.com/v4nguard/tiger-pkg) does most of the work required
to parse the packages from the locally installed game files. Package-layout
behavior was also informed by the Sunrise and Charm projects.

Thanks to Nox for his help in researching
[unnamed armor plugs](https://docs.google.com/spreadsheets/d/1U2DNRla6--q8PbU41QcqT2ku50hq5ew8uxy7r1tKe4c/edit).
Stat values were verified against Shadowkeep manifest
`86657.20.08.23.1800-9`; locally resolved game data takes priority.

Sundial was built with assistance from AI and reviewed by a real person. If you
are not comfortable with the use of AI in programming, you may want to avoid
this project.

Sundial is licensed under GPL-3.0-only. `tiger-pkg` is MIT-licensed. See
`THIRD_PARTY_NOTICES.md` in the release bundle for dependency notices. No
Bungie code, databases, or game assets are distributed with Sundial.

This project is not affiliated with or endorsed by Bungie Inc. or Sony
Interactive Entertainment. Destiny and related intellectual property are owned
by Bungie Inc. and their respective rights holders.

If you would like to support
[Project Sunrise](https://github.com/stanuwu/Sunrise), please direct that
support to stanuwu for their work on the project.

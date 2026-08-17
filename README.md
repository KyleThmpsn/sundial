# [Sundial](https://github.com/kylethmpsn/sundial)

<img src="assets/sundial-alt.png" alt="Sundial logo" width="160">

Sundial is a quick, simple GUI for editing
[Project Sunrise](https://github.com/stanuwu/Sunrise)'s characters, loadouts,
and more in `settings.json`.

Edit any of the three default characters' weapons, subclass abilities, armor,
Ghost shells, Sparrows, ships, emblems, character properties, and more. You can
also adjust the plugs and perks installed on supported items... or equip exotics
in every slot, if you want! Unsafe plug selection can show every plug matching
a socket type, while the “really unsafe” tier makes every discovered plug
available in every socket. Unsupported combinations may break items, corrupt
the loadout, or crash Destiny 2. (Never forget the Craftening!)

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

- Project Sunrise 0.1 through 0.3.1 with Destiny 2 Shadowkeep
  build `86657.20.08.23`

Sundial handles the known differences between Sunrise settings schemas.
Newer schemas display a warning and may be opened with caution, but future
compatibility is not guaranteed.

Named key-binding editing is experimental. Some Sunrise configurations may use
`cvars.xml` instead, so changes might not take effect.

## Features

- Guided character properties, subclasses, attunements, and attunement-aware
  ability choices
- Search and browse for equipment, plugs, and perks, with options to show dummy
  items or equip plugs not normally allowed on an item
- Character and profile (shared) inventory editing
- Remove equipped weapons completely (great for screenshots!)
- Automatic subclass, ability, and armor defaults when changing class
- Player name editing, initial support for named key-binding editing, and guided
  controls for supported game settings
- A straightforward JSON editor for anything not covered by the guided interface
- Preservation of unrecognized data, with warnings and extra safety copies for
  unexpected settings
- Automatic backups, version-matched Sunrise default restoration, and a locally
  cached catalog for faster startup

The catalog is stored at:

`%LOCALAPPDATA%\Sundial\catalog\d2sk-86657.json`

Sundial automatically rebuilds it after an app update or if the installed
package files change. You can also rebuild it manually from the **Paths** screen.

## Usage

Download `sundial.exe` or `Sundial-v0.3.0-windows-x86_64.zip` from the GitHub
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

No. Sundial is designed for Project Sunrise 0.1 through 0.3.1 with Destiny 2
Shadowkeep build `86657.20.08.23`. Select only the Shadowkeep installation used
with Project Sunrise.

### Does Sundial change my loadout while Destiny 2 is running?

No. After saving changes, fully exit Destiny 2 to the desktop and relaunch it
for Project Sunrise to load them.

### What do the unsafe plug-selection modes do?

Unsafe mode shows every plug matching the socket type. “Really unsafe” mode
allows any discovered plug in any socket, greatly increasing the risk of
loading failures or crashes. Sundial warns once before enabling it. Every save
is backed up, and **Paths > Restore Sunrise defaults** can recover the defaults.

### Why does Destiny 2 send me to character creation?

Sunrise may do this when a character contains an invalid or incompatible
configuration, especially a mismatched subclass, attunement, super, or melee
combination. Fully exit Destiny 2, open the file in Sundial, and save it again;
Sundial repairs the known ability pairings during save. If the problem remains,
reselect that character's class, subclass, and attunement before saving.

If all else fails, use **Paths > Restore Sunrise defaults**. Sundial preserves
the current file as `settings.json.bak` and as a timestamped backup before
restoring it. Earlier save backups are also available under
`%LOCALAPPDATA%\Sundial\backups`.

### Can I undo a change after saving?

Sundial creates a timestamped backup before every save. Backups are stored in
`%LOCALAPPDATA%\Sundial\backups`. Unexpected files also receive a
`settings.json.bak` beside the original.

### Why does an ornament show as equipped but not appear in game?

Sundial can equip an ornament plug correctly even when Sunrise shows the base
item model in game. Sunrise 0.2.1 applies shader and material overrides from
plugs, but does not apply every ornament's model replacement. This is why
shaders generally work while an ornament may appear equipped in menus without
changing the in-game model.

### Why does the character preview show a female Awoken or incorrect equipment?

Sunrise may use a default female Awoken for character previews, regardless of
the saved race or gender. Equipment can also render incorrectly in previews but
should appear correctly in game. Some character changes can leave the
character-select preview appearing to load even though the in-game model still
reflects them.

### Why does the first launch take longer?

Sundial scans the installed Shadowkeep packages to build its local catalog.
Later launches load that catalog from the cache unless the package files change
or you choose to rebuild it.

### Does Sundial download or include Destiny game data?

No. Item and ability data is read from your existing Shadowkeep installation.
The resulting catalog is cached locally on your PC. Sundial includes a small
definition list for armor plugs whose manifest names are missing or generic,
including derived stat allocations.

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

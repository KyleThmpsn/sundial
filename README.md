# [Sundial](https://github.com/kylethmpsn/sundial)

<img src="assets/sundial.png" alt="Sundial logo" width="160">

Sundial is a quick, simple GUI for editing
[Project Sunrise](https://github.com/stanuwu/Sunrise)'s characters, loadouts,
and more in `settings.json`.

Edit any of the three default characters' weapons, subclass abilities, armor,
Ghost shells, Sparrows, ships, emblems, character properties, and more. You can
also adjust the plugs and perks installed on supported items... or equip exotics
in every slot, if you want! Enable unsafe plug selection to show every plug
matching a socket type, but be aware that unsupported combinations may break
items, corrupt a loadout, or crash Destiny 2. (Never forget the Craftening!)

Edit the default controls, audio, display, interface, and social settings from a
dedicated Game settings page. Project Sunrise applies these settings when
loading into Destiny 2, and Sundial limits values to the accepted ranges.

Sundial reads the Destiny 2 Shadowkeep packages already installed on your PC to
provide searchable and browsable choices for each equipment slot, subclass
ability, and item socket. Item and ability names come directly from those local
game files. Sundial does not bundle a manifest database or any Destiny assets.

## Compatibility

Sundial is designed for:

- Project Sunrise 0.1 (Destiny 2 Shadowkeep build `86657.20.08.23`)

Support for future Project Sunrise releases is yet to be determined. Changes to
the settings schema are likely to break compatibility.

## Features

- Named controls for character class, race, gender, subclass, and attunement
- Attunement-aware super, melee, and perk choices
- Search and browse for valid equipment in each slot
- Display-only dummy items hidden by default, with an option to show them
- Plug and perk selection for supported item sockets
- Automatic subclass, ability, and armor defaults when changing class
- Guided controls for supported game settings
- Simple JSON editor for settings not covered by the guided interface
- Validation before saving
- Automatic timestamped backups in `%LOCALAPPDATA%\Sundial\backups`
- A locally cached package catalog for faster startup after the first scan

The catalog is stored at:

`%LOCALAPPDATA%\Sundial\catalog\d2sk-86657.json`

Sundial automatically rebuilds if the installed package files change. You
can also rebuild it manually from the **Paths** screen.

## Usage

Download `Sundial.exe` from the GitHub
[Releases](https://github.com/kylethmpsn/sundial/releases) page and run it. Or,
build it yourself from source using the instructions below.

On first launch, select the root of the Destiny 2 installation you use for
Project Sunrise. Sundial only reads the game's package files. It writes the
selected Sunrise settings file and stores its own cache, preferences, and
backups under `%LOCALAPPDATA%\Sundial`.

Before each save, Sundial validates the document and creates a backup. Unrelated
JSON fields are preserved, and arrays are encoded compactly to keep the file
below the 64 KiB settings limit.

## Building from source

Build and run the release version with Rust/Cargo:

```powershell
cargo run --release --bin sundial
```

## Frequently asked questions

### Does Sundial change my loadout while Destiny 2 is running?

No. After saving changes, fully exit Destiny 2 to the desktop and relaunch it
for Project Sunrise to load them.

### Can I use Sundial with the current live version of Destiny 2?

No. Sundial is designed for Project Sunrise 0.1 and Destiny 2 Shadowkeep build
`86657.20.08.23`. Select only the Shadowkeep installation used with Project
Sunrise.

### Why does the first launch take longer?

Sundial scans the installed Shadowkeep packages to build its local catalog.
Later launches load that catalog from the cache unless the package files change
or you choose to rebuild it.

### Does Sundial download or include Destiny game data?

No. Item and ability data is read from your existing Shadowkeep installation.
The resulting catalog is cached locally on your PC.

### What does unsafe plug selection do?

It shows every discovered plug matching a socket type instead of only the
normally supported choices. Unsupported combinations may break an item, corrupt
a loadout, or crash Destiny 2.

### Can I undo a change after saving?

Sundial creates a timestamped backup before every save. Backups are stored in
`%LOCALAPPDATA%\Sundial\backups`.

## Credits and licensing

[tiger-pkg](https://github.com/v4nguard/tiger-pkg) does most of the work required
to parse the packages from the locally installed game files. Package-layout
behavior was also informed by the Sunrise and Charm projects.

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

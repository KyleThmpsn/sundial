Sundial v0.5 adds Dawn support, expands account and progression editing, and delivers a major update to Parhelion's experimental weapon workbench.

Read the [Parhelion README](crates/parhelion/README.md) for details about the Custom Perk Workbench, new weapon crafting features, bundled examples, and how to build and install your creations.

## Parhelion

- Enabled the experimental **Custom Perk Workbench** for creating, editing, saving, and reusing private perks.
  - Includes guided forms, a node canvas, and stock behavior examples for creating your own perks with existing engine behavior.
  - Craft your own perks by combining actions, conditions, projectiles, and more.
  - Several bundled example perks are included for reference.
  - Custom perk authoring is still in early development. Destiny's perk system contains thousands of technical building blocks across actions, conditions, values, projectiles, assets, and weapon-specific behavior. Those pieces can be arranged in effectively countless combinations. Parhelion maps that technical data into controls you can use, but the work is ongoing. More behavior will be added as it is understood, and some names, assumptions, or mappings may turn out to be incomplete or wrong.
- Added **Unique Weapon Behavior** for borrowing an Exotic weapon's runtime behavior, with the option to include its intrinsic and trait perks.
  - Many Exotics only store half of their behavior in perks. The other half is built into the weapon's runtime behavior.
  - This allows combinations that were not previously possible, such as Malfeasance's rounds on other weapons, Hard Light's variable damage types, Leviathan's Breath on other bows, and much more.
  - Some behavior still depends on certain weapon types, so not everything will work. Some combinations may only partially work, such as Coldheart's projectile not appearing even though it still causes damage.
- Added **Variable (Hold Reload)** damage switching for compatible borrowed behaviors, including Hard Light, Borealis, and Two-Tailed Fox.
- Added an ornament selection option for weapon appearance, allowing you to use an ornament's model as the weapon's default model.
  - The icon editor attempts to remove the ornament overlay backdrop as cleanly as possible, but that layer is baked into the asset.
- Drag-and-drop is now supported for perk choices within and between sockets.
- Added a basic artwork editor for custom badges and watermarks.
- Added an icon browser for custom perks, badges, and watermarks.
  - Choose from icons found in the installed game packages, import your own, or optionally download [destiny-icons](https://github.com/justrealmilk/destiny-icons) for more choices.
- Added Dawn support for Parhelion.
  - Future online-supported versions of Dawn may require custom packages to be uninstalled or may reject them.
- Reduced repeat scan times for installed packages and perks.

### Fixes

- Fixed custom Lore tabs overwriting the authored weapon's Collections item link, resulting in strange entries such as a Ghost Mod instead of the authored weapon.
- Fixed Khvostov, Horror Story, Rose, Leviathan's Breath, and other valid weapons failing to build when their stock Collections entries used unusual acquisition conditions.
- Fixed authored weapons inheriting unexpected Collections unlock behavior from their donors.

## Sundial

- Added support for Dawn `player-state.db` schema v5, including characters, inventory, equipment, player preferences, key bindings, unlocks, and progression.
- Added Dawn tools for the Postmaster, saved randomized rolls, dismantle rewards, vendors, missions, and supported queued currency rewards.
- Added Dawn-specific Game Settings, including the **Omega Lua Executor**, **Directive UI**, **Private Regions**, **Hold Spawn During Loading**, and Spawn Hold timing.
  - Sundial shows controls for the active runtime and reports the runtime, account source, database, and installation paths in use.
- Added Triumph editing and expanded Sunrise schema v18 Seasonal tools with Season Pass completion, pending rewards, and consumable grants.
- Improved Collections editing with bulk acquisition and better handling of the unlock conditions required by each node.
- Added runtime selection and recovery when multiple runtime DLL copies are present, plus account database reset and safer backups.
- Expanded the Definition Inspector with more links between items, progression, rewards, stats, Power caps, sockets, missions, and vendors.

### Fixes

- Fixed Destiny symbols in picker rows using the wrong font even though their tooltips rendered correctly.
- Updated dummy-item classifications and the class-restricted item list.
- Fixed unnamed armor stat allocations depending on their derived names instead of the game packages.

Sundial v0.4.1 can update to v0.5 in the app. Older releases should install v0.5 manually.

Parhelion remains experimental, and not every weapon or perk combination has been tested. Report issues on [GitHub](https://github.com/KyleThmpsn/sundial/issues), on Discord, or on Twitter/X [@KyleThmpsn](https://x.com/KyleThmpsn). For Parhelion issues, include the recipe and describe what happens in game.

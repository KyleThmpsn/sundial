# Sundial v0.4.0

This release introduces Parhelion, an experimental custom investment global package builder bundled with Sundial. It's a weapon workbench that builds installable game packages from your custom weapon recipes. This release also expands Sunrise settings support and improves Sundial's inventory and editing tools.

## Parhelion

- Build your own weapons by mixing the behavior, appearance, and perks of existing weapons, including Exotics. Start with a base weapon, choose the model you want, and customize its name, stats, rarity, slot, ammo type, damage type, sockets, and perk options to create combinations that wouldn't normally exist in the sandbox.
- Try the 15 bundled weapons or make your own recipes, which you can save, duplicate, and share as JSON files.
- Remove base sockets without shifting neighboring sockets, or restore their original choices and roles. Added sockets can be removed from last to first.
- Changing an Exotic base weapon to a non-exotic rarity clears its exotic equip restriction. Choosing Exotic rarity applies the normal exotic weapon restriction.
- Add shaders that recolor supported dye channels, including channels normally locked by an Exotic appearance.
- Customize or import item artwork, including ammo HUD icons. Imported artwork is included when sharing a recipe.
  - Custom model importing is not included in this release but is being explored for a future release.
- Build and install your weapons with built-in backups and uninstall controls. Installed weapons appear in Collections and the new Project Sunrise badge, with Exotics under their own Collections category.
  - Thanks to [Solus](https://www.youtube.com/@Solus-yt) for creating the Project Sunrise logo used for the badge!
- Includes a custom-authored perk, Micro-Missile Frame, an intrinsic version of Micro-Missile for experimenting with additional weapon types.
  - Work is underway to map out perk behavior and add the ability to create your own custom perks, which will be included in a future release.

Enable Parhelion under **Preferences > Editing > Experimental**, then choose **Open Parhelion**. See the [Parhelion README](https://github.com/KyleThmpsn/sundial/blob/main/crates/parhelion/README.md) for instructions on creating, sharing, installing, and removing weapons.

Parhelion is experimental. Some weapon and perk combinations may not work or can freeze or crash the game.

## Sundial

- Added support for Sunrise settings schema v16, including controls for completing Exotic catalysts, revealing lore books, configuring the in-game Sunrise menu and emote wheel, and editing activity, entitlement, and character settings. These controls require schema v16 or newer. Older settings formats remain supported.
- Added a Socket + Gear Type plug-selection mode, which matches both the socket and the broad type of equipment. This is now the default.
- Improved inventory browsing with filtering, sorting, lock filtering, and faster loading of item cards and icons. Inventory actions now sit beside Plugs and Dummy Items without repeating the selected character label.
- Improved loadout randomization and armor stat adjustment, including preserving locked items when randomizing.
- Expanded the Definition Inspector with more weapon and perk information, direct unlock references, evaluated progression and counter values, and navigation between related records. Corrected condition-expression evaluation and progression rank costs.
- Made Collections browsing and progression inspection available by default. Changing progression state and Investment overrides still requires enabling experimental Progression Editing.
- Laid the groundwork for SQLite persistent accounts. Support will be added in a future release.
- Added an experimental option to equip subclasses from other classes on a character. This does not require a newer settings schema.
- Added an experimental option to extend Field of View up to 155 degrees on Sunrise schema v16 or newer. Standard Field of View editing requires schema v8 or newer and normally stops at 105 degrees. Sunrise 0.3.2 uses schema v6 and does not support this setting.
- Added undo and redo, plus an optional review of changes before saving.
- Expanded the JSON editor with find and replace, formatting, navigation to settings and errors, and the ability to add missing settings from installed defaults.

If you have suggestions or run into any issues, reach out [here on GitHub](https://github.com/KyleThmpsn/sundial/issues), on Discord, or on Twitter/X [@KyleThmpsn](https://x.com/KyleThmpsn). For Parhelion issues, please include the recipe and describe what happens in-game.

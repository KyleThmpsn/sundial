Sundial v0.4.1 focuses on support for Sunrise 0.4.0 settings schema v18+ and persistent accounts, alongside bug fixes and small feature additions for Parhelion.

## Parhelion

- Added custom Collections badges, seasonal watermarks, and weapon lore.
  - Artwork is automatically included in shared recipes.
- Added Collections page selection for non-Exotic weapons, including adding pages for weapon type combinations not in the base game, such as Sidearms under Special.
  - Collections is limited to 1,024 nodes, with 96 available for custom content. Each new page uses one node, and each custom badge uses four.
  - The remaining node budget is shown while editing.
- Improved the recipe library manager with bulk exports and the option to restore individual bundled recipes to their defaults.
- Added **Make Default** to the right-click menu for plug choices.
- Improved build progress and error messages.
- Fixed a build issue where Parhelion rejected valid Seventh Seraph and Trials weapon icons.
- Extended coverage for weapon combinations, fixing cases where models did not display in game.
  - Thousands of combinations are possible, so some model issues may remain. If you find one, please report it and include your recipe.

## Sundial

- Added support for **Sunrise 0.4.0 settings schema v18+ (persistent account SQLite schema v2)**. Legacy JSON accounts remain supported.
  - Inventory, equipment, account preferences, key bindings, unlocks, and progression editing now use the persistent account.
  - Added character level, title, ability unlock, character material, and pending reward editing.
  - Added **Seasonal XP**, **Artifact Mods**, and **Season Pass** views, including artifact reset, reward browsing, and claim status.
  - Added **Seen** controls for equipped, stored, and shared items.
- Mods now display descriptions in their tooltips and in pickers.
- Improved item menus across inventory layouts, character controls, key binding selection, and tooltips.
- Reorganized Sunrise settings into focused tabs. Some settings are advanced and should only be changed if you understand what they do.
- Cached and shared native asset indexes to reduce repeated scans across tools and app launches.
- Added **in-app updates** with release notes, download progress, restart, and automatic recovery if the updated application cannot start.
- Fixed a crash that could sometimes occur when editing characters with out-of-range values in legacy JSON accounts.
- Other minor fixes and improvements.

Install v0.4.1 manually when upgrading from v0.4. In-app updating is available for subsequent releases.

Report issues on [GitHub](https://github.com/KyleThmpsn/sundial/issues), on Discord, or on Twitter/X [@KyleThmpsn](https://x.com/KyleThmpsn). For Parhelion issues, include the recipe and describe what happens in game.

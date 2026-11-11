Sundial v0.6.1 adds character appearance editing and support for Sunrise settings v19 and SQLite investment schema v5. Parhelion builds are smaller and faster, and large recipe libraries are more responsive.

Read the [Parhelion README](crates/parhelion/README.md) for the workbench, bundled examples, and build and install instructions. Find previous release notes on the [releases page](https://github.com/KyleThmpsn/sundial/releases).

## Parhelion

- Builds reuse identical assets and custom perks to save disk space, with faster game-file checks and package preparation.
- **Perks & Sockets** has two views. **Reusable** lists perks you can swap to, and **Randomized** lists every possible roll, including reusable perks.
  - Right-click a perk and choose **Make Reusable** or **Remove from Reusable** to change whether it can be swapped to.
  - In **Randomized**, greyed-out base perks can be added with **Add to Randomized Plug Set**. Each socket supports up to 64 choices, and the button shows the remaining space.
- Renamed the custom perk action **Hold a Weapon Count** to **Enable Tracking**. Use it with **Change Fired Projectile** when the new projectile should track a target.
- Renamed **Set a Weapon Firing Mode** to **Override Animation Input** to describe what the action changes.
- Custom perk projectile cards now show tracking settings such as **Turn Rate**, **Lead Time Limit** and **Target Lead**.
- A custom perk effect can hold up to 64 actions, up from 16.
- **Ability Property** offers more stock modifiers, including **Sentinel Healing Shield** and **Extended Lunge Range**. Descriptions explain the settings and flag behavior that needs testing.
- Custom perk actions now show their saved property changes, amounts and dynamic inputs. Trigger overrides read **Hold to Fire** or **Hold to Charge**.
- **Custom Perks** and **Select Custom Perk** hide ornaments unless you check **Show Ornaments**. Both windows share this setting.
- The bottom bar shows background tasks. Hover to see all tasks, or click to open their status.
- Build progress uses clearer units and gives longer stages more of the progress bar. Progress is preserved across build passes.
- The activity log retains 100,000 events. **Show Detailed Runtime Output** reveals cache diagnostics, timings and entry ranges, and includes them in **Copy Progress**.
- **Review Installation** shows added subclasses and shaders, which characters will equip them, ability resets and Collections unlock changes.
- Authored items count toward each Collections page's total and acquired count on Dawn and Sunrise without using unlock slots. The Project Sunrise badge shows as complete. Replacing an older build clears its retired authored unlocks.
- The **Badges** page puts Project Sunrise first and sorts custom badges by name.
- Renaming a recipe warns if it creates a new item identity. Installing the renamed item removes copies of the previous item from your account.
- **Build & Stage** can skip failing recipes, list their errors and build the remaining selection using completed preparation work. Installing a partial build removes copies of skipped items from your account.
- Item names can be written entirely in other scripts, such as Japanese.
- Press Ctrl+S to save the open recipe.
- Added **Type**, **Damage**, **Ammo** and **Rarity** filters to the **Recipe Library** and **Items in This Build**. Hidden selections still build, and the list shows their count.

### Fixes

- Fixed saved damage sockets restoring a weapon's previous element after changing **Damage Type**.
- Fixed subclass abilities disappearing from the catalog after mod installs, which could also block builds using subclass conditions.
- Fixed installs equipping a subclass you had switched away from and resetting its ability choices. Only newly added subclasses are equipped automatically.
- Fixed **Build & Stage** saving or rejecting an open recipe that was excluded from the selection, preserving its unsaved edits.
- Fixed incorrect stock objective progress after builds added Collections pages or the Project Sunrise badge.
- Fixed builds rejecting certain valid private perks and component combinations.
- Fixed some Sunrise Studio mods being omitted from catalogs and installation review. Mods from older Studio releases still load, and Parhelion's own mod still works with older Sunrise releases.
- Fixed unchanged weapons recompiling on large builds.
- Large builds use additional packages when item records, artwork or private effects reach capacity.
- Parhelion opens its loading screen immediately and loads recipes and perks in the background, reducing freezes during startup, saves and browsing large libraries.
- Reduced pauses while typing or dragging custom perk controls. Drafts still save during editing and when Parhelion closes.
- Fixed valid saved recipes being hidden or blocking other saves when their cached listings are damaged or belong to another library.
- Model previews prepare in the background, preventing freezes when a model is first shown.
- On Windows, previously viewed models reopen faster, including after restarting Sundial.
- On Windows, model previews report stalled compilation and handle compiler failures more reliably.
- Fixed animated model previews turning some parts to the wrong angle, such as a machine gun's feed cover and ammo belt.
- Fixed valid weapon edits being rejected and **Barrel Settings** failing to load after installing a large build on Dawn or older Sunrise versions.
- Fixed brief freezes after opening a Sparrow recipe with a vehicle or projectile chosen through **Choose Other Vehicle…** or **Choose Other Projectile…**.
- Fixed brief freezes while **Custom Perks** resolves asset names or first opens its projectile picker.
- Fixed perks from saved items appearing twice in **Custom Perks**. Opening the window removes the extra copies.
- Perks can now be dragged into empty sockets.
- Random-roll sockets warn immediately when they have no default perk.
- Fixed shader **Preview** showing weapons when editing another gear type's dyes. It now lists items of that gear type, while **All Gear** lists every type.
- Collections errors now say which weapon recipe failed and which base or appearance item is missing.
- Fixed installs failing with "Access is denied" when another Sundial or Parhelion window held a package file open.
- Fixed removed items appearing under **Invalid Items** after returning from Parhelion.
- Fixed model previews failing with "Resource … has an invalid size" after installing a build that adds item artwork.
- Package backups on Dawn and older Sunrise follow the configured limit, while retaining recovery backups from installs that changed account or settings data.
- Recipe backups keep the configured number of recent copies for each recipe.

## Sundial

- Added **Appearance** for editing faces, hair, markings and colors, with native thumbnails, portrait and full-body previews, Save and undo.
  - Requires Sunrise 0.6 or later with a SQLite investment schema v3 or v5 account. Sundial still opens schema v2 accounts without migrating them.
  - Previews include native skin and lip colors, hair and brows, layered markings and colored Exo eye lights.
  - **Portrait** and **Full Body** reuse loaded previews. An info icon opens preview help.
- Sundial and Parhelion installs support Sunrise settings v19 and SQLite investment schema v5 alongside earlier versions. Sundial preserves items that Sunrise sets aside while their content is missing.
  - **Export Package Keys** works with v19. The **Advanced** tab also adds **Export Tag Definitions** for Sunrise Studio.
  - On v19, an external server or a bind address other than `127.0.0.1` requires **Allow Multiplayer** in **Networking**.
- If `settings.json` fails to load, **Restore Settings Backup…** restores a chosen backup and preserves the current file first.

### Fixes

- Closing Sundial while **Review Changes Before Saving and Exiting** is open keeps the review and your edits intact.
- Sundial rechecks the equipped weapon before deleting and asks for review if its slot changed.
- Switching installations reloads current settings and asks for review if they use a newer schema.
- Improved performance when saving large SQLite inventories, applying bulk Collections changes or inspecting account progression.
- Updated bundled SQLite to address a security vulnerability in database queries.
- Fixed settings saves on Dawn failing with an account source error when player-state.db is missing or can't be read.
- Fixed **Reset Settings to Dawn Defaults** removing the account data kept in settings.json. Its confirmation now names Dawn and player-state.db.
- Fixed **Cancel Download** waiting on a stalled connection.

Sundial v0.4.1 and newer can update to v0.6.1 in the app. Older releases should install v0.6.1 manually.

Parhelion remains experimental, and some item and perk combinations may crash. Report issues on [GitHub](https://github.com/KyleThmpsn/sundial/issues), or contact kylethmpsn on Discord or [Twitter/X](https://x.com/kylethmpsn). Include the recipe and describe what happens in game when reporting a Parhelion issue.

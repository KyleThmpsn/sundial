Sundial v0.6.1 adds character appearance editing and support for Sunrise settings v19 and SQLite investment schemas v5, v7 and v8. Parhelion adds Artifact recipes and Build Lists, and its builds are smaller and faster.

Read the [Parhelion README](crates/parhelion/README.md) for the workbench, bundled examples, and build and install instructions. Find previous release notes on the [releases page](https://github.com/KyleThmpsn/sundial/releases).

## Parhelion

### Items and Perks

- Added **Artifact** recipes for Sunrise with their own Collections page. **Class** controls who can equip an artifact and where it appears in the Project Sunrise badge. Bases without perk sockets offer supported shared sockets. Dawn builds leave artifacts out because Dawn has no artifact slot.
- The bundled Every End recipe is now Exotic and has its own perk, **Dying of the Light**, replacing Impetus and Auto-Loading Holster. Final blows temporarily ignite the weapon, letting it fire Hammers of Sol at a slower rate. The HUD shows the time left, and further final blows extend it. The perk is also added to **Custom Perks**.
- Added scout rifle REASSIGNED PURPOSE~ with **Hostile Takeover**. A precision final blow opens a 10-second window in which precision hits may temporarily turn targets into allies. It uses a native effect that Shadowkeep retains from Destiny 1's Memory of Timur. The perk is also added to **Custom Perks**.
- **Perks & Sockets** has two views. **Reusable** lists perks you can swap to, and **Randomized** lists all perks that can roll in each socket, including reusable perks.
  - Right-click a perk and choose **Make Reusable** or **Remove from Reusable** to change whether it can be swapped to.
  - In **Randomized**, greyed-out base perks can be added with **Add to Randomized Plug Set**. Each socket supports up to 64 choices, and the button shows the remaining space.
  - Perks can be dragged into empty sockets, and random-roll sockets warn immediately when they have no default perk.
- Item names can be written entirely in other scripts, such as Japanese.
- The icon editor's **Image** tab adds **Render from Model** for weapons, using the current ornament and shader to create the icon.

### Custom Perks

- A custom perk effect can hold up to 64 actions, up from 16.
- The workbench offers native effects and conditions that no stock perk uses, including **Assign Target to Owner's Team**, **Create Entity for Owner** and **Apply Program to Ability Slot**. Most are untested in game.
- Projectile cards show tracking settings such as **Turn Rate**, **Lead Time Limit** and **Target Lead**.
- Actions show their saved property changes, amounts and dynamic inputs. Trigger overrides read **Hold to Fire** or **Hold to Charge**.
- **Custom Perks** and **Select Custom Perk** hide ornaments unless you check **Show Ornaments**. Both windows share this setting.
- Renamed actions to describe what they change:
  - **Hold a Weapon Count** is now **Enable Tracking**. Use it with **Change Fired Projectile** when the new projectile should track a target.
  - **Set a Weapon Firing Mode** is now **Override Animation Input**.
  - **Hold a Named Count** is now **Give a Sword a Key** and moved to **Weapon Settings**. The key selects a sword guard or attack and has no effect on other weapons.
  - The **Element Switch** behavior is now **Damage Type Switch**.
- **Ability Property** offers more stock modifiers, including **Sentinel Healing Shield** and **Extended Lunge Range**. Descriptions explain each setting and flag untested behavior.

### Builds and Installs

- **Build & Stage** can skip failing recipes, list their errors and build the remaining selection using completed preparation work. Installing a partial build removes copies of skipped items from your account.
- **Review Installation** shows added subclasses and shaders, which characters will equip them, ability resets and Collections unlock changes.
- Renaming a recipe warns if it creates a new item identity. Installing the renamed item removes copies of the previous item from your account.
- Build progress uses clearer units and gives longer stages more of the progress bar. Progress is preserved across build passes.
- **Show Detailed Runtime Output** reveals timings and entry ranges in the activity log and includes them in **Copy Progress**.
- Large builds use additional packages when item records, artwork or private effects reach capacity.

### Collections and Badges

- Authored items count toward each Collections page's total and acquired count on Dawn and Sunrise without using unlock slots. The Project Sunrise badge shows as complete. Replacing an older build clears its retired authored unlocks.
- **Automatic** Collections placement keeps authored items before stock entries, including alongside Sunrise Studio mods. On pages where another mod places its own entries first, Automatic items follow the stock entries.
- The **Badges** page puts Project Sunrise first and sorts custom badges by name.
- The Project Sunrise badge's class pages list authored entries by item type, then by rarity from Exotic to Common.

### Library and Interface

- Added **Type**, **Damage**, **Ammo** and **Rarity** filters to the **Recipe Library** and **Items in This Build**. Hidden selections still build, and the list shows their count.
- Save and reload named selections through **Build Lists** in **Items in This Build**.
- Press Ctrl+S to save the open recipe.
- The bottom bar shows background tasks. Hover to see all tasks, or click to open their status.

### Performance

- Builds reuse identical assets and custom perks to save disk space, with faster game-file checks and package preparation.
- Fixed unchanged weapons recompiling on large builds.
- Parhelion opens its loading screen immediately and loads recipes and perks in the background, reducing freezes during startup, saves and browsing large libraries.
- Reduced pauses while typing or dragging custom perk controls. Drafts still save during editing and when Parhelion closes.
- Model previews prepare in the background to keep the editor responsive. On Windows, previously viewed models reopen faster across app restarts.

### Fixes

- Fixed a character selection crash with large combinations of installed mods.
- Fixed the magazine size preview counting dynamic perk bonuses as permanent bonuses.
- Fixed authored subclass tooltips showing the base subclass's unfinished tree objective.
- Fixed saved damage sockets restoring a weapon's previous element after changing **Damage Type**.
- Fixed subclass abilities disappearing from the catalog after mod installs, which could also block builds using subclass conditions.
- Fixed builds failing with "Shader Glow found an unsupported or ambiguous material operation" after choosing certain weapon or ornament appearances. Parts whose materials **Shaders Light Glowing Parts** cannot change now stay as they are.
- Fixed subclass recipes showing "The HUD glyph table has an unexpected layout" and no icon preview after installing a build that adds ability HUD icons or colors.
- Fixed installs equipping a subclass you had switched away from and resetting its ability choices. Only newly added subclasses are equipped automatically.
- Fixed **Build & Stage** saving or rejecting an open recipe that was excluded from the selection, preserving its unsaved edits.
- Fixed incorrect stock objective progress after builds added Collections pages or the Project Sunrise badge.
- Fixed an empty Project Sunrise badge appearing on the **Badges** page when no item in the build belongs to it.
- Fixed builds rejecting certain valid private perks and component combinations.
- Fixed Sunrise Studio mods missing from catalogs and installation review, including older Studio releases. Parhelion mods remain compatible with older Sunrise releases.
- Fixed valid saved recipes being hidden or blocking other saves when their cached listings are damaged or belong to another library.
- Fixed animated model previews turning some parts to the wrong angle, such as a machine gun's feed cover and ammo belt.
- Fixed valid weapon edits being rejected and **Barrel Settings** failing to load after installing a large build on Dawn or older Sunrise versions.
- Fixed brief freezes after opening a Sparrow recipe with a vehicle or projectile chosen through **Choose Other Vehicle…** or **Choose Other Projectile…**.
- Fixed brief freezes while **Custom Perks** resolves asset names or first opens its projectile picker.
- Fixed perks from saved items appearing twice in **Custom Perks**. Opening the window removes the extra copies.
- Fixed shader **Preview** showing weapons when editing another gear type's dyes. It now lists items of that gear type, while **All Gear** lists every type.
- Collections errors now say which weapon recipe failed and which base or appearance item is missing.
- Fixed installs failing with "Access is denied" when another Sundial or Parhelion window held a package file open.
- Fixed removed items appearing under **Invalid Items** after returning from Parhelion.
- Fixed model previews failing with "Resource … has an invalid size" after installing a build that adds item artwork.
- Package backups on Dawn and older Sunrise follow the configured limit, while retaining recovery backups from installs that changed account or settings data.
- Recipe backups keep the configured number of recent copies for each recipe.

## Sundial

- Added **Appearance** for editing a character's face, hair, markings and colors, with native thumbnails, portrait and full-body previews, **Save** and undo.
  - Appearance requires Sunrise 0.6 or later with a SQLite investment schema v3, v5, v7 or v8 account. Schema v2 accounts still open without Appearance and are not migrated.
  - Previews include native skin and lip colors, hair and brows, layered markings and colored Exo eye lights. An info icon beside the preview opens help for its controls.
- Sundial and Parhelion now work with Sunrise settings v19 and SQLite schemas v5, v7 and v8, and earlier versions keep working. Items that Sunrise sets aside while their content is missing stay in your account.
  - Schema v7 and later raise the character inventory and material limits, and Sundial follows them. Saves leave overflow items where Sunrise placed them and keep each item's objective progress.
  - On schema v8, saves, package installs and backups keep the mission flags that mods store in the account.
  - **Export Package Keys** works with v19. The **Advanced** tab also adds **Export Tag Definitions** for Sunrise Studio.
  - Settings v19 requires **Allow Multiplayer** in **Networking** to use an external server or a bind address other than `127.0.0.1`.
- If `settings.json` fails to load, **Restore Settings Backup…** restores a chosen backup and preserves the current file first.

### Fixes

- Closing Sundial while **Review Changes Before Saving and Exiting** is open keeps the review and your edits intact.
- Sundial rechecks the equipped weapon before deleting and asks for review if its slot changed.
- Switching installations reloads current settings and asks for review if they use a newer schema.
- Improved performance when saving large SQLite inventories, applying bulk Collections changes or inspecting account progression.
- Updated bundled SQLite to address a security vulnerability in database queries.
- Restoring an account backup keeps its SQLite schema, including v5, v7 and v8, and rejects a backup from a different schema.
- Fixed settings saves on Dawn failing with an account source error when player-state.db is missing or can't be read.
- Fixed **Reset Settings to Dawn Defaults** removing the account data kept in settings.json. Its confirmation now names Dawn and player-state.db.
- Fixed **Cancel Download** waiting on a stalled connection.

Sundial v0.4.1 and newer can update to v0.6.1 in the app. Older releases should install v0.6.1 manually.

Parhelion remains experimental, and some item and perk combinations may crash. Report issues on [GitHub](https://github.com/KyleThmpsn/sundial/issues), or contact kylethmpsn on Discord or [Twitter/X](https://x.com/kylethmpsn). Include the recipe and describe what happens in game when reporting a Parhelion issue.

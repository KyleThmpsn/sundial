Sundial v0.6.1 reduces package sizes, speeds up build preparation and fixes some freezes in Parhelion.

Read the [Parhelion README](crates/parhelion/README.md) for the workbench, bundled examples, and build and install instructions. Find previous release notes on the [releases page](https://github.com/KyleThmpsn/sundial/releases).

## Parhelion

- Builds reuse identical textures and other assets between items, so built packages take much less disk space.
- Builds spend less time checking game files and preparing packages.
- A weapon's **Perks & Sockets** has two views. **Reusable** shows the perks each socket can swap to, and **Randomized** shows every perk it can roll, reusable ones included.
  - Right-click a perk and choose **Make Reusable** or **Remove from Reusable** to change whether it can be swapped to.
  - The base weapon's random perks that aren't in your recipe appear greyed out in **Randomized**. Choose **Add to Randomized Plug Set** to include them.
- Renamed the custom perk action **Hold a Weapon Count** to **Enable Tracking**. Use it with **Change Fired Projectile** when the new projectile should track a target.
- Renamed **Set a Weapon Firing Mode** to **Override Animation Input**. Its controls now describe the animation selection key it changes.
- Custom perk projectile cards now show tracking settings such as **Turn Rate**, **Lead Time Limit** and **Target Lead**.
- Custom perk actions now show their saved property changes, amounts and dynamic inputs. Trigger overrides read **Hold to Fire** or **Hold to Charge**.
- The bottom bar shows work running in the background, such as a build, an install or a catalog reload. Hover it to see every task, or click it to open the task's status.

### Fixes

- Fixed valid private perks and component combinations failing to compile when optional component tables are empty.
- Large builds now use additional packages when item records, artwork or private effect resources fill a package. This avoids package-capacity errors.
- Fixed Parhelion freezing with large recipe libraries during startup, after saves, when browsing the **Recipe Library** and when opening **Custom Perks**.
- Fixed valid saved recipes being hidden or blocking other saves when their cached listings are damaged or belong to another library.
- Fixed model previews freezing Sundial and Parhelion when a model is first shown. A still image appears while the interactive preview loads.
- Fixed valid weapon edits being rejected and **Barrel Settings** failing to load after installing a large build on Dawn or older Sunrise versions.
- Fixed brief freezes after opening a Sparrow recipe with a vehicle or projectile chosen through **Choose Other Vehicle…** or **Choose Other Projectile…**.
- Fixed brief freezes while **Custom Perks** resolves asset names or first opens its projectile picker.
- Perks can now be dragged into empty sockets.
- Weapon sockets that roll at random now warn immediately if they have no default perk. Previously, the warning appeared only after another socket was edited.
- Fixed shader **Preview** showing weapons when editing another gear type's dyes. It now lists items of that gear type, while **All Gear** lists every type.
- Collections errors now say which weapon recipe failed and which base or appearance item is missing.

Sundial v0.4.1 and newer can update to v0.6.1 in the app. Older releases should install v0.6.1 manually.

Parhelion remains experimental. Authored abilities, custom perks on abilities, Emblems, mods, sockets on armor and other items, vehicles, Subclasses, appearances from other weapon types, and borrowed perk behavior still need in-game testing, and some combinations may crash. Report issues on [GitHub](https://github.com/KyleThmpsn/sundial/issues). You can also reach me on Discord or [Twitter/X](https://x.com/kylethmpsn) as kylethmpsn. For Parhelion issues, include the recipe and describe what happens in game.

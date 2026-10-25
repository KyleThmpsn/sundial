Sundial v0.6 lets Parhelion edit Subclass abilities, not just swap them, and adds vehicles, Emblems, standalone mods, more control over weapons and perks, and support for Project Sunrise 0.6. Model previews are more accurate, and Sundial starts faster and uses less memory.

Read the [Parhelion README](crates/parhelion/README.md) for the workbench, bundled examples, and build and install instructions. Find previous release notes on the [releases page](https://github.com/KyleThmpsn/sundial/releases).

## Parhelion

### Subclasses and Abilities

- Subclass abilities can now be edited, not just swapped. Grenades, melees, class and movement abilities, Supers, and attunement nodes can take their own names, icons, colors, and perks. Depending on the ability, you can also swap its projectile, change its damage type, add charges, change its recharge rate, and tune many other properties.
  - Rename any ability and give it a new description and icon, from any class's ability icons or the game's other icons. Your edits stay with that ability and everything it creates.
  - Give an ability stock or custom perks. Right-click a stock perk and choose **Edit as Custom Perk…** to change it for this ability alone.
  - Add up to four **Extra Charges** to most abilities, including Rifts, and change **Recharge Rate**. **Changes While Equipped** sets what an ability or node changes about the Subclass's other abilities, such as an extra grenade charge.
  - **Gameplay** gives each part an ability creates its own card, grouping its values under headings such as **Flight**, **Collision**, and **Health**. Tune timers, projectile flight and tracking, damage taken, changes to you and your weapons, invisibility, Super energy use, and Barricade and Rift health.
  - **Projectile** swaps an ability's projectile for any other in the game, including weapons' and enemies'. The new projectile has its own speed, gravity, and **Damage Type**.
  - **Blink Distance**, **Airborne Jumps**, **Vertical Impulse**, and **Directional Impulse** tune Blink, Hunter jumps, lifts, and glides.
  - **Falloff Start** sets how much of the area of Thermite Grenade, Voidwall Grenade, Daybreak, and Arc Staff hits at full strength before the effect fades toward its edge.
  - **Damage Type** changes supported projectile, detonation, and lingering-area damage. Effects keep their colors, so pair it with **Visuals**.
  - Set a color for the Subclass with **Subclass Color** and for each ability with its **Color**. It shows on the HUD's ability icons and charge bars and on the Subclass screen's tree. Pick the game's **Arc**, **Solar**, or **Void** color or any hex code.
  - **Effect Colors** lets you recolor an ability's effects, including trails, lights, and decals. Adjust them all with **Overall** or each color on its own, copy another ability's with **Colors From**, or set one hue with **Colorize**.
  - **Attached Abilities** changes the icon and HUD color of abilities a Super provides, such as Hammer of Sol or Sentinel Shield's melee.
  - Ability authoring is experimental. Some changes may not work properly yet, and some labels will be refined as I learn more about what their values affect.
- Subclasses have a **Class** picker. **Any Class**, the default, lets every character equip it and labels it **Guardian Subclass**. **Titan**, **Hunter**, or **Warlock** limits it to that class, even on another class's base.
- **Damage Type Icon** sets the Arc, Solar, Void, or Kinetic icon a Subclass shows beside its name. Its abilities keep their own damage types.
- The Subclass **Appearance** tab sets **Screen Art**, the full-screen picture for each attunement, from another Subclass or your own image.
- **Generated Icon** draws a Subclass's inventory icon as the stock ones are, a diamond in its **Subclass Color**, with a **Symbol** you pick from the artwork browser, including any perk's icon. You can also use your own image instead.

### Weapons

- **Weapon Stats** include starting perk bonuses, with green and red changes and a hover breakdown.
- **Firing Behavior**, **Barrel**, and **Magazine** on **Gameplay** can each come from a different weapon and no longer need Experimental Features. Each one now copies just that part's values into your base weapon and keeps the rest of its gameplay intact, so these swaps are much more reliable.
- **Barrel Settings** on **Gameplay** adds **Bullets per Shot**, which sets how many bullets fire with each trigger pull, and **Pellets per Bullet**, **Spread**, and **Pattern**. Choose **Filled Circle**, **Ring**, or **Custom**, then edit each ring's pellet count, inner and outer radius, rotation, and randomness beside a preview.
  - Some weapon types may not work as expected with **Pellets per Bullet** above 1. Extra Rocket Launcher rockets, for example, explode as soon as they're fired. Confirmed working types include Fusion Rifles, Shotguns, Pulse Rifles, Auto Rifles, Scout Rifles, Submachine Guns, Sniper Rifles, Grenade Launchers, Sidearms, and Hand Cannons.
- **Projectile** on **Gameplay** tunes what a weapon fires, such as a rocket's speed and gravity, on any weapon that fires one, including shotguns and fusion rifles. Projectiles it spawns get their own **Spawned Projectile** cards, and ones that spawn only with certain perks or states sit under **Show Alternatives**. Other weapons that fire the same projectile keep theirs.
- **Animations** on **Appearance** can come from another weapon. Its **Actions** borrow single actions, such as **Hip Fire** or **Holster**, from another frame. **Reload Animation** can borrow other reloads on compatible rigs, including Submachine Guns, Grenade Launchers, and Bows.
  - Some weapon type combinations may still not work as expected, such as a Bow's appearance on a Shotgun base, Swords with other weapon types, or Machine Guns with some weapon types.
- A weapon with another type's appearance now takes that type's name, Collections page, and HUD icon. A **Keep … Type** checkbox on **Appearance**, such as **Keep Scout Rifle Type**, keeps the base weapon's.
- **Placement** on **Appearance** moves the model in the hand, and **Markers** moves named points such as the sights and muzzle.

### Project Sunrise 0.6 and Sunrise Studio

- Parhelion supports Project Sunrise 0.6's mods feature. Builds install to the mod folder, and Sunrise's **Mods** page lists them as **Parhelion**, with the version of Sundial that built them. Packages from earlier builds can't move there, so Parhelion offers to build them again, and installing backs up and removes the old ones. **Review Sunrise Mod Upgrade…** in **Preferences… > Builds & Backups** opens that review again.
- You can switch between Sunrise 0.6 and Dawn or an earlier Sunrise. Sundial and Parhelion read whichever runtime is active, and installing there backs up and removes the build installed for the other one, so the two never load together. Parhelion keeps the build you last installed for each, and after you switch back it offers to install that build again without a rebuild.
- On Sunrise 0.6, Parhelion installs alongside other mods, including Sunrise Studio mods. Sunrise combines the rows both mods add to item, weapon, perk, Collections, and other shared tables. Installation stops and says why when both mods change the same entry in a way Sunrise can't combine, when a mod that loads before Parhelion adds Collections pages, or when a mod declares Parhelion incompatible.
- Items that join a game Collections page, such as weapons, Exotic armor, and items shown beside their base item, have a Collections **Position**. **Automatic** keeps them first on their page, as before, unless another installed Sunrise mod changes Collections. Then they go last, so both mods load. The Sunrise badge and added pages follow the same rule.
- Sundial and Parhelion also read content from other Sunrise mods, including their configuration values and the tables Sunrise combines from several mods, and refresh when a mod changes. A mod Sunrise would refuse is left out, and Sundial's **Copy Report** lists why.

### Vehicles

- Sparrows have a **Vehicle** editor with **Driving**, **Handling**, **Durability**, and **Weapons** cards. Values are set as a percentage of the vehicle's stock values.
- **Summon Vehicle** lets a Sparrow summon another vehicle instead, such as a Pike, Interceptor, or Tank.
  - The Sparrow's inventory icon becomes that vehicle's HUD silhouette, or clear **Vehicle Icon** to keep its own art.
- **Driving Speed** goes up to 10× the vehicle's speed. The Sparrow's **Speed** stat is display only, so **Driving Speed** is what changes how fast it drives. **Acceleration**, **Braking**, and **Boost Strength** can be set separately or together with **Match Speed**.
- **Handling** adds better side dodges, air control, roll tricks, and faster summoning. **Durability** adjusts health and repair.
- Armed vehicles can change **Weapon Damage** and **Firing Rate**, or fire another vehicle's projectiles.

### Armor, Emblems, and Other Items

- Parhelion can now author Emblems, with your own banner, overlay, and background images, nameplate colors, and **Stat Trackers**. Each nameplate image has **Edit Artwork…** to crop, position, rotate, flip, and recolor it.
  - In game, the character screen only shows the lower part of an Emblem's background, about two thirds of it on a 16:9 screen and less on ultrawide. The Background tile, its preview, and the artwork editor shade the hidden part so you can see where your picture will be cut off. **Fit Visible Area** in the editor shrinks the whole picture into the visible part.
- Armor, Sparrows, Ships, and Ghost Shells can add sockets, remove sockets, or change a socket's role. Armor's energy sockets are still set with **Energy Type** and **Energy Capacity**.
- Armor has a **Class** picker: the base's class, **Titan**, **Hunter**, **Warlock**, or **Any Class**. It sets who can equip the armor, where it appears in Collections, and which class badge it joins.
- Authored armor appears in a **Project Sunrise** or **Dawn** category under **Collections > Armor** for each class it supports, in **Armor Set** rows of up to five. Pieces from the same set stay together in slot order. Exotic armor appears under **Exotics** instead.
- Sparrows, Ships, Ghost Shells, Emblems, and Shaders can be **Exotic** on any base.
- The artwork editor for Emblems, badges, and release watermarks adds **Before** and **After** comparison, **Reset All Edits**, and color and opacity adjustments.

### Custom Perks

- The bundled **Custom Perks** include **Wolfpack Rounds**. Attaching it carries its projectile assets with the weapon, so saved recipes can rebuild without separate asset files.
- **Change Weapon Properties** lets a perk change a weapon's own properties, such as bullets per shot, rate of fire, spread, damage, magazine size, and reload time. Pellets per bullet isn't one of them. Set it under **Gameplay > Barrel Settings**.
- **Change an Ability Property** can set or add to an ability's own values, such as a grenade's blast radius, with **Add Property…**.
- **On a Specific Ability** and **Ends on a Specific Ability** can name any Subclass ability.
- A value can scale with your **Intellect**, **Discipline**, or **Strength** tier, as the game's own stat bonuses do.
- Extra melee and class ability charges now work on more abilities.
- Attached effects such as Devour and invisibility can have their own **Attachment Length**, or never end on their own with **Unlimited**.
- HUD statuses such as Arc Shield can show your own **HUD Name** and **HUD Image**, or another status's **HUD Icon**.
- A projectile a perk fires can have its own **Damage Type**.
- **New Item > Mod** creates a standalone mod from a custom perk. It shows up in every matching socket, such as every weapon's mod socket, without being attached to an item. A custom perk on one of your items can also appear in every matching socket with **Offer Everywhere**.
- More perk properties have names instead of numbers, such as **Damage**, **Blast Radius**, and **Magazine Size**.
- **Perk Diagnostics…** lists problems that can keep a perk from working. It won't catch everything, but it can flag some issues before you build and install.
- **Find All Uses** in the **Engine Catalog** lists everything that uses a resource.

### Workbench and Builds

- Builds and installs are faster. Unchanged items reuse their compiled data and more work runs on several cores at once. Parhelion also uses less memory when browsing artwork and the recipe library.
- Opening an item, its model preview, and other views that read the game's packages no longer spends several seconds rereading the packages each time.
- Installation also removes saved items and perk selections the game no longer defines, such as ones left from an older build. The review lists them before you confirm, and the account backup keeps them.
- Weapon, Armor, Sparrow, Ship, and Ghost Shell pages now show an embedded model preview, with a corner button that opens the full viewer. To load a page's model only when you click **Load Model**, turn off **Load Model Previews Automatically** in **Preferences…**.
- A changed value turns its name white and shows its original value beside it. Click that to restore it. A dot marks each section you've changed.
- Stats are drawn as in Destiny's item tooltips, with a bar and the change from the base in green or red.
- The recipe editor has **Undo** and **Redo**, in the **Recipe…** menu or with Ctrl+Z and Ctrl+Y.
- Leaving a recipe with unsaved changes now offers to save them.
- **Technical Build** also shows the rig, moved markers, and animation sources, flags when the recipe has changed since the build, and names far more of the game's native fields.
- The recipe library and **Items in This Build** mark recipes whose items are installed, and **Build & Stage** warns before a build would remove installed items and offers **Add to Build**.

### Fixes

- Fixed refreshing Custom Perks allowing unsaved edits to overwrite changes saved in another window.
- Fixed custom perk drafts being lost or replaced, such as when a second Parhelion window autosaved or an autosave failed before Sundial closed. Sundial now asks before closing while drafts are unsaved.
- Fixed some borrowed weapon behaviors leaving out data their perks rely on.
- Fixed choosing a borrowed behavior for a weapon moving its custom perks, or their choice weights and conditions, onto other perks in the same socket.
- Fixed large builds, such as several Subclasses with swapped abilities, failing when they ran out of room for private perks and abilities.
- Fixed the game sometimes freezing when inspecting an authored Subclass that takes abilities from other Subclasses.
- Fixed weapons with another type's appearance firing like that type, such as a Scout Rifle with a Pulse Rifle look firing three-round bursts, or being held the wrong way.
- Fixed a weapon dealing Kinetic damage when the socket holding its damage type was given another role or removed.
- Fixed **Restore Donor Row** keeping custom perks the restored socket no longer offers.
- Fixed owned weapons keeping an old default perk after a rebuild made a custom perk the default.
- Fixed installs leaving a socket empty and hidden on a saved item when its selected perk no longer exists after a change to the item's sockets. The socket now gets its new default perk.
- Fixed a renamed custom perk sometimes showing its base perk's name and description in tooltips.
- Fixed authored weapons with an Exotic base, an Exotic appearance, or an ornament refusing shaders.
- Fixed exported recipe bundles with deeply nested recipes failing to open.
- Fixed custom perk ability and event conditions accepting values the game ignores. The game signal conditions are now **When a Game Signal Starts** and **When a Game Signal Ends**.
- Fixed installs and recovery overwriting package files or mod metadata another program changed in the meantime.
- Fixed installs and removals leaving a `.parhelion-transaction.lock` file in the Destiny 2 folder.
- Fixed installs, recovery, and removals failing when the Sunrise or Dawn DLL is linked to a build outside the game folder.
- Fixed installs and removals clearing only the active runtime's item cache, so switching between Dawn and Sunrise could start the game with outdated custom items.
- Fixed builds refusing some base perks and items the game records in more than one place.
- Fixed builds occasionally failing on a custom perk or item, depending only on which identifiers the build gave it.
- Fixed Dawn installs and uninstalls sometimes bringing back removed items or changing account data they didn't list.
- Fixed Dawn installs leaving saved rolls on sockets a weapon no longer has.
- Fixed Dawn Collections sync sometimes adding a duplicate unlock row, which made Sundial refuse the account, or changing an account whose database layout Sundial doesn't support.
- Fixed viewing a socket's plug set or other technical fields changing their saved value when the installed game has fewer rows.
- Minor UI fixes and adjustments.

## Sundial

- Sunrise 0.6 accounts (SQLite schema v3) can be edited alongside earlier Sunrise SQLite and JSON accounts. Saves preserve character appearances and starter loadouts. Reset and recovery require matching account formats, and Parhelion protects items used by starter loadouts during removal.
- The catalog cache is about 50 times smaller and inspection caches are compressed, so Sundial starts faster and uses far less memory and disk space. The first start after updating rescans the installation once.
- The perk picker's **Plugs Offered** dropdown adds a sixth scope. On an Auto Rifle's barrel socket, they range from the item's own barrels through **Auto Rifle Barrels**, **All Barrels**, **Auto Rifles**, and **All Weapons** to **All**.
- The **Inspector** is redesigned, inspired by the Armory design I used for Dawn's in-game Loadout Studio. Its home page browses every item without a search.
  - **Weapons**, **Armor**, **Cosmetics**, **Perks & Mods**, **Other**, and **Recent** tabs show items as cards in their rarity's color, grouped by type.
  - Filter by type, rarity, and class, sort by type, name, or rarity, and show **Dummy Items**. Search narrows every tab.
  - Hovering over an item card or item link shows a tooltip styled like the ones in game. Plugs show their stat changes and perk descriptions. **Show Package and Hash in Item Tooltips** in **Preferences** adds where each item is defined.
  - Right-click a card to **Open** it, **Add To** a character or the profile, open its **Model Preview**, or copy its name or hash.
  - An item's **Overview** shows its model beside its details when the window has room.
- Item, perk, and plug tooltips across Sundial and Parhelion, including perk pickers, abilities, and the recipe library, use the same in-game style.
- Characters can now hold bounties, quest steps, engrams, and other non-equipment items. A quest added this way may not start at its first step.
- **Game Settings > Sunrise > Advanced** adds Project Sunrise 0.6's **Unlock Cheat Tools**, **Allow Studio Attach**, and **Export Package Keys**. Cheat tools unlock movement tools and infinite ammo in Sunrise's in-game menu, and progress made while they're on isn't saved. The other two are for Sunrise Studio.
- **Open Parhelion** is now always in the sidebar.
- **Field of View** in **Game Settings > Display** goes up to 155 on Sunrise 0.5 or later without turning on a preference. The in-game menu still stops at 105.

### Model Previews

Model previews in Sundial and Parhelion support more models and reproduce more of their materials, lighting, and motion. They remain approximations of the game.

- Previews do more of their work on the graphics card, including animation, cloth, particles, and exports, so they use less CPU.
- More models load with their textures and lighting, including props, map pieces, and vehicles. Terrain can now be previewed too.
- Materials better reproduce painted and worn surfaces, reflections, decals, and line effects. Armor, Ghost Shells, Ships, and Sparrows also have more accurate shader colors and glow.
- Iridescent paint now shifts color with viewing angle as it does in game, such as the purple sheen on Age-Old Bond.
- Transparent and glowing effects, such as ornament sights and energy, blend more accurately over the model as the camera moves.
- Finer detail survives, such as normal maps on mirrored parts, distant patterns, and partly filled texture sheets.
- Cutout parts of materials, such as grate holes, no longer draw as solid black.
- Glowing surfaces follow their material's brightness and have bloom halos. **View > Bloom** and **Filmic Output** control glow and highlights.
- Default lighting keeps darker paint and midtones, and reflections follow the **Key** and **Fill** lights, so surfaces look less washed out. Rough surfaces no longer take a pale sheen at their edges.
- Vehicle previews keep their full textures, lights, and damage markings, and long effect meshes no longer shrink them.
- Cloth now moves in previews, including capes, Titan marks, and Warlock robes, and robes keep their full length.
- Animations support all recovered clip formats, with improved joint movement, lighting, and framing. Models that share animations play together.
- Very large models no longer lose parts, and appearances no longer pick up stray particle parts.
- **View > Particle Study** shows particle previews with their placement, motion, and fading.
- **View > Auto Rotate** orbits the model. Embedded previews zoom with the scroll wheel and pan with Shift-drag.
- Right-click an item and choose **Model Preview** to see it with its saved ornament and shader. **Details** opens the **Assets and Effects** list in its own window.
- Exported models keep these colors and materials without hairline cracks, and very large or densely textured models now export correctly.

### Fixes

- Fixed switching between Sunrise and Dawn opening the previous runtime's account source. Dawn selections now remember the correct settings folder.
- Choosing a runtime copy now backs up Dawn's settings, account, and scripts with its DLL, and restores them together.
- Fixed external links not opening in your default browser.
- Fixed wider perk picker scopes listing shaders and ornaments on sockets that aren't cosmetic.
- Fixed quests, bounties, currencies, and consumables in a character's inventory reading as not valid for the character.
- Fixed swapping a Subclass offering every item instead of other Subclasses.
- Fixed saving asking you to close Destiny 2 after it had already closed.
- Fixed account saves, Collections sync, and Parhelion item grants writing a Sunrise SQLite or Dawn account when Destiny 2 started during the save.
- Fixed opening Parhelion freezing Sundial until a model preview finished decoding audio.
- Fixed Sundial asking again which settings folder to use after restarting for an update.
- Fixed Dawn settings refusing to save when a character held more than 135 unequipped items.
- Fixed Dawn saves overwriting changes another tool made after the account was loaded. Sundial now asks you to reload.
- Fixed Dawn account saves and Collections sync leaving an empty database when the account file disappears during the operation.
- Fixed Dawn vendor edits deleting data other tools linked to those vendors.
- Fixed the Linux installer writing through a link already at the launcher's location.
- Fixed two security issues in the XML parsing that Linux accessibility uses.
- Fixed opening a page rewriting saved values outside a field's range, such as a game setting, an item's quantity, or a progression value.
- Minor UI fixes and adjustments.

Sundial v0.4.1 and newer can update to v0.6 in the app. Older releases should install v0.6 manually.

Parhelion remains experimental. Authored abilities, custom perks on abilities, Emblems, mods, sockets on armor and other items, vehicles, Subclasses, appearances from other weapon types, and borrowed perk behavior still need in-game testing, and some combinations may crash. Report issues on [GitHub](https://github.com/KyleThmpsn/sundial/issues). You can also reach me on Discord or [Twitter/X](https://x.com/kylethmpsn) as kylethmpsn. For Parhelion issues, include the recipe and describe what happens in game.

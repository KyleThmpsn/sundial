Sundial v0.6 expands Parhelion with custom abilities, vehicles, Emblems, and mods, more control over weapons and perks, and improved model previews. Sundial also starts faster and uses less memory.

Read the [Parhelion README](crates/parhelion/README.md) for the workbench, bundled examples, and build and install instructions. Find previous release notes on the [releases page](https://github.com/KyleThmpsn/sundial/releases).

## Parhelion

### Subclasses and Abilities

- Subclass abilities can now be made your own, not only swapped. Any grenade, melee, class ability, movement ability, Super, or attunement node can fire a different projectile, deal another damage type, gain charges, recharge faster, move farther, take new colors, and carry its own perks, including custom perks.
  - Give each one its own name, description, and icon. Changes apply to that ability alone and everything it creates, such as Suppressor Grenade's detonation.
  - Each ability's page has **Ability**, **Perks**, **Gameplay**, and **Visuals** tabs, marked once you change something.
  - **Change Icon…** opens an icon browser that starts with **Ability Icons** from every class and can search **All Icons**.
  - Right-click a stock perk and choose **Edit as Custom Perk…** to change it for this ability alone.
  - Add up to four **Extra Charges** to any ability, including Rifts, and change **Recharge Rate**. **Ability Changes** sets what an ability or node changes about the Subclass's other abilities, such as an extra grenade charge.
  - **Find Properties** searches every value an ability has.
  - **Always Active** shows a Subclass's **Base Movement** and **Stat Passives**.
  - Moving an ability to another node or basing it on another one keeps the edits that still apply.
  - **Gameplay** shows a card for each part an ability creates, with controls for timers, projectile flight and tracking, damage taken, buffs, invisibility, Super energy use, and Barricade and Rift health. Finer values sit in a closed **More** section.
  - **Projectile** swaps an ability's projectile for any other in the game, including weapons' and enemies', such as a Skip Grenade that fires Axion Bolt seekers. The new projectile has its own speed, gravity, and **Damage Type**.
  - **Blink Distance**, **Airborne Jumps**, **Vertical Impulse**, and **Directional Impulse** tune Blink, Hunter jumps, lifts, and glides.
  - **Damage Type** changes supported projectile, detonation, and lingering-area damage, such as a Solar Axion Bolt. Effects keep their colors, so pair it with **Visuals**.
  - **Effect Colors** recolors everything an ability draws, including trails, lights, and decals. Adjust them all with **Overall** or each color on its own, copy another ability's with **Colors From**, or set one hue with **Colorize**.
  - **Subclass Color** sets one color for tree nodes, ability tiles, and charge bars, and each ability's **Color** can override it. Pick the game's **Arc**, **Solar**, or **Void** color or any hex code, or restore the original with **Donor Colors**. The Super meter stays gold.
  - **Attached Abilities** changes the icon and HUD color of abilities a Super provides, such as Hammer of Sol or Sentinel Shield's melee. They follow **Subclass Color** even when they have their own elemental color.
  - Some labels will be refined over time as I learn more about what their values affect.
  - Some changes may not work properly yet, and ability authoring will improve in future releases.
- Subclasses have a **Class** picker. **Any Class**, the default, lets every character equip it and labels it **Guardian Subclass**. **Titan**, **Hunter**, or **Warlock** limits it to that class, even on another class's base.
- **Damage Type Icon** sets the Arc, Solar, Void, or Kinetic icon a Subclass shows beside its name. Its abilities keep their own damage types.
- The Subclass **Appearance** tab sets **Screen Art**, the full-screen picture for each attunement, from another Subclass or your own image, and exports it as a PNG.
- **Generated Icon** draws a Subclass's inventory icon as the stock ones are, a diamond in its **Subclass Color**, with a **Symbol** you pick from the artwork browser, including any perk's icon.

### Armor, Gear, and Vehicles

- Armor, Sparrows, Ships, and Ghost Shells can add sockets, remove sockets, or change a socket's role. Armor's energy sockets are still set with **Energy Type** and **Energy Capacity**.
- Armor has a **Class** picker: the base's class, **Titan**, **Hunter**, **Warlock**, or **Any Class**. It sets who can equip the armor, where it appears in Collections, and which class badge it joins.
- Authored armor appears in a **Project Sunrise** or **Dawn** category under **Collections > Armor** for each class it supports, in **Armor Set** rows of up to five. Exotic armor appears under **Exotic > Armor** instead.
- Sparrows have a **Vehicle** editor with **Driving**, **Handling**, **Durability**, and **Weapons** cards. Each value is a percentage of the vehicle's own.
  - **Driving Speed** goes up to 10× the vehicle's speed, even on fixed-speed Sparrows. **Acceleration**, **Braking**, and **Boost Strength** can be set separately or together with **Match Speed**.
  - **Handling** adds better side dodges, air control, roll tricks, and faster summoning. **Durability** adjusts health and repair.
  - Armed vehicles can change **Weapon Damage** and **Firing Rate**, or fire another vehicle's projectiles, such as a Pike's or a Tank's.
  - **Summon Vehicle** makes a Sparrow summon a Pike, Interceptor, Tank, or another vehicle instead, shown in the page's preview. Its icon becomes that vehicle's HUD silhouette unless you clear **Vehicle Icon**, and **Inventory Model** sets which model inventory and inspect show.
- Sparrows, Ships, Ghost Shells, Emblems, and Shaders can be **Exotic** on any base.

### Emblems and Artwork

- Parhelion can now author Emblems, with your own banner, overlay, and background images, nameplate colors, and **Stat Trackers**. Each nameplate image has **Edit Artwork…** to crop, position, rotate, flip, and recolor it.
- The artwork editor for Emblems, badges, and release watermarks adds **Before** and **After** comparison and **Reset All Edits**. Emblems and badges get **Colors** for hue, saturation, brightness, contrast, opacity, and inversion, and watermarks get **Opacity**.

### Custom Perks

- **New > Mod** authors a mod on its own, with a custom perk offered in every socket of its type, such as every weapon's mod socket. **Offer Everywhere** does the same for a custom perk on an item you build, as Shaders are offered.
- **Change Weapon Properties** lets a perk change a weapon's own properties, such as rounds per burst, rate of fire, spread, damage, magazine size, and reload time. Set simultaneous pellets with **Pellets per Shot** under **Gameplay > Barrel Settings**.
- **Change an Ability Property** can set or add to an ability's own values, such as a grenade's blast radius, with **Add Property…**.
- **On a Specific Ability** and **Ends on a Specific Ability** can name any Subclass ability.
- A value can scale with your **Intellect**, **Discipline**, or **Strength** tier, as the game's own stat bonuses do.
- Extra melee and class ability charges now work on more abilities.
- Attached effects such as Devour and invisibility can have their own **Attachment Length**, or never end on their own with **Unlimited**.
- HUD statuses such as Arc Shield can show your own **HUD Name** and **HUD Image**, or another status's **HUD Icon**.
- A projectile a perk fires can have its own **Damage Type**.

### Weapons

- **Firing Behavior**, **Barrel**, and **Magazine** on **Gameplay** can each come from a different weapon and no longer need Experimental Features. Borrowed pellet barrels keep their full spread pattern.
- **Gameplay** shows **Parts** and **Barrel Settings** side by side. **Runtime**, formerly **Firing & Runtime Baseline**, is now the first part.
- **Barrel Settings** on **Gameplay** adds **Pellets per Shot**, **Spread**, and **Pattern** without Experimental Features or a Barrel donor. Choose **Filled Circle**, **Ring**, or **Custom**, then edit each ring's pellet count, inner and outer radius, rotation, and randomness beside a preview. Each value can restore the selected Barrel's own.
- **Projectile** on **Gameplay** tunes what a weapon fires, such as a rocket's speed and gravity. Other weapons that fire the same projectile keep theirs. Weapons that hit instantly show **Hitscan** for **Speed**.
- **Animations** on **Appearance** can come from another weapon. Its **Actions** borrow single actions, such as **Hip Fire** or **Holster**, from another frame. **Reload Animation** can borrow other reloads on compatible rigs, including Submachine Guns, Grenade Launchers, and Bows.
- A weapon with another type's appearance now takes that type's name, Collections page, and HUD icon. A **Keep … Type** checkbox on **Appearance**, such as **Keep Scout Rifle Type**, keeps the base weapon's.
- **Appearance** warns when a Sword appearance is used on another type or a model will play the base weapon's animations, and **Rounds Per Minute** warns when another type's runtime changes its fire rates.
- **Placement** on **Appearance** moves the model in the hand, and **Markers** moves named points such as the sights and muzzle.
- **Choose Ornament** has type, damage, ammo, and rarity filters, and opens on the base weapon's type when it has no ornaments of its own.

### Workbench and Builds

- Weapon, Armor, Sparrow, Ship, and Ghost Shell pages now show an embedded model preview, with a corner button that opens the full viewer.
- Item fields such as **Rarity**, **Class**, and **Energy Capacity** sit side by side. Hover a name to see what it does.
- A changed value turns its name white and shows its original value beside it. Click that to restore it. A dot marks each section you've changed.
- Stats are drawn as in Destiny's item tooltips, with a bar and the change from the base in green or red. Weapon stats follow the game's order and also show their saved value.
- **Add Socket** and each socket's role picker list every role once, with names that tell similar roles apart, such as **Undying Armor Mod** or **Hunter Universal Ornament**.
- **Plugs Offered**, formerly **Plug Safety**, sits beside each perk picker's search, with choices named for the socket, such as **Auto Rifle Barrels**.
- Weapons show how many perk effects their default perks use, against the game's limit of 16.
- More perk properties have names instead of numbers, such as **Damage**, **Blast Radius**, and **Magazine Size**.
- **Perk Diagnostics…** lists the problems that keep a perk from working.
- **Technical Build** now also shows the rig, moved markers, and animation sources, and flags when the recipe has changed since the build.
- **Find All Uses** in the **Engine Catalog** lists everything that uses a resource.
- **Resync Account** shows its progress, then the Collections and inventory changes it made and anything it couldn't add.
- Leaving a recipe with unsaved changes now offers to save them.
- Rebuilds are faster. Unchanged weapons and Subclass abilities reuse their compiled data, and independent packages build in parallel.
- Large builds no longer run out of room for private perks and abilities.
- Parhelion uses less memory when browsing artwork and the recipe library.
- **Build & Install** is easier to follow, with a resizable window and clearer progress. **Build & Stage** warns before a build would remove installed items and offers **Add to Build**.

### Fixes

- Fixed some borrowed weapon behaviors leaving out data their perks rely on.
- Fixed build and install progress finishing stages early or going past its total.
- Fixed **Runtime Values** showing **Reset to Donor** beside unchanged values.
- Fixed Parhelion sometimes freezing briefly while loading a recipe's icon.
- Fixed the game sometimes freezing when inspecting an authored Subclass that takes abilities from other Subclasses.
- Fixed weapons with another type's appearance firing like that type, such as a Scout Rifle with a Pulse Rifle look firing three-round bursts, or holding the support hand in the wrong place.
- Fixed a weapon dealing Kinetic damage when the socket holding its damage type, such as Nature of the Beast's **Weapon Mod**, was given another role or removed.
- Fixed **Restore Donor Row** keeping custom perks the restored socket no longer offers.
- Fixed owned weapons keeping an old default perk after a rebuild made a custom perk the default.
- Fixed a renamed custom perk sometimes showing its base perk's name and description in tooltips.
- Fixed authored weapons with an Exotic base, an Exotic appearance, or an ornament refusing shaders.
- Fixed exported recipe bundles with deeply nested recipes, such as Hammer Time, failing to import.
- Fixed a second Parhelion window replacing the first window's autosaved custom perk drafts.
- Fixed installs overwriting a package file another program changed during the install.
- Fixed Dawn installs and uninstalls sometimes bringing back removed items or changing account data they didn't list.
- Fixed Dawn installs leaving saved rolls on sockets a weapon no longer has.
- Fixed Dawn Collections sync adding a duplicate unlock row, which made Sundial refuse the account, or changing an account whose database layout Sundial doesn't support.
- Minor UI fixes and adjustments.

## Sundial

### Performance and Inspection

- The catalog cache is about 50 times smaller and inspection caches are compressed, so Sundial starts faster and uses far less memory and disk space. The first start after updating rescans the installation once.
- The perk picker's **Plugs Offered** dropdown adds a sixth scope. On an Auto Rifle's barrel socket, they range from the item's own barrels through **Auto Rifle Barrels**, **All Barrels**, **Auto Rifles**, and **All Weapons** to **All**.
- The **Inspector** home page browses every item without a search.
  - **Weapons**, **Armor**, **Cosmetics**, **Perks & Mods**, **Other**, and **Recent** tabs show items as cards in their rarity's color, grouped by type.
  - Filter by type, rarity, and class, sort by type, name, or rarity, and show **Dummy Items**. Search narrows every tab.
  - Hovering a card shows the game's item tooltip. Right-click a card to **Open** it, **Add To** a character or the profile, open its **Model Preview**, or copy its name or hash.
- Characters can now hold bounties, quest steps, engrams, and other non-equipment items. A quest added this way may not start at its first step.

### Model Previews

Model previews in Sundial and Parhelion support more models and reproduce more of their materials, lighting, and motion. They remain approximations of the game.

- Previews do more of their work on the graphics card, including animation, cloth, particles, and exports, so they use less CPU.
- More models load with their textures and lighting, including props, map pieces, and vehicles. Terrain can now be previewed too.
- Materials better reproduce painted and worn surfaces, reflections, decals, and line effects. Armor, Ghost Shells, Ships, and Sparrows also have more accurate shader colors and glow.
- Transparent and glowing effects, such as ornament sights and energy, blend more accurately over the model as the camera moves.
- Finer detail survives, such as normal maps on mirrored parts, distant patterns, and partly filled texture sheets like One Thousand Voices' fins.
- Glowing surfaces follow their material's brightness and have bloom halos. **View > Bloom** and **Filmic Output** control glow and highlights.
- Default lighting keeps darker paint and midtones, and reflections follow the **Key** and **Fill** lights, so surfaces look less washed out.
- Vehicle previews keep their full textures, lights, and damage markings, and long effect meshes no longer shrink them.
- Cloth now moves in previews, including capes, Titan marks, and Warlock robes, and robes keep their full length.
- Animations support all recovered clip formats, with improved joint movement, lighting, and framing. Models that share animations play together.
- Very large models no longer lose parts, and appearances no longer pick up stray particle parts.
- **View > Particle Study** shows particle previews with their placement, motion, and fading.
- **View > Auto Rotate** orbits the model. Embedded previews zoom with the scroll wheel and pan with Shift-drag.
- Right-click an item and choose **Model Preview** to see it with its saved ornament and shader. **Details** opens the **Assets and Effects** list in its own window.
- Exported models keep these colors and materials without hairline cracks, and very large or densely textured models now export correctly.

### Fixes

- Fixed wider perk picker scopes listing shaders and ornaments on sockets that aren't cosmetic.
- Fixed quests, bounties, currencies, and consumables in a character's inventory reading as not valid for the character.
- Fixed swapping a Subclass offering every item instead of other Subclasses.
- Fixed Dawn settings refusing to save when a character held more than 135 unequipped items.
- Fixed Dawn saves overwriting changes another tool made after the account was loaded. Sundial now asks you to reload.
- Fixed Dawn account saves and Collections sync leaving an empty database when the account file disappears during the operation.
- Fixed Dawn vendor edits deleting data other tools linked to those vendors.
- Fixed **Armor Stats** running several searches at once when goals changed quickly.
- Fixed the Linux installer writing through a link already at the launcher's location.
- Fixed two security issues in the XML parsing that Linux accessibility uses.
- Minor UI fixes and adjustments.

Sundial v0.4.1 and newer can update to v0.6 in the app. Older releases should install v0.6 manually.

Parhelion remains experimental. Authored abilities, custom perks on abilities, Emblems, mods, new gear, vehicles, Subclasses, appearances from other weapon types, and borrowed perk behavior still need in-game testing, and some combinations may crash. Report issues on [GitHub](https://github.com/KyleThmpsn/sundial/issues), on Discord, or on Twitter/X [@KyleThmpsn](https://x.com/KyleThmpsn). For Parhelion issues, include the recipe and describe what happens in game.

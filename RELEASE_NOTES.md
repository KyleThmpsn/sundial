Sundial v0.5.3 lets Parhelion fully author Subclass abilities and Emblems, borrow more parts from other weapons, and give custom perks more control over abilities. Sundial itself is lighter and has a more capable model preview.

Read the [Parhelion README](crates/parhelion/README.md) for the workbench, bundled examples, and build and install instructions. Find previous release notes on the [releases page](https://github.com/KyleThmpsn/sundial/releases).

## Parhelion

- Subclass abilities can now be authored, not only swapped.
  - Give any grenade, melee, class ability, movement ability, Super, or attunement node its own name, description, icon, and perks, including custom perks.
  - Each ability's page has **Ability**, **Perks**, **Gameplay**, and **Visuals** tabs, marked once you change something.
  - Right-click a stock perk and choose **Edit as Custom Perk…** to change it for this ability alone.
  - Add up to four extra **Charges**, now including Rifts, Phoenix Dive, Well of Radiance, Chaos Reach, and the second Titan melee.
  - **Recharge** makes an ability recharge faster or slower. A node's **Ability Changes** sets what selecting it changes about the Subclass's abilities, such as an extra grenade charge.
  - **Gameplay** shows a card for each part an ability creates, with its timers, projectile flight, buffs, and invisibility. **Fires** swaps the projectile it fires for another ability's, such as a Skip Grenade that fires Axion Bolt seekers.
  - **Blink Distance**, **Airborne Jumps**, **Vertical Impulse**, and **Directional Impulse** tune Blink, Hunter jumps, lifts, and glides.
  - **Effect Colors** recolors an ability's effects. Change the hue, saturation, and brightness of each color, or all of them with **Set All**. **Colors From** copies another ability's colors, **Colorize** sets a single hue, and **Overall** adjusts everything it draws, including trails, lights, and decals.
  - Changes apply to that ability alone, including everything it creates, such as Suppressor Grenade's detonation.
  - A Subclass's HUD colors can't currently be changed. The game sets them from the damage type of its Super.
  - Some changes may not work properly yet, and ability authoring will improve in future releases.
- Subclasses have a **Class** picker. **Any Class**, the default, lets every character equip it and labels it **Guardian Subclass**. **Titan**, **Hunter**, or **Warlock** limits it to that class, even on another class's base. **Custom Item-Type Label** sets your own label.
- The Subclass **Appearance** tab sets **Screen Art**, the full-screen picture for each attunement, from another Subclass or your own image, and exports it as a PNG.
- Armor and other gear can do more.
  - Armor, Sparrows, Ships, and Ghost Shells can add sockets, remove sockets, or change a socket's role. Armor's energy sockets are still set with **Energy Type** and **Energy Capacity**.
  - Armor has a **Class** picker: the base's class, **Titan**, **Hunter**, **Warlock**, or **Any Class**. It sets who can equip the armor, where it appears in Collections, and which class badge it joins.
  - Authored armor appears in a **Project Sunrise** or **Dawn** category under **Collections > Armor** for each class it supports, in **Armor Set** rows of up to five. Exotic armor appears under **Exotic > Armor** instead.
  - Sparrows have **Driving Speed**, up to 10× their normal speed, even on fixed-speed bases such as Always on Time.
  - **Summon Vehicle** makes a Sparrow summon a Pike, Interceptor, Tank, or another vehicle instead.
  - Gear pages show a preview of the base item's model, with a corner button that opens the full viewer.
- Parhelion can now author Emblems, with your own banner, overlay, and background images, nameplate colors, and **Stat Trackers**.
- Custom perks can do more with abilities.
  - **On a Specific Ability** and **Ends on a Specific Ability** can name any Subclass ability.
  - **Change an Ability Property** can set or add to an ability's own values, such as a grenade's blast radius, with **Add Property…**.
  - Extra melee and class ability charges now work on more abilities.
  - Attached effects such as Devour and invisibility can have their own **Attachment Length**.
  - HUD statuses such as Arc Shield can show your own **HUD Name** and **HUD Image**, or another status's **HUD Icon**.
- Weapons can borrow more parts from other weapons, and an appearance from another weapon no longer changes how the weapon fires. **Behavior** and **Type Markers** on **Gameplay**, and **Animations** on **Appearance**, can each come from another weapon and reset to the base with one click.
- **Firing Behavior**, **Barrel**, and **Magazine** on **Gameplay** can each come from a different weapon and no longer need Experimental Features.
- **Actions** under **Animations** borrow single actions, such as **Hip Fire** or **Holster**, from another frame. **Reload Animation** can borrow other reloads on compatible rigs, including Submachine Guns, Grenade Launchers, and Bows.
- A weapon with another type's appearance now takes that type's name, Collections page, and HUD icon. A **Keep … Type** checkbox on **Appearance**, such as **Keep Scout Rifle Type**, keeps the base weapon's.
- **Appearance** warns when a Sword appearance is used on another weapon type, or when a model will play the base weapon's animations.
- **Rounds Per Minute** warns when a runtime from another weapon type makes the weapon fire at that type's rates.
- **Placement** on **Appearance** moves the model in the hand, and **Markers** moves named points such as the sights and muzzle.
- **Advanced Gameplay** is now **Gameplay**, with borrowed parts under **Parts** and technical controls in a closed **Technical** section.
- The runtime donor picker sorts donors into **Lower Risk**, **Experimental**, and **Rejected**. Experimental donors need **Accept Crash Risk**.
- **Technical Build** shows borrowed parts, the rig, moved markers, runtime and animation sources, and where damage comes from. It flags when the recipe has changed since the build.
- Leaving a recipe with unsaved changes now offers to save them.
- More perk properties have names instead of numbers, such as **Damage**, **Blast Radius**, and **Magazine Size**.
- The workbench adds **Perk Diagnostics…** and **Gameplay Verification…**. Gameplay Verification keeps your in-game test results with the perk you tested.
- **Build & Stage** warns before a build would remove installed items and offers **Add to Build**.
- Rebuilds are faster. A build reuses the compiled data of each weapon whose recipe hasn't changed, so after editing one weapon it skips most of the work for the others.
- Parhelion uses less memory when browsing artwork and the recipe library. Icons load as their rows come into view.
- Model previews render far more accurately, though they aren't perfect yet. See the Sundial section.
- **Choose Ornament** has type, damage, ammo, and rarity filters, and opens on the base weapon's type when it has no ornaments of its own.
- **Find All Uses** in the **Engine Catalog** lists everything that uses a resource.

### Fixes

- Fixed build and install progress finishing stages early or going past its total.
- Fixed **Runtime Values** showing **Reset to Donor** beside unchanged values.
- Fixed the game sometimes freezing when inspecting an authored Subclass that takes abilities from other Subclasses.
- Fixed weapons with another type's appearance firing like that type, such as a Scout Rifle with a Pulse Rifle look firing three-round bursts, or a Hand Cannon with a Sidearm look firing twice as fast.
- Fixed weapons with another type's appearance holding the support hand in the wrong place, with parts drifting in third person.
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
- Fixed Dawn Collections sync adding a duplicate unlock row, which made Sundial refuse the account.
- Minor UI fixes.

## Sundial

- The catalog cache is about 50 times smaller, so Sundial starts faster and uses far less memory. The first start after updating rescans the installation once.
- Inspection caches are now compressed, using less disk space.
- The perk picker now has six scopes (up from five), from narrowest to widest. For example, on an Auto Rifle's barrel socket they read:
  - [Item name] Barrels: the perks this item lists for the socket.
  - Auto Rifle Barrels: barrels used on any Auto Rifle.
  - All Barrels: barrels from any item.
  - Auto Rifles: perks from any socket on any Auto Rifle.
  - All Weapons (or All Armor): perks from any socket on any weapon or armor piece.
  - All: every perk.
- The **Inspector** home page browses every item without a search.
  - **Weapons**, **Armor**, **Cosmetics**, **Perks & Mods**, **Other**, and **Recent** tabs show items as cards in their rarity's color, grouped by type.
  - Filter by **Type**, **Rarity**, and **Class**, sort by type, name, or rarity, and show **Dummy Items**.
  - Search narrows every tab, finds records, objectives, and other definitions, and opens a pasted hash.
  - Hovering a card shows the game's item tooltip, with Power, damage type, and ammo or energy. Right-click a card to **Open** it, **Add To** a character or the profile, open its **Model Preview**, or copy its name or hash.
  - The inspector opens larger.
- Characters can now hold bounties, quest steps, engrams, and other non-equipment items. A quest added this way may not start at its first step.
- Model previews in Sundial and Parhelion are much more accurate and can show many more models, though they aren't perfect yet.
  - More models load with their textures and lighting, including props and map pieces. Terrain can now be previewed too.
  - Shaders, materials, and glow look much closer to the game, including painted and worn surfaces, reflections, decals, and line effects.
  - Transparent and glowing effects, such as ornament sights and energy, blend over the model instead of drawing as solid surfaces.
  - Armor, Ghost Shells, Ships, and Sparrows take the right shader colors, and their chest glow and other effects show.
  - Finer detail survives at its correct texture scale, such as normal maps on mirrored parts and partly filled texture sheets like One Thousand Voices' fins.
  - Distant patterns and normal-map detail follow the source texture filtering and mip levels on supported gear.
  - Cloth shows in its stored pose, including capes, Titan marks, and Warlock robes.
  - Animations play every stored clip format, move the right joints, stay correctly lit, and keep the model in frame.
  - Very large models no longer lose parts, and appearances no longer pick up stray particle parts.
  - **View > Particle Study** shows approximate particle previews.
  - Previews load in the background, so the rest of the app stays responsive.
  - The preview window opens larger, and **Details** opens its own window with the **Assets and Effects** list.
  - Right-click an item and choose **Model Preview** to see it with its saved ornament and shader.
  - Exported models keep these colors and materials without hairline cracks. Very large or densely textured models now export correctly.
  - The background color picker no longer closes on the first click.

### Fixes

- Fixed the perk picker showing other weapon types' perks when scoped to the item's own type.
- Fixed wider perk picker scopes listing shaders, ornaments, and trackers on sockets that aren't cosmetic.
- Fixed quests, bounties, currencies, and consumables in a character's inventory reading as not valid for the character.
- Fixed swapping a Subclass offering every item instead of other Subclasses.
- Fixed Dawn settings refusing to save when a character held more than 135 unequipped items.
- Fixed Dawn saves overwriting changes another tool made after the account was loaded. Sundial now asks you to reload.
- Fixed Dawn vendor edits deleting data other tools linked to those vendors.
- Fixed **Armor Stats** running several searches at once when goals changed quickly.
- Fixed the Linux installer writing through a link already at the launcher's location.

Sundial v0.4.1 and newer can update to v0.5.3 in the app. Older releases should install v0.5.3 manually.

Parhelion remains experimental. Authored abilities, custom perks on abilities, Emblems, new gear, vehicles, Subclasses, appearances from other weapon types, and borrowed perk behavior still need in-game testing, and some combinations may crash. Report issues on [GitHub](https://github.com/KyleThmpsn/sundial/issues), on Discord, or on Twitter/X [@KyleThmpsn](https://x.com/KyleThmpsn). For Parhelion issues, include the recipe and describe what happens in game.

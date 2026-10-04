Sundial v0.5.3 lets Parhelion fully author Subclass abilities and Emblems, borrow more parts from other weapons, and give custom perks more control over abilities. Sundial itself is lighter and has a more capable model preview.

Read the [Parhelion README](crates/parhelion/README.md) for the workbench, bundled examples, and build and install instructions. Find previous release notes on the [releases page](https://github.com/KyleThmpsn/sundial/releases).

## Parhelion

- Subclass abilities can now be authored, not only swapped.
  - Give any grenade, melee, class ability, movement ability, Super, or attunement node its own name, description, icon, and perks, including custom perks from the **Custom Perk Workbench**.
  - Right-click one of its stock perks and choose **Edit as Custom Perk…** to change that perk for this ability alone.
  - Add up to four extra **Charges**, change what an ability or node affects under **Modifiers**, and adjust an ability's own values, projectiles, and effects under **Tuning**, which lists values with plain names, such as a projectile's **Initial Speed**, first.
  - **Effect Colors** recolors an ability's effects. Take each color palette's colors from another ability with **Colors From**, then turn its hue, saturation, or brightness. Only that ability changes, so other abilities with the same colors keep theirs.
  - A Subclass's HUD colors can't currently be changed. The game sets them from the damage type of the Subclass's Super.
  - Some changes may not work properly yet, and ability authoring will improve in future releases.
- Authored Subclasses can now be used without any in-game class restriction. Turn on **Every Class** to give a Subclass to every character and let any of them equip it. Its default label becomes **Guardian Subclass**, and **Custom Item-Type Label** can give it your own label.
- Subclasses have an **Appearance** tab with **Screen Art**, the full-screen character picture the subclass screen shows for each attunement. Take any picture from another subclass or import your own, and export any of them as a PNG. Untested in game.
- Armor and other gear can do more.
  - Armor, Sparrows, Ships, and Ghost Shells can add sockets, remove the base's sockets, or give a socket another role, as weapons can. Armor's energy sockets stay with **Energy Type** and **Energy Capacity**.
  - Gear pages show a preview of the item's model beside its details. Hover it and click the corner icon to open it in the full model viewer.
  - Armor has a **Class** picker beside **Rarity**. It follows the base by default, or can use **Titan**, **Hunter**, **Warlock**, or **Any Class**. The choice changes equip eligibility, Collections placement and class badge membership.
  - Authored armor appears in a **Project Sunrise** or **Dawn** category under **Collections > Armor** for each supported class. Its numbered **Armor Set I**, **Armor Set II** rows hold up to five pieces each, fixing categories that appeared but did not show their armor when selected.
  - Exotic armor appears exclusively under **Exotic > Armor > Class**. Ordinary and Exotic armor join each supported class's project badge, whose completion counts only that class's items.
- Parhelion can now author Emblems, with your own banner, overlay, and background images and nameplate colors.
  - **Stat Trackers** lets an emblem follow its base, allow all native trackers, or allow selected categories.
- Custom perks can do more with abilities.
  - **On a Specific Ability** and **Ends on a Specific Ability** can name any Subclass ability.
  - **Change an Ability Property** can set or add to an ability's own values, such as a grenade's blast radius, with **Add Property…**.
  - Extra melee and class ability charges now work on more abilities.
  - Devour, invisibility, and other attached effects can be given their own **Attachment Length**.
  - An attached effect that shows a status on the HUD, such as Arc Shield, can show your own **HUD Name** and **HUD Image**, or another status's **HUD Icon**.
- Weapons can borrow more parts from other weapons, and an appearance from another weapon no longer changes how the weapon fires. On **Gameplay**, **Behavior** and **Type Markers** each take theirs from another weapon, and on **Appearance**, **Animations** does the same. Type Markers and Animations list the types and animation profiles available rather than every weapon. Each row shows where the part comes from and goes back to the base with one click.
- **Rounds Per Minute** in **Weapon Stats** shows a warning when a runtime swapped in from another weapon type makes the weapon fire at that type's rates.
- **Placement** on **Appearance** moves the weapon model forward, sideways or up in the hand, which shows most in first person, and its **Markers** show the named points on the weapon model, such as its sights and muzzle. Double-click a marker to select it, then drag it, nudge it with the arrow keys, or type a distance.
- **Advanced Gameplay** is now **Gameplay**. Its **Parts** gather every part taken from another weapon, and the technical runtime, perk and inventory controls sit in a **Technical** section that stays closed until opened. The Weapon tab notes which parts come from other weapons and links to them.
- **Firing Behavior**, **Barrel**, and **Magazine** on **Gameplay** can each come from a different weapon, and no longer need Experimental Features. **Reload** still swaps in the other weapon's whole runtime.
- **Actions** under **Animations** on **Appearance** play single actions, such as **Hip Fire**, **Aim Fire** or **Holster**, from another frame's animations while the rest follow **Animations**. Only actions the frames play differently are listed.
  - **Reload Animation** can now borrow another profile's reload animations on compatible rigs, including Submachine Guns, Grenade Launchers and Bows.
- **Technical Build** lists each borrowed part, which rig the weapon uses, and every moved marker.
- The **Gameplay** runtime donor picker sorts donors into **Lower Risk**, **Experimental**, and **Rejected**. Applying an Experimental donor asks you to **Accept Crash Risk** first.
- Leaving a recipe with unsaved changes now offers to save them, not only to discard them.
- More perk properties have names instead of numbers, such as **Damage**, **Blast Radius**, and **Magazine Size**.
- The workbench adds **Perk Diagnostics…**, which lists every problem it finds in a perk, and **Gameplay Verification…**, which keeps your in-game test results with the exact perk you tested.
- **Build & Stage** now warns before a build would remove installed items and offers **Add to Build**.
- Rebuilds are faster. A build reuses the compiled data of each weapon whose recipe hasn't changed, so after editing one weapon it skips most of the work for the others.
- Parhelion uses less memory when browsing artwork and the recipe library. Icons load as their rows come into view.
- Model previews, including the full model viewer window, render far more accurately, though they aren't perfect yet. See the Sundial section for details.
- **Choose Ornament** has weapon type, damage, ammo, and rarity filters, and opens on the base weapon's type when it has no ornaments of its own.
- **Find All Uses** in the **Engine Catalog** lists every resource that uses the one shown.

### Fixes

- Fixed build and installation progress marking stages complete too early, and build progress sometimes exceeding its total.
- Fixed **Runtime Values** showing **Reset to Donor** beside values that were never changed.
- Fixed the game sometimes freezing when inspecting an authored Subclass that takes abilities from other Subclasses.
- Fixed a weapon with an appearance from another weapon type always firing like the appearance's weapon, such as a Scout Rifle with a Pulse Rifle's look firing three-round bursts. Set **Animations** to the base weapon to keep its own firing.
- Fixed a weapon with an appearance from another weapon type firing at that type's rates, such as a Hand Cannon with a Sidearm's look firing about twice as fast. Its stats now convert as the base weapon's type.
- Fixed a weapon dealing Kinetic damage while showing its damage type when the socket that holds that damage type, such as Nature of the Beast's **Weapon Mod** socket, was given another role or removed. The weapon now keeps its damage type, or the one chosen under **Damage Type**, without that socket.
- Fixed **Restore Donor Row** keeping custom perks the restored socket no longer offers.
- Fixed weapons you already own keeping a socket's old default perk after a rebuild replaced that default with a custom perk, such as a renamed intrinsic. Installing now moves them to the new default.
- Fixed a renamed custom perk, such as a renamed intrinsic, sometimes showing its base perk's name and description in item tooltips while the inspect screen showed your own.
- Fixed authored weapons with an Exotic base, an Exotic appearance, or an ornament refusing shaders when no shader was chosen before building.
- Minor UI fixes.

## Sundial

- The catalog cache is about 50 times smaller, so Sundial starts faster and uses far less memory. The first start after updating rescans the installation once.
- More inspection caches now use compressed storage, reducing disk use while keeping existing cached discoveries readable.
- The perk picker now has six scopes (up from five), from narrowest to widest. For example, on an Auto Rifle's barrel socket they read:
  - [Item name] Barrels: the perks this item lists for the socket.
  - Auto Rifle Barrels: barrels used on any Auto Rifle.
  - All Barrels: barrels from any item.
  - Auto Rifles: perks from any socket on any Auto Rifle.
  - All Weapons (or All Armor): perks from any socket on any weapon or armor piece.
  - All: every perk.
- Model previews in Sundial and Parhelion are much more accurate and can show many more models, though they aren't perfect yet.
  - The preview reads the game's own vertex formats, so far more weapons, armor, props, and map pieces load with their textures and lighting, and terrain can now be previewed.
  - Shaders and materials look closer to the game. Colors and textures are filtered and lit more accurately, glowing panels use proper exposure, and animated materials run much more of their in-game math.
  - Armor, Ghost Shells, Ships, and Sparrows take the right shader colors, and models with many visual effects keep their shader detail textures.
  - Transparent and glowing effects, such as ornament sights and energy, blend over the model instead of drawing as solid surfaces.
  - More armor, weapon, Ghost Shell, Ship, and Sparrow effects can be previewed, including textured glow, chest glow on armor and ornaments, reflections, and animated effect geometry.
  - Cloth now shows, including capes, Titan marks, and Warlock robes, in its stored pose.
  - Animations play every stored clip format, move the right joints, stay correctly lit as the model moves, and keep the model in frame.
  - Ordinary model previews keep their detail during playback. Software rendering follows display density within a fixed frame budget.
  - Approximate particle previews are available through **View > Particle Study**. Models no longer show guessed sparks automatically.
  - Imported weapon glow supports more material color outputs and keeps usable sampler bindings when unused resources follow them.
  - Imported previews keep moving solid parts aligned with their glow and show more body colors and texture patterns.
  - Previews load and render in the background, so the rest of the app stays responsive.
  - The preview window opens larger, and **Details** now opens its own window with the **Assets and Effects** list, which is much easier to browse.
  - Right-click an item and choose **Model Preview** to see it with its saved ornament and shader.
  - Hairline cracks along model edges are gone from exported images and previews drawn without the GPU, and exported models no longer fail validation at 65,536 vertices or lose normal direction at extreme scales.
  - The background color picker no longer closes on the first click.

### Fixes

- Fixed the perk picker showing other weapon types' perks when scoped to the item's own type.
- Fixed the perk picker's wider scopes listing shaders, ornaments, and trackers on sockets that aren't cosmetic.
- Fixed quests, bounties, currencies, and consumables in a character's inventory reading as not valid for the character.
- Fixed swapping a Subclass offering every item instead of other Subclasses.
- Fixed Dawn settings refusing to save when a character held more than 135 unequipped items.

Sundial v0.4.1 and newer can update to v0.5.3 in the app. Older releases should install v0.5.3 manually.

Parhelion remains experimental. Authored abilities, custom perks on abilities, Emblems, new gear, Subclasses, appearances from other weapon types, and borrowed perk behavior still need in-game testing, and some combinations may crash. Report issues on [GitHub](https://github.com/KyleThmpsn/sundial/issues), on Discord, or on Twitter/X [@KyleThmpsn](https://x.com/KyleThmpsn). For Parhelion issues, include the recipe and describe what happens in game.

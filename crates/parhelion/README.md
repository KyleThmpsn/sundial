# Parhelion

Parhelion is an **experimental** weapons workbench bundled with [Sundial](../../README.md) for Project Sunrise and Dawn. Build your own Destiny weapons by mixing weapon types, perks, stats, and appearance, including Exotics and combinations that wouldn't normally exist in the sandbox.

Weapons are saved as **recipes** that you can edit and share. Some combinations may not work or may crash the game, so test your weapons in-game after installing them.

See Sundial's [Compatibility](../../README.md#compatibility) section for supported game and runtime versions. Before using future Dawn versions with online support, uninstall custom packages or Dawn may report an error.

## Contents

- [Getting Started](#getting-started)
- [Making Weapons](#making-weapons)
  - [Stats and Power](#stats-and-power)
  - [Perks and Sockets](#perks-and-sockets)
  - [Custom Perk Workbench](#custom-perk-workbench)
  - [Common Effects](#common-effects)
  - [Conditions and Operands](#conditions-and-operands)
  - [Organizing Effects](#organizing-effects)
  - [Engine Catalog](#engine-catalog)
  - [Unique Weapon Behavior](#unique-weapon-behavior)
  - [Ornaments, Icons and Shaders](#ornaments-icons-and-shaders)
  - [Collections Placement](#collections-placement)
- [Bundled Custom Weapons and Perks](#bundled-custom-weapons-and-perks)
- [Recipes and Sharing](#recipes-and-sharing)
- [Generated Packages](#generated-packages)
- [Backups and Recovery](#backups-and-recovery)
- [FAQ](#faq)

## Getting Started

1. In Sundial, open **Preferences > Editing**. Under **Experimental**, turn on **Enable Parhelion Weapon Workbench**, then click **Open Parhelion**.
2. Choose **New Recipe**, or open a library weapon and use **Recipe… > Duplicate**. Pick a base weapon, give yours a name, and choose its perks, stats, and appearance.
3. Save your recipe. Click **weapons selected for build…** at the bottom, select every weapon you want installed, then click **Apply Selection**.
4. Click **Build & Stage**. When the build finishes, click **Review**.
5. Check the install location and any listed removals. Close Destiny and any other apps editing the same account, then confirm the installation.
6. Relaunch Destiny and get your weapons from Collections. Look under their weapon type, or under **Exotics** for Exotic weapons.

Each install replaces the previously installed Parhelion overlay package set. Select every weapon you want to keep, not just the weapons you've changed.

If the new build leaves out an installed weapon or custom perk, Parhelion lists what needs to be removed from your selected account. This can include equipped weapons, inventory items, saved perk selections, Collections unlocks, and reward rules for any character in that account. Review the list, then click **Back Up, Remove & Install** to continue or **Cancel** to make no changes. Other accounts aren't changed.

Parhelion checks that your selected weapons fit within the game's package limits before installation.

Check the build selection before each build. Opening a recipe doesn't automatically select it. **Include default Parhelion weapons** is checked by default. Leave it checked to include all bundled weapons, or uncheck it to choose them individually.

## Making Weapons

Your weapon starts with the **base weapon**'s firing behavior, stats, and perks. The **appearance donor** determines the weapon's in-game model and iconography. The picker opens on your base weapon's type, and you can clear that filter to use any weapon's appearance. Parhelion warns you when the two types differ, because the model may not render correctly, the build may fail, and the game may crash. A different inventory slot or an incompatible animation set is still refused.

Most editing happens on **Weapon** and **Appearance**. **Advanced Gameplay** contains experimental controls that are more prone to bugs and crashes and aren't yet recommended for normal use. **Identity** shows the weapon's hex hash identifiers for reference.

You can choose the weapon slot, damage type, and ammo type separately. The editor shows any restrictions. Changing ammo type doesn't rebalance the weapon's stats. Adjust those separately as needed.

Changing an Exotic base weapon to a non-exotic rarity removes its exotic equip restriction. Choosing Exotic rarity applies the normal exotic weapon restriction. Other unique-equip restrictions are preserved.

Installing unlocks your weapons in Collections and, by default, adds them to the **Project Sunrise** or **Dawn** badge for your active runtime. Get a copy from Collections to add it to your inventory.

You can also write flavor text and a custom **Lore** tab on **Weapon**. Leave both lore boxes off to keep the donor's lore, turn on **Custom Lore Tab** to write your own, or turn on **No Lore Tab** for a weapon that should have no lore entry at all.

### Stats and Power

**Weapon Stats** lists the values saved on your weapon. **Raw Value** is what gets written. **Preview** shows how the game presents it, such as rounds per minute. A stat you add only shows up in-game if the weapon and its stat group support that stat, so check the preview rather than assuming. A raw value of 30 on a rate of fire stat, for example, is not 30 rounds per minute. The preview shows what the game will actually display.

**Stat Options** holds the less common controls. **Show Internal Stats** reveals package-level rows such as Attack and Power along with unnamed ones. Hidden rows are kept either way, so turning it off doesn't discard anything. Use the reset command to put every value back to the base weapon.

**Power Cap** sets the weapon's infusion limit. Current Power is a separate thing and is edited in Sundial, not here.

### Perks and Sockets

A **socket** is a slot for a perk or mod. The game calls these perks and mods **plugs**. Click a perk to replace it, or use **+ Add Choice** to add another option to that socket. Only one choice per socket is active at a time. Drag a choice by its handle to reorder it, or right-click it to make it the default. The first choice starts equipped. Dragging onto a choice in another socket replaces that destination choice and keeps the original in place.

Use a socket's **… > Remove Socket** command to remove its choices and custom perk assignments. Other sockets keep their positions. For a removed base socket, click **Restore Socket** to bring back the base weapon's choices and role. Added sockets must be removed from last to first.

### Custom Perk Workbench

A **custom perk** is one you author yourself rather than borrow from another weapon. Open **Custom Perk Workbench…** from the main Parhelion menu, or click **Use Custom Perk…** above the socket list. Use **New Perk** in the workbench or **Create Custom Perk…** in the socket picker to start from scratch. You can also reopen an existing perk for editing.

Each effect is a small program that describes what starts it, what it does, and when it ends. You can use a stock effect exactly as the game has it, take one apart and change how it works where Parhelion can read it well enough, or start from scratch by combining triggers, conditions, actions, projectiles, player effects, and more. Guided forms help configure supported behavior, while the node canvas shows how the parts connect. Behavior Parhelion has mapped appears as named controls, including health and shield capacity and regeneration, invisibility, incoming damage modifiers, and component properties. Behavior that is not mapped yet keeps the engine's own names under **Advanced**. **Engine Catalog…** browses the behavior in the perks your game already has, covered under [Engine Catalog](#engine-catalog).

Match the perk's **Type** to its socket. Use **Trait** for normal perk columns and **Intrinsic** for the weapon's intrinsic frame.

To create and use a custom perk:

1. Open **Custom Perk Workbench…** from the main menu, or **Use Custom Perk…** above the socket list, then choose **New Perk**. You can also open a saved or bundled perk in the workbench. To start from a specific socket choice, click that choice and use **Use Custom Perk… > Create Custom Perk…**.
2. Give the perk a name and description, then use **Change Icon…** if you want different artwork.
3. Build effects from scratch or reuse stock behavior, then configure how each effect starts, acts, and ends. Stock examples show where the same behavior appears in the game.
4. Click **Save to Library** to keep the perk under **Custom Perks** for reuse.
5. Under **Weapon Socket**, choose the socket and choice you want to use, then click **Apply to Weapon** and save the weapon recipe.

Saving a perk to the library and applying it to a weapon are separate. **Save to Library** updates the reusable perk document. **Apply to Weapon** copies the current perk into the selected socket choice, and later library edits do not change that weapon copy automatically. Applying a perk does not save the weapon recipe for you. Use **Save as New Perk** when you want to experiment without replacing the saved original.

Parhelion includes several [bundled custom perks](#bundled-custom-perks) that are ready to inspect and use in your own recipes. A custom perk is included in a build only when a selected weapon uses it. Once the packages are installed, it appears like any other plug in Sundial's plug picker. As usual, which plugs appear depends on your plug selection mode.

To add a saved custom perk to another weapon:

1. Open the weapon you want to edit, then click the socket choice you want to replace. Use **+ Add Choice** first if you want another option.
2. Choose **Use Custom Perk…**, then select a perk from the picker.
3. Save the recipe.

The picker includes saved custom perks and perks embedded in your weapon recipes. It copies the selected perk's settings into this recipe. The source weapon isn't changed and doesn't need to be included in the build.

Use **Duplicate**, **Import…**, and **Export…** in the custom perk library to reuse or share perk documents. **Copy Test Plan** copies a checklist to your clipboard based on the perk's triggers, actions, and lifetime. Use it to guide your in-game testing. **Restore Default Custom Perks…** replaces edits to the bundled examples and restores any that are missing. Changed defaults are backed up first, while your other custom perks and weapon recipes are left alone.

A perk can only do what the weapon it sits on supports. If a perk depends on a reload, a magazine, or a firing behavior your weapon doesn't have, it does nothing. [Unique Weapon Behavior](#unique-weapon-behavior) covers the other half of that problem, where the behavior lives in the weapon rather than the perk.

Custom perk authoring is still in early development. Destiny's perk system contains thousands of technical building blocks across actions, conditions, values, projectiles, assets, and weapon-specific behavior. Those pieces can be arranged in effectively countless combinations. Parhelion maps that technical data into controls you can use, but the work is ongoing. More behavior will be added as it is understood, and some names, assumptions, or mappings may turn out to be incomplete or wrong.

A build that succeeds only proves the packages are put together correctly, not that every combination behaves as expected in game. A perk that looks right in the editor can still do nothing, behave differently, or crash the game. Try one new combination at a time, use **Copy Test Plan** to check each behavior, and keep a working recipe before experimenting further.

### Counters and Stacking Perks

A perk that needs several events before it fires, three kills say, is built on the effect's counter. Choose **When the Effect's Counter Is Reached (Accumulator)** as the trigger (search for *counter*), set **Count Needed**, then add **Contributing Conditions**. Each one is an event such as **On a Kill**, and each time it passes the counter goes up by the amount in its **Counter Change**, which starts at 1. **After It Fires** decides whether the count is kept, so the trigger stays satisfied, or starts over, the way stock perks that fire every few kills behave. A counter with no contributing conditions can never fire, and the workbench says so. **Set the Effect's Counter** is an action for writing the count directly.

### Common Effects

An effect is a trigger plus what it does when that trigger fires. These are some the workbench names in plain words:

- **Change Fired Projectile** swaps what the weapon shoots for something else entirely, such as another weapon's rounds, a missile, or an ability's projectile.
- **Spawn an Object or Effect** and **Attach an Effect** put something in the world or keep it on a target for the duration.
- **Generate Orbs of Light** drops a collectible orb at the kill or at the player, the way a Masterwork does.
- **Adjust Ammunition** and **Adjust Ammunition by Capacity** add or remove rounds, either a fixed count or a share of the magazine.
- **Reload from Reserves** refills the magazine without the reload.
- **Change Damage Type** switches the element the weapon deals.
- **Change a Weapon or Ability Stat** raises or lowers a stat while the effect runs.
- **Set a Weapon Firing Mode** changes how the weapon fires.
- **Change Ability Energy** scales grenade, melee, or class ability energy, with an optional limit.
- **Change an Ability Stat** changes a named stat inside an ability itself.

Pair one with a trigger such as **On a Kill**, **On Precision Weapon Kill**, **On Reloading**, **On Firing This Weapon**, **On Taking Damage**, or **On a Game Event** for something like an Orb of Light being picked up.

Many more are included. Anything the workbench can describe plainly gets a name like these, and the rest stays under **Advanced** with the engine's own name.

### Conditions and Operands

Conditions stack, and the workbench prints the operator between them.

**OR** runs down a list of alternatives, and any one of them is enough. Every effect can take them, including one you start from scratch. Use **Add Alternative Trigger…** for another way in, **Add End Condition…** for another way out, and **Add Reactivation Condition…** for another way to start again. Each one comes from the same condition picker.

**AND** works differently. Requirement groups belong to the complete native form, so you see them when you open a stock effect that has them, or after you choose **Edit All Behavior…** on an effect of your own. There **Add Requirement** adds a group, and the effect runs only **When All Requirements Are Met**.

For a NOT, turn on a condition's **Invert Result**. It passes when its check is false, which is how a perk says while you are not Charged with Light rather than while you are. Not every condition offers it.

End conditions and reactivation conditions each take any one of their entries, so the first one that matches ends the effect or lets it start again.

Plenty of stock effects already use these stacks. Opening one and reading down its conditions is the quickest way to see how the game combines them before you build your own.

### Organizing Effects

Effects run in the order shown. Drag an effect by the grip at the top of its card to move it, or use **Move Up** and **Move Down** in the effect menu. The list scrolls while you drag, so a card further down the perk stays reachable. Actions reorder the same way inside an effect.

**Duplicate Effect** copies an effect so you can try a variation without losing the original. The copy gets its own identity, so editing it leaves the effect you copied alone.

**Undo** and **Redo** cover the edits you make during this session, including removing, reordering, and copying effects. Parameter editors keep their own **Apply** and **Discard**, and a text box keeps its own undo while you type in it.

Collapse the cards you are done with to keep a long perk readable. When validation fails, **Show Problem** opens the right card and scrolls to the action that needs fixing.

### Engine Catalog

**Tools > Engine Catalog…** is a technical browser for the behavior read out of the perks your game already has. Turn on **Enable Experimental Features** under **Preferences… > Editor & Library** if the entry is not there.

Choose a kind to see what it does and which installed perks use it. An entry shows its graph, how its components fit together, what links to it and what it links to, and the details of the scan that found it. Use **Authorable Only** to narrow the list to kinds you can put in a perk, or **All Kinds** to see everything the scan found.

Use it to find behavior worth copying into a perk of your own, or to check what a stock perk really does before you borrow from it. The catalog reads your installed packages as you browse, so use **Retry Scan** if a scan reports an error.

### Unique Weapon Behavior

Some Exotics keep part of what makes them special in the weapon itself rather than in a perk. Hard Light's bouncing rounds, Malfeasance's embedded rounds, and Borealis's damage switching all work this way. **Unique Weapon Behavior** on **Weapon** copies that half onto the weapon you are building. It sits with the damage type and the other weapon-wide choices.

Pick a source the same way you pick an appearance donor. Most sources are Exotics, with a few Legendaries such as Drang and Warden's Law. Hover **Unique Weapon Behavior** for the full description of your current choice, and watch for a caution under the row when a combination has known limits. Choose **None** to go back to your weapon's own behavior.

**Include Its Perks** is on by default. It adds the source weapon's Exotic trait to your weapon's matching socket, and when the source is the same type of weapon its intrinsic frame replaces your weapon's own, because several Exotics keep the other half of the behavior there. A frame is built for its own weapon type, so a weapon of a different type keeps its own frame; the picker and its tooltip show which perks will land. These perks appear in **Perks & Sockets** while you edit. Check the choices and defaults after selecting a behavior, especially if you have already customized those sockets. Turn it off to borrow just the weapon's runtime behavior.

You can try a behavior on a different weapon type than the Exotic it came from, but some combinations only partly work or do nothing. The picker warns about known dependencies, such as a frame that expects its own ammo or a perk that reads a scope the host weapon does not have.

Damage switching is set by the damage type rather than here. Choose **Variable (Hold Reload)** and your weapon gets The Fundamentals and the behavior that drives it, while keeping its own appearance. Choosing Hard Light or Borealis as a Unique Weapon Behavior does the same and locks the damage type, since those two switch damage as well as fire differently.

If a borrowed behavior launches projectiles and your base weapon normally fires instantly, those projectiles may travel very slowly. **Projectile Speed Multiplier** raises their launch speed, with the increase capped at the value you choose. Sources already above that value keep their own speed. The default is a starting point, so adjust it a little at a time and test in-game.

For example, to put Malfeasance's rounds on an ordinary Hand Cannon:

1. Start a recipe with the Hand Cannon you want as the base weapon.
2. On **Weapon**, open **Unique Weapon Behavior** and choose **Malfeasance**.
3. Leave **Include Its Perks** on. Malfeasance keeps the detonation in its perk, so the weapon half alone will not finish the job.
4. Save, build, install, and get a copy from Collections.

This is new and most combinations have not been tested, so treat it as experimental. Try one new combination at a time and test it in-game before adding more.

### Ornaments, Icons and Shaders

**Appearance** and the ornament, shader, and appearance donor pickers show a preview of the weapon model, with its textures, animations, and shader colors, so you can see a choice before you commit to it. Previews are not exact yet, so some models light or shade differently from how they look in game.

**Use Ornament** lists the stock ornaments your chosen appearance can wear. Picking one takes that ornament's model, its own colors, and its inventory icon. Everything it sets stays editable afterwards, and **Default Appearance** puts it back. The button sits beside the appearance picker on **Weapon** as well as on **Appearance**, and it only appears when the appearance you chose has ornaments.

On **Appearance**, use **Change Icon** to choose another weapon's icon. Use **Edit Icon…** to import an image, adjust or replace colors, rotate, or flip it. Use PNG for transparent artwork. Imported images are saved in the recipe, so you don't need to share them separately.

The background follows your weapon's rarity. Icon edits don't recolor the 3D weapon model, but you can use shaders and appearance choices for that.

Custom perks, badges, and watermarks share an icon browser. Choose from game assets, use **Add Icon…** to load your own, or choose **Download destiny-icons** for the optional [destiny-icons](https://github.com/justrealmilk/destiny-icons) collection. Imported artwork is stored with the recipe or perk document.

Use **Edit Artwork…** under **Release Watermark** on **Appearance** to customize the small watermark on the weapon's inventory icon. Custom badges have the same artwork editor on **Collections**. Both support crop, placement, rotation, and flipping, and badges also offer background colors and gradients.

Adding a shader choice lets it recolor supported dye channels, including channels normally locked by an Exotic appearance. The original colors remain when no shader is selected. Explicit render-dye overrides in Advanced Gameplay take precedence.

To change the small weapon icon beside the ammo count, use **Ammo HUD Icon > Import HUD PNG…** and choose a transparent PNG. This doesn't change the inventory icon. **Use Appearance** switches back to the donor's HUD icon. Rebuild and install to see your changes in-game.

### Collections Placement

**Collections** chooses where your weapon appears in the game's Collections. By default it uses the node matching your weapon's ammo type and its gameplay donor's weapon type, creating that node when the game has no stock one for the combination. Your weapon goes at the start of the node rather than beside its donor, so it is easier to find. Adding a page creates one shared page that uses the stock weapon-type name and icon. Exotic weapons always use the Exotics collection for their inventory slot.

Enable **Custom Badge** to group weapons under your own badge, with a name, description, and artwork. Use the same badge settings for each member, or select an existing badge with **Choose from Library**. You can also choose whether the weapon appears in the default Sunrise or Dawn badge.

Custom pages come out of a limited budget. The tab shows how many nodes you have used and how many remain, and it tells you when a build would go over. Remove a custom badge or an added page to get back under the limit.

Placement is presentation only. It does not change ammo, stats, or gameplay.

## Bundled Custom Weapons and Perks

Parhelion includes these examples to show a taste of what the workbench can do. Use them as references, inspiration, or starting points for your own creations. Open one to see how its donors, sockets, stats, presentation, and custom behavior fit together.

### Bundled Weapons

- **Hammer Time**: An Exotic Grenade Launcher that moves Wendigo GL3's Heavy frame into the Energy slot with Special ammo. Three custom trait choices switch between Solar hammers, massive Arc bolts, and Void Nova Bombs, each setting the damage type and rewarding final blows with ability energy or invisibility.
- **SUROS Renaissance**: SUROS Regime with Hard Light's ricocheting rounds, **Variable (Hold Reload)** damage switching, and Scatter Matrix's SIVA swarms on final blows. It combines borrowed Exotic behavior with a custom perk.
- **Ravenous Horizon**: A Special-ammo Auto Rifle that grafts Malfeasance's runtime behavior onto Gnawing Hunger, with Event Horizon creating a lingering Void anchor on precision final blows. It combines an Exotic's hidden behavior with a custom kill effect.
- **Redacted**: An Arc Special-ammo Rocket Sidearm with Interregnum XVI's appearance and Micro-Missile Frame. It shows how a custom intrinsic and advanced gameplay donors can create an entirely new archetype.
- **Reclamation Order**: An Exotic Void Sword in the Energy slot that uses Special ammo, combining Black Talon gameplay with Traitor's Fate appearance. It is the clearest example of moving a Heavy weapon family into an otherwise impossible loadout.
- **Vaultbreaker**: A Legendary Arc Shotgun that combines Prophet of Doom, The Fourth Horseman, and Micro-Missile Frame. It is a useful reference for grafting custom projectile behavior onto a conventional weapon family.
- **Second Sun**: A Legendary Solar Rocket Launcher that combines The Wardcliff Coil's salvo with Truth's appearance. It shows how Exotic gameplay and presentation donors can be separated and recast as a Legendary.
- **Dead Air**: An Exotic Solar Trace Rifle in the Kinetic slot, built from Coldheart. It shows how slot and damage type can be authored independently and expands the stock weapon with substantially more perk columns and choices.
- **Still Here**: A Solar Machine Gun moved into the Energy slot with Special ammo, based on Hammerhead. It demonstrates cross-slot and cross-ammo authoring on a Heavy weapon family.
- **Good Company**: A Legendary Arc Pulse Rifle built from Vigilance Wing, preserving its five-round burst while dropping the Exotic rarity. Its expanded perk columns show how a familiar weapon can become a configurable Legendary.
- **Stay**: A Legendary Kinetic Sniper Rifle combining Whisper of the Worm's gameplay with Alone as a god's appearance. It shows how Exotic weapon behavior can anchor a conventional-looking Legendary.
- **Periapsis**: A Legendary Solar Hand Cannon built from Ancient Gospel in Sunshot's shell. It demonstrates borrowing Exotic presentation while keeping a Legendary weapon and a custom trait mix.
- **June Ninth**: A Void Submachine Gun in the Kinetic slot, pairing The Recluse with Imminent Storm's appearance. It is a clean example of separating weapon slot, damage type, and visual donor.
- **Night Shift**: A Void Fusion Rifle moved into the Kinetic slot, based on Loaded Question. It highlights cross-slot authoring while retaining its native weapon behavior.
- **Unsent**: An Arc Special-ammo Bow with Hush gameplay and Le Monarque's appearance. It demonstrates changing ammo and element while keeping bow-specific behavior and perk choices.
- **Every End**: A Solar Auto Rifle that combines Arc Logic's gameplay with Foregone Conclusion's appearance and heavily tuned stats. It is a broad reference for changing presentation, element, stats, and perk choices together.
- **Holdover**: A Legendary Solar Scout Rifle with No Feelings gameplay and Polaris Lance's appearance. It demonstrates using an Exotic model independently from its gameplay.
- **Last Watch**: A Kinetic Shotgun pairing Threat Level with Perfect Paradox and an aggressive close-range perk suite. It shows how far a familiar weapon can be pushed without changing its weapon family.

### Bundled Custom Perks

These library examples are Traits. Redacted and Vaultbreaker use Intrinsic versions of Micro-Missile Frame in their frame sockets.

- **Borrowed Time**: Adds a grenade charge and grants grenade energy on final blows.
- **Chicken**: Final blows turn enemies into chickens.
- **Chromatic Instinct**: Precision, grenade, and melee final blows switch damage to Arc, Solar, and Void respectively.
- **Closed Circuit**: Final blows grant grenade energy. Grenade final blows move ammunition from reserves into the magazine.
- **Constellation**: Precision final blows have a chance to create an Orb of Light.
- **Deadeye Dividend**: Precision final blows return a round and overflow the magazine by one.
- **Event Horizon**: Precision final blows collapse the target into a lingering Void anchor.
- **Hammer of Sol Frame**: Fires explosive Solar hammers. Final blows grant grenade energy.
- **Loaded Dice**: Final blows periodically roll ammo drops, with Primary common, Special scarce, and Heavy rare.
- **Micro-Missile Frame**: Fires fast, straight-flying micro-missiles and increases movement speed while equipped.
- **Runaway Reactor**: Final blows stack damage, rate of fire, and reload speed bonuses.
- **Scatter Matrix**: Final blows release seven SIVA swarms from the target.
- **Scavenger's Rhythm**: Any final blow transfers ammunition from reserves into the magazine.
- **Storm Cannon Frame**: Fires massive Arc bolts. Final blows grant melee energy.
- **Void Nova Frame**: Fires high-velocity Void Nova Bombs. Final blows grant invisibility.

## Recipes and Sharing

Use **Recipe… > Export…** to share a recipe and **Import…** to open one you've received. Find your saved recipes under **Preferences… > Editor & Library > Open Recipe Folder**.

Use **Duplicate** to make a separate weapon. Edit the existing recipe to update a weapon you've already made.

Renaming a weapon also changes its in-game identifiers, so the game treats it as a different weapon. If the next build replaces the old weapon, installation asks you to confirm removal of any saved copies.

**Recipe… > Discard Changes** returns to the last saved version of the recipe. It doesn't undo an installation. Export a working recipe before experimenting if you want a copy to return to. To replace a recipe with an earlier version, follow [Restoring a recipe](#restoring-a-recipe).

You can edit any of the default weapons bundled with Parhelion too. Parhelion uses your saved version instead of the bundled version.

## Generated Packages

These are examples of files Parhelion generates. Your selected recipes determine which files are needed. The build result and manifest list them all. Keep the files from each build together, even if you only changed one weapon.

| Package File | Purpose |
| --- | --- |
| `w64_parhelion_assets_0aa0_0.pkg` | Holds new artwork and other assets used by your weapons, including Sunrise or Dawn badge and watermark assets. |
| `w64_investment_0361_7.pkg` | Connects weapons and perks to their in-game behavior. |
| `w64_investment_globals_client_058c_4.pkg` | Adds Collections entries, unlocks, and badge progression. |
| `w64_investment_globals_client_0593_4.pkg` | Registers custom items and connects them to their Collections entries. |
| `w64_investment_globals_client_0709_4.pkg` | Provides item and perk information shown in menus. |
| `w64_investment_globals_client_0913_4.pkg` | Stores weapon names, descriptions, and other custom text. |
| `w64_investment_globals_client_0914_4.pkg` | Defines custom items and connects them to their icons and appearance. |
| `w64_sandbox_01bb_7.pkg` | Adds custom perk behavior when a recipe needs it. |
| `w64_shared_manifest_0374_7.pkg` | Makes additional gameplay resources available when needed. |
| `w64_ui_037e_6.pkg` | Adds custom ammo HUD icons when a recipe uses them. |

## Backups and Recovery

Parhelion leaves stock package files intact and backs up the installed overlay package set before replacing it.

If installation also removes anything from your account, Parhelion backs up the account with the packages. These backups aren't deleted automatically.

Under **Preferences… > Builds & Backups**, choose where to save backups, how many to keep, and whether to back up recipes. By default, Parhelion keeps the latest **three** completed package backups for each game installation. It keeps incomplete backups and older backups it can't identify.

[Settings backups](../../README.md#backups-and-local-files) are separate. Restoring settings doesn't restore packages.

### Restoring Default Recipes

Save or discard open edits, then choose **Restore Default Recipes…** in the recipe library. Review the confirmation and click **Restore Defaults**. This restores every bundled recipe to the version included with your current Parhelion build, including missing defaults. Changed files are backed up first.

Custom recipes, build selection, and installed packages are kept. Rebuild and install to use the restored defaults in-game.

### Restoring a Recipe

1. Open **Preferences… > Editor & Library > Open Recipe Folder**, then close Parhelion.
2. Copy the current recipe somewhere outside the recipe folder as a backup. Replace it with your earlier exported copy, keeping the existing filename.
3. Reopen Parhelion and check the restored recipe. To use it in-game, select it for the build, rebuild, then close Destiny and install.

### Uninstalling

1. Close Destiny and any other apps editing the same account.
2. Open **Preferences… > Builds & Backups > Uninstall Custom Packages…**.
3. Leave **Remove Custom Items and Progression** checked to remove custom items, saved perk selections, Collections unlocks, and reward rules from the selected account. Equipped custom weapons are removed too, leaving those slots empty.
4. Review the removals, confirm, then click **Back Up & Uninstall**.

This removes the installed Parhelion overlay package set.

Stock items, unrelated progress, other settings, and recipes are kept. Other accounts aren't edited. If cleanup is off or unavailable, remove the custom items and their unlock data in Sundial yourself before uninstalling.

If you leave account cleanup checked, the backup includes both the packages and your account before removal. This backup isn't deleted automatically.

### Recovering an Interrupted Installation

With Destiny closed, open **Preferences… > Builds & Backups** and choose **Recover Interrupted Operation**. This also handles interrupted uninstalls. If recovery fails, keep the complete backup and build manifest and ask for help. Don't try to repair the installation by deleting stock packages or mixing builds.

## FAQ

### Why Doesn't This Perk Work on My Weapon?

Perks can depend on a specific reload action, magazine size, or weapon behavior. Some perks won't work if your weapon doesn't have those mechanics. When the missing piece is the weapon rather than the perk, [Unique Weapon Behavior](#unique-weapon-behavior) can copy it across.

### Why Aren't My Changes Showing?

Make sure you saved the recipe, selected it for the build, and installed the new packages with Destiny closed. Fully relaunch the game afterward.

Existing weapons may keep their selected perks. Get a fresh copy from Collections to check new defaults. If a weapon is missing from Collections, check the install report for an unlock error even if the packages installed successfully.

### Where Are the Advanced Controls?

Enable them under **Preferences… > Editor & Library**, then open **Advanced Gameplay**. Advanced controls are not yet recommended for normal use, as some combinations may freeze or crash the game. Please use with caution.

Turning them off hides the controls without removing your saved changes.

### Why Doesn't the Borrowed Behavior Do Anything?

Some behavior is split between the weapon and its perk. Check that **Include Its Perks** is on, then get a fresh copy from Collections so the new perks are selected. If the behavior still does nothing, that combination may simply not carry over. Hover **Unique Weapon Behavior** to see whether that source has been confirmed in-game.

A weapon fires through one behavior at a time, so two sources that replace the projectile cannot both apply. Thorn's poison and Lumina's Noble Rounds are an example. Pick the one you want.

### What Should I Include in a Bug Report?

Include the recipe, what you did, what happened, and the versions of Sundial and your runtime (Sunrise or Dawn). Add any error messages or screenshots. Use **Preferences… > Activity Log… > Copy Log** for recent events, or **Open Log Folder** to find `parhelion.log`. For install or recovery issues, keep the matching backup and build manifest too.

See the [Sundial README](../../README.md) for bug reports, credits, and licensing.

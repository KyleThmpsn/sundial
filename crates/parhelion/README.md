# Parhelion

Parhelion is an **experimental** item workbench bundled with [Sundial](../../README.md) for Project Sunrise and Dawn. Build weapons by mixing gameplay, appearance, perks, and stats, including Exotics and unusual combinations. You can also author armor, Sparrows, Ships, Ghost Shells, Shaders, and Subclasses from stock items.

Your items are saved as **recipes** that you can edit and share. Some combinations may not work or may crash the game, so test them in-game after installing them.

See Sundial's [Compatibility](../../README.md#compatibility) section for supported game and runtime versions. Before using future Dawn versions with online support, uninstall custom packages or Dawn may report an error.

## Contents

- [Getting Started](#getting-started)
- [Making Weapons](#making-weapons)
  - [Stats and Power](#stats-and-power)
  - [Perks and Sockets](#perks-and-sockets)
  - [Custom Perk Workbench](#custom-perk-workbench)
  - [Finding Triggers, Actions, and Objects](#finding-triggers-actions-and-objects)
  - [Counters and Stacking Perks](#counters-and-stacking-perks)
  - [Common Effects](#common-effects)
  - [Conditions](#conditions)
  - [Organizing Effects](#organizing-effects)
  - [Engine Catalog](#engine-catalog)
  - [Unique Weapon Behavior](#unique-weapon-behavior)
  - [Ornaments, Icons and Shaders](#ornaments-icons-and-shaders)
  - [Collections Placement](#collections-placement)
- [Making Other Items](#making-other-items)
- [Making Shaders](#making-shaders)
- [Making Subclasses](#making-subclasses)
- [Bundled Custom Weapons and Perks](#bundled-custom-weapons-and-perks)
- [Recipes and Sharing](#recipes-and-sharing)
- [Generated Packages](#generated-packages)
- [Backups and Recovery](#backups-and-recovery)
- [FAQ](#faq)

## Getting Started

1. In Sundial, open **Preferences > Editing**. Under **Experimental**, turn on **Enable Parhelion Weapon Workbench**, then click **Open Parhelion**.
2. Click **New Weapon** or open the adjacent **New Item** menu to choose another item type. You can also open a library recipe and use **Recipe… > Duplicate**. Pick a stock base and edit the choices available for that item.
3. Save your recipe. Open **Items in This Build**, select every item you want installed, then click **Apply Selection**.
4. Click **Build & Stage**. When the build finishes, click **Review**.
5. Check the install location and any listed removals. Close Destiny and any other apps editing the same account, then confirm the installation.
6. Relaunch Destiny and find your authored weapons, gear, and Shaders in Collections. Weapons appear under their type, or under **Exotics** for Exotic weapons. [Making Other Items](#making-other-items) covers where the rest appear. Subclasses do not appear in Collections. Equip them in game like any other Subclass.

Each install replaces the previously installed Parhelion overlay package set. Select every recipe you want to keep, not just those you've changed.

If the new build leaves out an installed item or custom perk, Parhelion lists what needs to be removed from your selected account. This can include equipped items, inventory items, saved perk selections, Collections unlocks, and reward rules for any character in that account. Review the list, then click **Back Up, Remove & Install** to continue or **Cancel** to make no changes. Other accounts aren't changed.

Parhelion checks that your selected items fit within the game's package limits before installation.

Check the build selection before each build. Opening a recipe doesn't automatically select it. **Include Default Weapons** is checked by default. Leave it checked to include all bundled weapons, or uncheck it to choose them individually.

## Making Weapons

Your weapon starts with the **base weapon**'s firing behavior, stats, and perks. The **appearance donor** determines the weapon's in-game model and iconography. The picker opens on your base weapon's type, and you can clear that filter to use any weapon's appearance. Appearances from another weapon type can carry the appearance weapon's rig and animations when the two weapon types can share them. Otherwise, the model uses the base weapon's rig and some moving parts may remain still. The picker shows which result to expect.

Most editing happens on **Weapon** and **Appearance**. **Advanced Gameplay** contains experimental controls that are more prone to bugs and crashes and aren't yet recommended for normal use. **Identity** shows the weapon's hex hash identifiers for reference.

**Advanced Gameplay** swaps one gameplay component at a time, such as a weapon's **Firing Behavior**, for another weapon's. Donors of the same weapon type are listed first. **Show Experimental Matches** adds donors of other types, which Parhelion rewires to fit, a Hand Cannon's **Firing Behavior** on a Sidearm for example. **Show Rejected Donors** lists the ones it cannot fit and why.

You can choose the weapon slot, damage type, and ammo type separately. The editor shows any restrictions. Changing ammo type doesn't rebalance the weapon's stats. Adjust those separately as needed.

Changing an Exotic base weapon to a non-exotic rarity removes its exotic equip restriction. Choosing Exotic rarity applies the normal exotic weapon restriction. Other unique-equip restrictions are preserved.

Installing unlocks your weapons in Collections and, by default, adds them to the **Project Sunrise** or **Dawn** badge for your active runtime. Get a copy from Collections to add it to your inventory.

You can also write flavor text and a custom **Lore** tab on **Weapon**. Leave both lore boxes off to keep the donor's lore, turn on **Custom Lore Tab** to write your own, or turn on **No Lore Tab** for a weapon that should have no lore entry at all.

### Stats and Power

**Weapon Stats** lists the values saved on your weapon. **Raw Value** is what gets written, and **Preview** shows what the game displays, such as rounds per minute. A raw value of 30 on a rate of fire stat is not 30 rounds per minute. A stat you add only shows in-game if the weapon's stat group supports it, so check the preview.

**Stat Options** holds the less common controls. **Show Internal Stats** reveals package-level rows such as Attack and Power along with unnamed ones. Hidden rows are kept either way, so turning it off doesn't discard anything. Use the reset command to put every value back to the base weapon.

**Power Cap** sets the weapon's infusion limit. Current Power is a separate thing and is edited in Sundial, not here.

### Perks and Sockets

A **socket** is a slot for a perk or mod. The game calls these perks and mods **plugs**. Click a perk to replace it, or use **+ Add Choice** to add another option to that socket. Only one choice per socket is active at a time. Drag a choice by its handle to reorder it, or right-click it to make it the default. The first choice starts equipped. Dragging onto a choice in another socket replaces that destination choice and keeps the original in place.

Use a socket's **… > Remove Socket** command to remove its choices and custom perk assignments. Other sockets keep their positions. For a removed base socket, click **Restore Socket** to bring back the base weapon's choices and role. Added sockets must be removed from last to first.

### Custom Perk Workbench

A **custom perk** is one you author yourself rather than borrow from another weapon. Open **Custom Perk Workbench…** from the main Parhelion menu, or click **Use Custom Perk…** above the socket list. The window has three parts. **Custom Perks** on the left lists your saved perks and open drafts. The perk you are editing fills the middle. The footer applies it to a socket of the open item.

Each effect is a card that says what starts it, what it does, and when it ends. You can use a stock effect exactly as the game has it, change one in place, or build one from scratch by combining triggers, conditions, actions, objects, and more. Behavior Parhelion has mapped appears under plain names. Behavior that is not mapped yet keeps the engine's own names, reachable from each item's menu. **Engine Catalog…** browses the behavior in the perks your game already has, covered under [Engine Catalog](#engine-catalog).

To create and use a custom perk:

1. Click **New Perk**, or **New from Perk…** to start from a copy of a stock perk. To start from a socket choice, click that choice and use **Use Custom Perk… > Create Custom Perk…**. You can also open a saved or bundled perk from the list.
2. Name the perk. The row under the name holds its **Type**, its **Description**, and **Change Icon…**. Match **Type** to the socket: **Trait** for normal perk columns and **Intrinsic** for the weapon's frame.
3. Click **Add Effect** for an empty card, or **Add from Perk…** to copy a stock perk's effects. A perk with several effects adds them all. Open it in the picker to add one on its own.
4. On the card, choose the **Trigger**, click **Add Action…** for each action, and set **Timing**. **Duration** is how long the effect lasts and **Cooldown** how soon it can fire again. An effect nothing triggers has a **Repeat Interval** instead. A kill effect whose actions happen once reads **At Once**, so every kill fires it.
5. Click **Save to Library** (Ctrl+S) to keep the perk under **Custom Perks**.
6. In the footer, choose the socket and choice under **Weapon Socket**, or **Armor Socket** and so on for other items, then click **Apply to Weapon** or the matching button for your item type. Click **Save Recipe** afterwards.

Saving a perk and applying it are separate. **Save to Library** updates the saved perk. **Apply to Weapon** copies the current perk into the chosen socket choice, and later edits to the saved perk do not change that copy. Use **Save as New Perk** in the perk's menu to experiment without replacing the saved original. When a perk cannot be applied, the footer says why, such as an action with no object chosen, and **Show Problem** opens the card that needs it.

The workbench keeps open drafts between sessions, marked **Draft** until you save them. **Use Effect Summary** under the description writes a starting description from the effects. **Copy Test Plan** in the perk's menu copies an in-game checklist built from the perk's triggers, actions, and timing.

Use **Import…** and **Export…** in the library menu to share perk files. **Restore Default Custom Perks…** puts the bundled examples back and backs up any you changed.

To reuse a saved perk on another item, click a socket choice there and choose **Use Custom Perk…**. The picker also lists perks embedded in your recipes and copies the chosen one into this recipe. A custom perk is included in a build only when a selected item uses it. Once installed, it appears like any other plug in Sundial's plug picker.

A perk can only do what the weapon it sits on supports. If a perk depends on a reload, a magazine, or a firing behavior your weapon doesn't have, it does nothing. [Unique Weapon Behavior](#unique-weapon-behavior) covers the other half of that problem, where the behavior lives in the weapon rather than the perk.

Custom perk authoring is still early. Parhelion maps more of Destiny's perk system into plain controls over time, and some names or mappings may turn out to be incomplete or wrong.

A build that succeeds only proves the packages are put together correctly, not that every combination behaves as expected in game. A perk that looks right in the editor can still do nothing, behave differently, or crash the game. Try one new combination at a time, use **Copy Test Plan** to check each behavior, and keep a working recipe before experimenting further.

### Finding Triggers, Actions, and Objects

Every picker has a search box. Search by name, by what a behavior does, such as "reload" or "headshot", or by a stock perk that uses it. Press **Enter** to use the first result, or double-click a row.

**Sort: Suggested** puts everyday choices first, such as **On Weapon Kill**, **Change a Weapon or Ability Stat**, **Nova Bomb**, and **Rampage**. Sorting never hides a result. Each row shows what the behavior does and which stock perks use it.

### Counters and Stacking Perks

A perk that needs several events before it fires, such as three kills, uses the effect's counter. Choose **When the Effect's Counter Is Reached** as the trigger, set **Count Needed**, then click **Add Condition…** beside **Contributing Conditions** for each event that counts, such as **On Weapon Kill**. Each one adds 1 by default, which you can change under **Counter Change**. **After It Fires** chooses **Keep Counting** or **Start Over**, the way stock perks that fire every few kills behave. **Set the Effect's Counter** is an action that writes the count directly.

### Common Effects

An effect is a trigger plus what it does when that trigger fires. These are some the workbench names in plain words:

- **Change Fired Projectile** swaps what the weapon shoots for something else entirely, such as another weapon's rounds, a missile, or an ability's projectile.
- **Spawn an Object or Effect** and **Attach an Effect** put something in the world or keep it on a target for the duration.
- **Generate Orbs of Light** drops a collectible orb at the kill or at the player, the way a Masterwork does.
- **Adjust Ammo** and **Adjust Ammo by Capacity** add or remove rounds, either a fixed count or a share of the magazine.
- **Reload from Reserves** refills the magazine without the reload.
- **Change Damage Type** switches the damage type the weapon deals.
- **Change a Weapon or Ability Stat** raises or lowers a stat while the effect runs.
- **Set a Weapon Firing Mode** changes how the weapon fires.
- **Change Ability Energy** scales grenade, melee, or class ability energy, with an optional limit.
- **Change Incoming Damage** raises or lowers the damage you take while the effect runs, the way Riven's Curse does.
- **Change an Ability Stat** changes a named stat inside an ability itself.

Pair one with a trigger such as **On a Kill**, **On Precision Weapon Kill**, **On Reloading**, **On Firing This Weapon**, **On Taking Damage**, or **On a Game Event** for something like an Orb of Light being picked up.

Many more are included. **Add Action…** also offers presets such as **Fire at Full Auto**, **Hold to Charge**, and **Rampage's Stacking Damage**, which add all the actions the stock perk uses in one step.

### Conditions

Use **Or…** beside a condition to add an alternative, so any one of them can pass. Use **And…** to add a requirement that must also pass. A requirement's menu sets **Stays Met For**, how long its condition counts as passed.

Use **Not** on a condition to reverse its result, for example to run while you are not Charged with Light. Not every condition offers it.

**More Conditions** adds an **End Condition**, which stops the effect, or a **Reactivation**, which lets it start again. Without them, the card shows only the **Timing** row. If an effect could only ever fire once, the card says so under its trigger and **End at Once** fixes it.

Some conditions check a state instead of waiting for an event: nearby enemies or allies, your subclass, how full the magazine is, melee energy, or whether your Super is active. Each starts with the comparison a stock perk uses, such as three or more enemies nearby, and you can change it. The **State** list can be searched and leads with everyday states such as **Aiming Down Sights** and **Reloading**.

Plenty of stock effects already use these stacks. Opening one and reading down its conditions is the quickest way to see how the game combines them before you build your own.

### Organizing Effects

Effects run in the order shown. Drag an effect by the grip at the top of its card, or use **Move Up** and **Move Down** in the effect menu. Actions reorder the same way inside an effect.

**Duplicate Effect** copies an effect so you can try a variation without touching the original.

**Add Behavior Group** in the effect menu adds a second trigger with its own actions to the same effect. Only the main behavior starts from a game event, so if the second one needs its own event, use **Move to Its Own Effect**.

**Undo** and **Redo** in the perk's menu cover your edits this session, including removing and reordering effects.

Collapse cards you are done with. A collapsed card still shows its trigger and actions. When validation fails, **Show Problem** opens the card that needs fixing.

### Engine Catalog

**Tools > Engine Catalog…** is a technical browser for the behavior read out of the perks your game already has. Turn on **Enable Experimental Features** under **Preferences… > Editor & Library** if the entry is not there.

Choose a kind to see what it does and which installed perks use it. An entry shows its graph, how its components fit together, what links to it and what it links to, and the details of the scan that found it. Use **Authorable Only** to narrow the list to kinds you can put in a perk, or **All Kinds** to see everything the scan found. The **Markers** tab lets you search markers found in stock objects and see where they are used.

Use it to find behavior worth copying into a perk of your own, or to check what a stock perk really does before you borrow from it. The catalog reads your installed packages as you browse, so use **Retry Scan** if a scan reports an error.

### Unique Weapon Behavior

Some Exotics keep part of what makes them special in the weapon itself rather than in a perk. Hard Light's bouncing rounds, Malfeasance's embedded rounds, and Borealis's damage switching all work this way. **Unique Weapon Behavior** on **Weapon** copies that half onto the weapon you are building. It sits with the damage type and the other weapon-wide choices.

Pick a source the same way you pick an appearance donor. Most sources are Exotics, with a few Legendaries such as Drang and Warden's Law. Hover **Unique Weapon Behavior** for the full description of your current choice, and watch for a caution under the row when a combination has known limits. Choose **None** to go back to your weapon's own behavior.

**Include Its Perks** is on by default. It puts the source weapon's intrinsic in place of your weapon's own and adds its Exotic trait to a trait socket, because several Exotics keep the other half of the behavior there. The picker and its tooltip show which perks will land. These perks appear in **Perks & Sockets** while you edit. Check the choices and defaults after selecting a behavior, especially if you have already customized those sockets. Turn it off to borrow just the weapon's runtime behavior.

When the source's perks change how many rounds a burst fires, as Graviton Lance's and Bastion's do, your weapon takes the source's burst. Set **Firing Pattern** to **Base Weapon** to keep your weapon's own burst and rate of fire.

You can try a behavior on a different weapon type than the Exotic it came from, but some combinations only partly work or do nothing. The picker warns about known dependencies, such as a frame that expects its own ammo or a perk that reads a scope the host weapon does not have.

Damage switching is set by the damage type rather than here. Choose **Variable (Hold Reload)** and your weapon gets The Fundamentals and the behavior that drives it, while keeping its own appearance. Choosing Hard Light or Borealis as a Unique Weapon Behavior does the same and locks the damage type, since those two switch damage as well as fire differently.

If a borrowed behavior launches projectiles and your base weapon normally fires instantly, those projectiles may travel very slowly. **Projectile Speed Multiplier** raises their launch speed, with the increase capped at the value you choose. Sources already above that value keep their own speed. The default is a starting point, so adjust it a little at a time and test in-game.

For example, to put Malfeasance's rounds on an ordinary Hand Cannon:

1. Start a recipe with the Hand Cannon you want as the base weapon.
2. On **Weapon**, open **Unique Weapon Behavior** and choose **Malfeasance**.
3. Leave **Include Its Perks** on. Malfeasance keeps the detonation in its perk, so the weapon half alone will not finish the job.
4. Save, build, install, and get a copy from Collections.

### Ornaments, Icons and Shaders

**Appearance** and the ornament, shader, and appearance donor pickers show a preview of the weapon model, with its textures, animations, and shader colors, so you can see a choice before you commit to it. Drag to turn the model and Shift-drag or right-drag to pan. **View** changes the background and lighting, and **Clip** and **Speed** play one of its animations. Previews are not exact yet, so some models light or shade differently from how they look in game.

**Use Ornament** lists compatible stock ornaments, including ones from other weapons. Picking one takes that ornament's model, its own colors when available, and its inventory icon. An ornament has no rig of its own, so choosing one from another weapon also makes that weapon the appearance donor. Everything the ornament sets stays editable afterwards, and **Default Appearance** puts it back. The button sits beside the appearance picker on **Weapon** as well as on **Appearance**.

On **Appearance**, use **Change Icon** to choose another weapon's icon. Use **Edit Icon…** to import an image, adjust or replace colors, rotate, or flip it. Use PNG for transparent artwork. Imported images are saved in the recipe, so you don't need to share them separately.

The background follows your weapon's rarity. Icon edits don't recolor the 3D weapon model, but you can use shaders and appearance choices for that.

Custom perks, badges, and watermarks share an icon browser. Choose from game assets, use **Add Icon…** to load your own, or choose **Download destiny-icons** for the optional [destiny-icons](https://github.com/justrealmilk/destiny-icons) collection. Imported artwork is stored with the recipe or perk document.

Use **Edit Artwork…** under **Release Watermark** on **Appearance** to customize the small watermark on the weapon's inventory icon. Custom badges have the same artwork editor on **Collections**. Both support crop, placement, rotation, and flipping, and badges also offer background colors and gradients.

Adding a shader choice lets it recolor supported dye channels, including channels normally locked by an Exotic appearance. The original colors remain when no shader is selected. Explicit render-dye overrides in Advanced Gameplay take precedence.

To change the small weapon icon beside the ammo count, use **Ammo HUD Icon > Import HUD PNG…** and choose a transparent PNG. This doesn't change the inventory icon. **Use Appearance** switches back to the donor's HUD icon. Rebuild and install to see your changes in-game.

### Collections Placement

**Collections** chooses where your weapon appears in the game's Collections. By default it uses the node matching your weapon's ammo type and its base weapon's type, creating that node when the game has no stock one for the combination. Your weapon goes at the start of the node rather than beside its base weapon, so it is easier to find. Adding a page creates one shared page that uses the stock weapon-type name and icon. Exotic weapons always use the Exotics collection for their inventory slot.

Enable **Custom Badge** to group weapons under your own badge, with a name, description, and artwork. Use the same badge settings for each member, or select an existing badge with **Choose from Library**. You can also choose whether the weapon appears in the default Sunrise or Dawn badge.

Custom pages come out of a limited budget. The tab shows how many nodes you have used and how many remain, and it tells you when a build would go over. Remove a custom badge or an added page to get back under the limit.

Placement is presentation only. It does not change ammo, stats, or gameplay.

## Making Other Items

The **New Item** menu beside **New Weapon** starts an armor piece, Sparrow, Ship, Ghost Shell, Shader, or Subclass from a stock base. Save it as a recipe and select it in **Items in This Build** before building. This section covers gear. Shaders and Subclasses have their own sections below.

Edit the name, description, icon, rarity, supported stats, and socket choices. Gear keeps the base item's slot, class, model, and runtime behavior. Armor also has **Energy Type** and **Energy Capacity** controls. Exotic gear requires an Exotic base.

In Collections, armor appears beside its base. Sparrows, Ships, Ghost Shells, and Shaders appear on a **Project Sunrise** or **Dawn** page under their kind, and each of these pages uses one node from the [Collections budget](#collections-placement). Authored armor does not appear in the Project Sunrise or Dawn badge. Sparrow Speed, Boost, and Durability are tooltip stats. Changing the displayed Speed alone does not make a Sparrow travel faster, since its engine perk controls the speed tier.

Armor authoring will gain more options in future releases.

## Making Shaders

Choose **Shader** from the **New Item** menu and pick a stock Shader as the base. Each **Armor**, **Cloth**, and **Suit** dye channel has surfaces you can change one at a time:

- Pick any color with the color picker, and set its iridescence, metalness, smoothness, glow, detail strength, and worn finish.
- Give a dye another Shader's detail textures and change how often they repeat.
- Use **Copy from Shader…** to copy a surface, a dye's textures, or a whole dye channel from a stock Shader.

Edit every gear type at once, or one at a time to give weapons different colors from armor. The Shader's icon is built automatically from your color and material choices, in the style of the stock Shader icons, and updates as you edit. Turn off **Icon From Dyes** to pick an icon instead.

Authored Shaders appear on a **Project Sunrise** or **Dawn** page in Collections, and installing adds a stack of 777 of each to your inventory when there is room.

Shader authoring will gain more options in future releases as more of how Shaders work is understood.

## Making Subclasses

Choose **Subclass** from the **New Item** menu and pick a stock Subclass as the base. You can change its name, description, and icon.

For each ability, keep the base's or choose one from any stock Subclass, including another class's, such as Nova Bomb on a Hunter. The list shows the base's class first and notes when a choice comes from another class. Attunements work the same way. Top and bottom attunements can swap places, but a middle attunement can only go in the middle.

You can also build attunement paths node by node. Each node can come from any Subclass and take its own name, description, and perks, and each path takes its own name.

Installing adds each authored Subclass to every character of its base Subclass's class and equips the first one. Authored Subclasses have no Collections entry.

Subclass authoring will gain more options in future releases as more of how Subclasses work is understood.

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
- **Unsent**: An Arc Special-ammo Bow with Hush gameplay and Le Monarque's appearance. It demonstrates changing ammo and damage type while keeping bow-specific behavior and perk choices.
- **Every End**: A Solar Auto Rifle that combines Arc Logic's gameplay with Foregone Conclusion's appearance and heavily tuned stats. It is a broad reference for changing presentation, damage type, stats, and perk choices together.
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
- **Everything at Once**: A perk that does it all, the opposite of what a balanced sandbox would allow, and a good example of the different effects you can use in your own perks. Final blows and finishers refill every ability, drop Orbs of Light, reload, set off Firefly, Arc, and Void blasts, and grant invisibility, Truesight, an Arc Soul, and Devour. Adds an extra grenade, melee, and class ability charge.
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

Use **Duplicate** to make a separate item. Edit the existing recipe to update an item you've already made.

To remove a recipe from the library, open its menu and choose **Delete…**. Parhelion backs it up and removes it from the build selection. If the item was installed, review any account removals before installing the next build.

Renaming an item also changes its in-game identifiers, so the game treats it as a different item. If the next build replaces the old item, installation asks you to confirm removal of any saved copies.

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

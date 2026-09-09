# Parhelion

Parhelion is an **experimental** custom investment global package builder bundled with [Sundial](../../README.md) (basically, a weapons workbench). Build your own Destiny weapons by mixing weapon types, perks, stats, and appearance, including Exotics and combinations that wouldn't normally exist in the sandbox.

Weapons are saved as **recipes** that you can edit and share. Some combinations may not work or may crash the game, so test your weapons in-game after installing them.

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

Your weapon starts with the **base weapon**'s firing behavior, stats, and perks. The **appearance donor** determines the weapon's in-game model and iconography. Some models and animations won't work with a different weapon type.

Most editing happens on **Weapon** and **Appearance**. **Advanced Gameplay** contains experimental controls that are more prone to bugs and crashes and aren't yet recommended for normal use. **Identity** shows the weapon's hex hash identifiers for reference.

You can choose the weapon slot, damage type, and ammo type separately. The editor shows any restrictions. Changing ammo type doesn't rebalance the weapon's stats. Adjust those separately as needed.

Changing an Exotic base weapon to a non-exotic rarity removes its exotic equip restriction. Choosing Exotic rarity applies the normal exotic weapon restriction. Other unique-equip restrictions are preserved.

Installing unlocks your weapons in Collections and adds them to the **Project Sunrise badge**. Get a copy from Collections to add it to your inventory.

### Perks and Custom Perks

A **socket** is a slot for a perk or mod. The game calls these perks and mods **plugs**. Click a perk to replace it, or use **+ Add Choice** to add another option to that socket. Put your default first. Only one choice per socket is active at a time.

Use a socket's **… > Remove Socket** command to remove its choices and custom perk assignments. Other sockets keep their positions. For a removed base socket, click **Restore Socket** to bring back the base weapon's choices and role. Added sockets must be removed from last to first.

Custom perk authoring is in early development and unavailable in this release. The only prebuilt custom perk in this release is **Micro-Missile Frame**, an intrinsic version of Micro-Missile for experimenting with other weapon types, including Sidearms and Shotguns. Build and install a weapon that uses it, then test the combination in-game. Once packages are refreshed, it will appear like any other plug in Sundial's plug picker. As usual, which plugs appear depends on the socket filters you've selected.

Existing custom perks still load, save, and build with their recipes.

To add a saved custom perk to another weapon:

1. Open the weapon you want to edit, then choose **Custom Perks… > Use Existing Custom Perk…**.
2. Select the socket, then choose a perk. It replaces that socket's first choice.
3. Save the recipe.

The perk's settings are copied into this recipe. The original recipe isn't changed and doesn't need to be included in the build.

### Icons and Shaders

On **Appearance**, use **Change Icon** to choose another weapon's icon. Use **Edit Icon…** to import an image, adjust or replace colors, rotate, or flip it. Use PNG for transparent artwork. Imported images are saved in the recipe, so you don't need to share them separately.

The background follows your weapon's rarity. Icon edits don't recolor the 3D weapon model, but you can use shaders and appearance choices for that.

Adding a shader choice lets it recolor supported dye channels, including channels normally locked by an Exotic appearance. The original colors remain when no shader is selected. Explicit render-dye overrides in Advanced Gameplay take precedence.

To change the small weapon icon beside the ammo count, use **Ammo HUD Icon > Import HUD PNG…** and choose a transparent PNG. This doesn't change the inventory icon. **Use Appearance** switches back to the donor's HUD icon. Rebuild and install to see your changes in-game.

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
| `w64_parhelion_assets_0aa0_0.pkg` | Holds new artwork and other assets used by your weapons, including Sunrise badge and watermark assets. |
| `w64_investment_0361_7.pkg` | Connects weapons and perks to their in-game behavior. |
| `w64_investment_globals_client_058c_4.pkg` | Adds Collections entries, unlocks, and Sunrise badge progression. |
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

Perks can depend on a specific reload action, magazine size, or weapon behavior. Some perks won't work if your weapon doesn't have those mechanics.

### Why Aren't My Changes Showing?

Make sure you saved the recipe, selected it for the build, and installed the new packages with Destiny closed. Fully relaunch the game afterward.

Existing weapons may keep their selected perks. Get a fresh copy from Collections to check new defaults. If a weapon is missing from Collections, check the install report for an unlock error even if the packages installed successfully.

### Where Are the Advanced Controls?

Enable them under **Preferences… > Editor & Library**, then open **Advanced Gameplay**. Advanced controls are not yet recommended for normal use, as some combinations may freeze or crash the game. Please use with caution.

Turning them off hides the controls without removing your saved changes.

### What Should I Include in a Bug Report?

Include the recipe, what you did, what happened, and your Sundial and Sunrise versions. Add any error messages or screenshots. Use **Preferences… > Activity Log… > Copy Log** for recent events, or **Open Log Folder** to find `parhelion.log`. For install or recovery issues, keep the matching backup and build manifest too.

See the [Sundial README](../../README.md) for bug reports, credits, and licensing.

Sundial v0.5.1 expands perk editing and inspection, fixes borrowed Exotic weapon behavior, and adds model previews across both apps.

Read the [Parhelion README](crates/parhelion/README.md) for details about the Custom Perk Workbench, new weapon crafting features, bundled examples, and how to build and install your creations.

## Parhelion

- Added **model previews** with textures, normal maps, native animations, and animated shader effects.
  - Previews appear in the Engine Catalog, the Appearance tab, and the shader, ornament, and appearance donor pickers.
  - Not every model previews yet. Some render without their animation, and a few are not supported at all.
  - Previews are not as accurate as intended yet. Some models will light or shade differently from how they look in game.
- Builds with several weapons are faster. The build now reads Collections and the package files once instead of once per weapon.
- Added undo and redo to the **Custom Perk Workbench**, along with effect duplication, reordering, and navigation to validation failures.
- Expanded native behavior editing with editable action and condition lists, behavior groups, and alternative ending conditions.
  - Mapped more perk behavior into named controls, including health, shield, invisibility, and component properties.
- Counter triggers are now named controls. Set **Count Needed** and what happens **After It Fires**, and each contributing condition shows what it adds. A counter with nothing feeding it is flagged in the status bar.
- **Copy Existing…** now filters by type, origin, and effects, sorts, and opens on perks that have effects.
- Attached entities are named after the perks that carry them, and attachment and spawn roles are searchable in the asset picker.
- Added **Unique Weapon Behavior** entries for Drang, Warden's Law, Vigilance Wing, and Traveler's Chosen.
- Rebuilt the **Unique Weapon Behavior** picker with weapon icons, filters, the perks each choice brings, and Destiny symbols in its tooltips.
- Added a **Technical Build** window listing every value a build writes into the weapon, before a build to preview what it will assign and after one to check what it produced. Enable **Show Technical Build** in Preferences.
- Bundled recipes and example perks now update themselves after a release. An edited copy is kept as a duplicate and the old file is backed up.
- Added a **No Lore Tab** option for authored weapons that should not carry a lore entry at all.
- Expanded the **Engine Catalog** with resource graphs, component structure, scan details, and linked source and target information.
- Appearance donors are no longer limited to your weapon's own type. The picker opens on the matching type, and you can clear that filter to use any weapon's appearance.
  - A different weapon type may not render correctly, may fail the build, or may crash the game, so Parhelion warns when the two differ. A different inventory slot or an incompatible animation set is still refused.
  - The appearance's model is fitted to the gameplay donor's skeleton, so animations stay the gameplay donor's. Only a few combinations have been tested, such as a Hand Cannon appearance on Auto Rifle gameplay. This will improve in future releases.
- Authored weapons are now added to the Collections node matching their ammo and weapon type, at the start of that node rather than beside their donor.

### Fixes

- Fixed a borrowed firing graph arriving without its weapon's behavior record, so Graviton Lance, Cerberus+1, Izanagi's Burden, Lord of Wolves, and Symmetry now transfer both halves from one choice.
- Fixed borrowed behavior that only adds to its weapon never reaching the build, such as Tarrabah's.
- Fixed a borrowed behavior from another weapon family replacing your weapon's frame. The weapon keeps its own frame, and the picker says which frame stays behind.
- Fixed Parhelion clearing Sunrise's cache instead of Dawn's after installing.
- Fixed some bundled example perks shipping an older version than the one intended for release.
- Fixed a borrowed behavior leaving its source weapon's own labels behind, so a perk that keys on them never fired: Cosmology detonates only on a kill carrying Graviton Lance's label, which every graft now carries onto the host alongside the graph, record, and trait.
- A borrowed intrinsic frame now replaces your weapon's own frame instead of sitting beside it, and the donor's frame returns when the behavior is removed.
- Fixed a borrowed behavior pinning its source weapon's frame into a weapon of another type, which left an auto rifle taking Graviton Lance unable to fire. The frame now travels only between weapons of the same type.
- Fixed searching **Objects and Effects** in the Engine Catalog stalling on every keystroke.
- Fixed the **Text Presentation** section expanding on its own when opening a recipe.
- Fixed a finished build being reported as blocked when its temporary package view could not be removed yet. The build is kept, the message is shown, and the view is cleaned up by a later build; a stale view that still cannot be removed no longer stops the next build either.
- A recipe whose base weapon is another weapon from your library now builds. The build starts from that weapon's recipe, its stock donor and its changes, with your recipe's settings on top, and the Base Weapon control says so.

## Sundial

- Added model previews to the **Definition Inspector** for items that carry a model.
- Expanded the **Definition Inspector** with Triumphs, seasonal rewards, missions, stat groups, and Power caps.
- Reduced repeated catalog and account work while drawing.

### Fixes

- Fixed package reads exhausting the available file handles, which could fail a scan with "Too many open files".
- Fixed release notes in the updater showing broken links.

Sundial v0.4.1 and newer can update to v0.5.1 in the app. Older releases should install v0.5.1 manually.

Parhelion remains experimental, and not every weapon or perk combination has been tested. Report issues on [GitHub](https://github.com/KyleThmpsn/sundial/issues), on Discord, or on Twitter/X [@KyleThmpsn](https://x.com/KyleThmpsn). For Parhelion issues, include the recipe and describe what happens in game.

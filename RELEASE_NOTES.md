Sundial v0.5.2 adds Subclass and Shader authoring to Parhelion, along with armor and cosmetics, expands the Custom Perk Workbench and shared asset previews, and makes more installed game data searchable.

Read the [Parhelion README](crates/parhelion/README.md) for the workbench, bundled examples, and build and install instructions. Find previous release notes on the [releases page](https://github.com/KyleThmpsn/sundial/releases).

## Parhelion

- Parhelion can now author Subclasses that mix and match abilities from every class.
  - Take any ability or attunement from any stock Subclass, such as Nova Bomb on a Hunter or a Titan's grenade on a Warlock.
  - Build attunement paths node by node. Each node can come from any Subclass and take its own name, description, and perks, and each path takes its own name.
  - Installing adds each authored Subclass to every character of its base Subclass's class and equips the first one.
- Parhelion can now author Shaders in your own colors and materials.
  - Pick any color for each **Armor**, **Cloth**, and **Suit** surface with a color picker, and choose its iridescence, metalness, smoothness, glow, detail strengths, and worn finish.
  - Give any dye another Shader's detail textures and change how often they repeat.
  - Edit every gear type at once or one at a time, such as weapons apart from armor.
  - Or copy a surface, a dye's textures, or a whole dye channel from any stock Shader.
  - The Shader's icon is built automatically from your color and material choices, in the style of the stock Shader icons, and updates as you edit. Turn off **Icon From Dyes** to pick an icon instead.
  - Authored Shaders appear on a **Project Sunrise** or **Dawn** page in Collections, and installing adds a stack of 777 of each to your inventory when there is room.
- Parhelion can now author armor, Sparrows, Ships, and Ghost Shells. Customize their text, icons, supported stats, sockets, and perks.
  - Armor and cosmetic authoring may not have much use today, since new items are mostly copies of what is already in the game. It is groundwork for importing modern Destiny 2 items, which will be built on it once the importer is ready (hint, hint).
  - Subclass, Shader, armor, and cosmetic authoring will gain more options as I figure out more of how they work.
- Appearances from another weapon type can now bring their own rig and animations. The picker shows when the model must use the base weapon's rig instead.
- **Unique Weapon Behavior** combinations across weapon types now work much better.
  - **Include Its Perks** now brings the source weapon's intrinsic frame to any weapon type, fitted to that type.
  - A source whose perks change how many rounds a burst fires now gives any weapon type the source's burst. Set **Firing Pattern** to **Base Weapon** to keep the weapon's own burst and rate of fire.
  - For example, Graviton Lance on an Auto Rifle brings Black Hole and fires (very rapid) two-round bursts, and Bastion fires its three-round bursts without its slower fire rate and smaller magazine.
- Ornaments from other compatible weapons can now be used as appearances.
- The **Custom Perk Workbench** now edits everything in each effect card, with **Or…**, **And…**, and **Not** in place and a **Timing** row for duration and cooldown. Linked objects and visual effects open from the card for editing.
- Every stock effect can now be added and edited in place without changing the original perk, even ones the workbench could not open before, such as Sparrow traits.
- **Add Action…** now has one-step presets for common stock perks: make a weapon full auto, charge its shots like Charge Shot, track targets like Tracking Module, or add Rampage's stacking damage or Outlaw's faster reload.
- The workbench's pickers are easier to use.
  - **Suggested** is the default order and puts everyday choices first, such as **On Weapon Kill**, **Change a Weapon or Ability Stat**, **Nova Bomb**, and **Rampage**.
  - Search also finds behaviors by what they do, such as "headshot", "reload", or "orb", by the stock perks that use them, and objects and effects by their in-game names, such as "hammer of sol".
  - Double-click a choice to use it. **Add from Perk…** adds all of a perk's effects at once.
- More perk behavior, values, and keys have plain names, including **Improve Radar Detail**, **Change How the Trigger Fires**, **Change Incoming Damage**, and **Change Outgoing Damage**. Damage cards show base damage, precision, and overall bonuses, and ammo drop cards show Primary, Special, and Heavy values together.
- Kill effects whose actions happen once now end at once, as Firefly does, so each kill starts them again. Kill triggers such as **On Melee Kill** offer **In Hand**, which counts the kill only while this weapon is in hand, as Grave Robber does.
- Builds after the first are much faster, because Parhelion now keeps what it learns about stock game data between builds.
- **Tools** adds **Resync Account**, which sets the installed weapons' Collections unlocks and adds the installed Subclasses and Shaders to the current account again, for example after switching between Sunrise and Dawn, without a rebuild or a reinstall.
- **Everything at Once** joins the bundled perks. It does it all, the opposite of what a balanced sandbox would allow, and is a good example of the different effects you can use in your own custom perks.
  - Final blows and finishers refill every ability, drop Orbs of Light, reload, set off Firefly, Arc, and Void blasts, and grant invisibility, Truesight, an Arc Soul, and Devour. It also adds an extra grenade, melee, and class ability charge.
- **On Releasing the Trigger**, **On Weapon Swap**, and **Ends on a Specific Ability** now appear in the condition picker without turning on **Show All**.
- **On a Specific Ability** and **Ends on a Specific Ability** now let you pick a supported grenade or Super by name.
- Recipes can now be deleted from the library.
- When one recipe stops a build, **Build Blocked** offers **Remove from Build** and **Open Recipe** for it.
- Restoring base sockets now asks before removing custom perks.
- The **Engine Catalog** adds searchable **Markers** and **Native Resources** tabs, and links behavior kinds to their stock uses.

### Fixes

- Fixed requirements added in the **Custom Perk Workbench** never passing, so an effect that needed all of them never ran. This includes kill effects combined with another requirement using **And…**.
- Fixed counters created in the **Custom Perk Workbench** never counting. Only counters copied from a stock perk worked before.
- Fixed a crash when an ornament replaces only some model parts, such as Third Rail on Riskrunner.
- Fixed an appearance donor changing how the weapon fires, which cost the base weapon its own Exotic behavior, such as Trinity Ghoul's.
- Fixed a borrowed intrinsic frame removing a custom perk placed in the same socket.
- Fixed new **On Picking Up Ammo** and **On Sliding** conditions never passing, and a new **Hold a Weapon Count** removing a count instead of adding one. Some less common conditions and actions also start with the settings stock perks use.
- Fixed effects that could never work building without a warning, such as ones missing a key, target, or object, or a counter with nothing to count. The **Custom Perk Workbench** now points them out.
- Fixed **Unique Weapon Behavior** refusing to build with some sources, including Arbalest, Traveler's Chosen, and Warden's Law.
- Fixed builds failing for weapons the game lists in one slot but equips in another, such as "dummy" copies of Trust and Polaris Lance.
- Fixed appearances with moving parts or an empty model part, such as The Spiteful Fang and Whispering Slab, failing to build on another weapon type.
- Fixed an installed stock weapon being mistaken for an authored weapon with the same identity.
- Fixed unclear build errors for a socket with no default perk, such as Bad Reputation's. They now name the recipe and socket and say what to choose.
- Fixed base weapons with no ammo type of their own, such as a "dummy" copy of Rose, failing to build. They now join the Primary Collections node unless the recipe chooses an **Ammo Type**.
- Fixed some objects and effects showing the name of something unrelated, such as Outlaw's buff appearing as a Leviathan raid decoy. It now reads **Outlaw Attachment**.
- Fixed names in the **Custom Perks** list appearing in bold, which made them harder to read ([#11](https://github.com/KyleThmpsn/sundial/issues/11)).

## Sundial

- The preview window adds camera and lighting controls, a choice of animation and playback speed, and image and 3D model export in the current pose.
  - It also previews static maps and props, particle systems with their sequence timing, and light volumes, with approximate rendering for some particles.
  - It can now play supported packaged audio on Windows, save clips as WEM, and decode supported clips to WAV.
  - Metal now reflects a studio, so polished gold, chrome, and iridescent finishes read as metal and show their patterns.
  - Previews are not perfect yet, and some models may look inaccurate. They will keep improving in future releases.
- Reworked **Definition Inspector** search and navigation, with more links and detail across items, Collections, progression, and related definitions. Open it from the sidebar or with Ctrl+I.
- Reduced memory retained after large catalog scans on Linux. **Copy Report** now includes resident memory when available.
- Sundial now remembers the size and position of its windows, and the zoom level, between sessions.

### Fixes

- Fixed Linux scans failing with "Too many open files" ([#11](https://github.com/KyleThmpsn/sundial/issues/11)). Sundial now raises its open-file limit when it can.
- Fixed each update leaving its workspace folder beside Sundial. Startup now removes the finished ones.
- Fixed items added to a Dawn character not advancing that character's inventory counter, which could stop Dawn from loading the character.
- Fixed game symbols in text, such as the Solar glyph in an objective's description, drawing as unrelated icons. The game's symbol fonts now lead the text fonts everywhere.
- Fixed the Definition Inspector, JSON Editor and model preview windows showing a scaled-down copy of the large Sundial icon. They carry the window icon at title-bar size, and their titles leave out symbols the desktop cannot draw.

Sundial v0.4.1 and newer can update to v0.5.2 in the app. Older releases should install v0.5.2 manually.

Parhelion remains experimental. New gear, Subclasses, appearances from other weapon types, and borrowed perk behavior still need in-game testing, and some combinations may crash. Report issues on [GitHub](https://github.com/KyleThmpsn/sundial/issues), on Discord, or on Twitter/X [@KyleThmpsn](https://x.com/KyleThmpsn). For Parhelion issues, include the recipe and describe what happens in game.

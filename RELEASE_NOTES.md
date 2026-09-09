# Sundial v0.4.1

This release adds support for Sunrise settings v18 and the official SQLite account database, while preserving support for older JSON accounts.

- Edit characters, all 17 equipment slots, inventory, preferences, numeric key bindings, ownership, Collections and progression in `data/investment.sqlite3` schema v2.
- Keep runtime configuration in `settings.json`. Legacy account fields in that file remain preserved when SQLite is active.
- Use Parhelion's collection unlock synchronization, package replacement and uninstall cleanup with SQLite accounts, including socket resizing and journaled recovery.
- Match Sunrise's acquired-flag encoding and validate native binding codes, reserved settings, character stacks and entitlement rows.
- Browse every decoded unlock definition with saved state, evaluated state, objectives and references. Inspect all stored SQLite unlock lanes and override rows, including preserved raw values.
- Count preserved native overrides toward Sunrise's limits and accept native objective values independently of legacy JSON sentinel rules.
- Correct Reclamation Order to special ammo in the bundled recipe.
- Save with verified native backups and checks for outside changes, including changes in the SQLite write-ahead log. Undo preserves unknown native fields.
- Block account editing for missing, uninitialized or unsupported databases instead of falling back to inactive JSON data. Start Sunrise to initialize or migrate its database, then reload Sundial.

SQLite support is included in standard builds. JSON-only builds remain available. Package and persistence checks do not replace in-game testing of authored weapons.

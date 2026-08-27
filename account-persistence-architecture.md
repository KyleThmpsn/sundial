# Account persistence architecture

Status: implemented for the pinned PR 88 contract; pending upstream contract confirmation and
release review.

## Current implementation

- `sundial-account` is a compiler-enforced, storage-neutral crate with stable entity IDs,
  capabilities, profile-item commands, dismantle-reward commands, and atomic validation.
- The production JSON path for profile items and dismantle rewards now executes those commands and
  losslessly projects the result back into the document.
- Frozen legacy JSON mutations remain test-only differential oracles. Schemas 2 through 8, future
  profile behavior, failures, ordering, duplicate policies, and opaque-field preservation are
  covered.
- Characters, equipment, inventory, plugs, and SOID allocation now have one atomic neutral-domain
  aggregate. The production JSON path uses operation-scoped loads for inventory add/edit/remove,
  cross-character moves, inventory/equipment swaps, and unequip operations. Lossless projection,
  malformed unrelated data, future-schema opacity, failures, atomicity, ordering, and complete-row
  preservation are covered against a frozen test-only legacy oracle.
- Equipment-only definition, level, flag, single-plug, and empty-slot mutations now use the same
  neutral aggregate. Patch-scoped loads preserve the legacy helpers' minimal-row behavior without
  weakening full-account validation, and single-plug commands preserve every untouched JSON plug
  representation. The pre-domain equipment mutations remain test-only differential oracles.
- Guided account preferences and named key bindings now issue storage-neutral, atomically validated
  command batches. Their operation-scoped JSON projection changes only requested scalar fields,
  accepts known fields in future JSON schemas, and leaves malformed unrelated groups opaque. JSON
  schema-8 preference materialization is isolated with the JSON adapter rather than UI policy.
- Character race, gender, class, and coordinated ability fields now live in the neutral character
  aggregate. Class changes, armor restoration, subclass changes, inventory swaps, and save-time
  ability repair use atomic character commands. Armor copies retain destination instance identity
  and losslessly merge adapter-owned opaque fields.
- `AccountWorkspace` selects the authoritative account source once per loaded workspace. Guided
  profile, dismantle, inventory, equipment, character-metadata, and account-setting mutations route
  through it, as do the neutral profile/inventory/equipment/metadata snapshots consumed by those
  views. JSON and SQLite have separate adapters; neither adapter calls or probes the other.
- Bulk/randomized inventory removal also emits one neutral atomic command batch rather than
  retaining JSON rows directly.
- The SQLite adapter is pinned to PR-88 commit
  `5a5583ab0cc4244bca11974a928bdc1a0b49f4b7`. It validates all six schema-v1 table layouts and the
  relational, account-row, and settings-payload versions independently before reconstructing the
  neutral account state. It never creates or migrates a database.
- SQLite saves use a complete validated candidate, an `IMMEDIATE` transaction, an in-transaction
  source-revision check, a verified SQLite-native backup, and a post-commit revision rebase. A failed
  coordinated JSON save restores and verifies the SQLite backup.
- A JSON-selected workspace re-probes before saving and requires reload if a compatible or blocked
  SQLite source appears. SQLite writers use read/write-without-create flags, so a deleted source is
  never silently replaced by a blank database.
- Guided recovery validates the selected exact-contract backup, preserves the current database with
  SQLite's native backup API and `quick_check`, restores the selected snapshot, and reloads source
  selection. This also permits recovery from a healthy but contract-incompatible database without
  discarding that incompatible source.
- PR-88 fixtures cover JSON/SQLite semantic parity, high-bit SOIDs, full-width item flags,
  nullable plugs, combined gear-class masks, malformed row prefixes, count mismatches, newer
  nested formats, exact settings-payload round trips, transactional save/reload, source conflicts,
  backup verification, and restore verification.
- The default development build includes `sqlite-account`. Building with `--no-default-features`
  preserves the JSON-only code path and is tested independently.
- JSON schema-8 preference materialization runs only after a workspace selects JSON as its account
  source. SQLite and blocked workspaces keep stale JSON account fields untouched.

## Dependency rule

Sundial has one storage-neutral account domain. UI code issues account commands against an
in-memory account state. Persistence adapters load and save that state; they do not implement
inventory, equipment, character, or settings actions.

The `sundial-account` crate must not depend on JSON, SQLite, egui, filesystem APIs, Sunrise file
paths, or catalog types. Keeping it as a separate crate makes this boundary compiler-enforced.

```text
UI -> account commands -> account state
                            |
                       load/save port
                         /       \
                 JSON adapter  SQLite adapter
```

JSON and SQLite adapters must not call one another. Source selection and coordinated saves belong
to the workspace layer above both adapters.

## Source ownership

JSON continues to own player identity, language, progression, client/server configuration, and
generated-file inputs. The account source owns account settings, profile items, dismantle rewards,
characters, equipment, character inventory, and item plugs.

Sundial never mirrors account data between sources. A workspace writes account data only to the
source selected when it loaded.

## Source selection

| `state.sqlite3` state | Account behavior |
| --- | --- |
| Missing, zero-byte, or uninitialized | Preserve the existing `settings.json` behavior |
| Exact pinned PR-88 contract | Use SQLite for account reads and writes |
| Corrupt, structurally different, or newer contract | Block account editing; never fall back to stale JSON account data |

Selection happens once at workspace load. Reloading is required to select a newly created or
changed source.

PR 88 uses SQLite WAL mode. A read-only SQLite connection may create or retain the standard
`state.sqlite3-wal` and `state.sqlite3-shm` coordination files even though the main database is
unchanged. Sundial uses normal SQLite locking and change detection; it does not use the unsafe
`immutable` shortcut or manually delete WAL files.

## Adapter responsibilities

An adapter may:

- Detect and validate its own format versions.
- Translate persisted values to and from domain values.
- Preserve opaque fields or rows it does not understand.
- Produce storage-neutral capabilities for the loaded account.
- Verify source revisions, create backups, and commit atomically within its own storage format.

An adapter must not:

- Contain account mutation or validation policy.
- Expose JSON values, SQL rows, paths, or table names to the account domain.
- Silently fall back to a different writable source after finding an unsupported authoritative
  source.
- Write an unknown or structurally modified format.

## Incremental cutover

Each operation family is first implemented in the account domain and exercised through a JSON
adapter in differential tests. The existing JSON mutation is retained temporarily as the oracle.
Production switches only after success, failure, atomicity, ordering, and unknown-field
preservation match. The legacy mutation is then removed before beginning the next family.

Profile items, dismantle rewards, character inventory, equipment mutations, character metadata,
guided account preferences, and named key bindings now run through the domain. Character inventory,
equipment, metadata, and abilities move together because swaps, class armor restoration, subclass
selection, and SOID allocation cross that boundary. JSON-owned player identity, language,
progression, and client settings remain separate.

## Remaining sequence

1. Track PR 88 and compare every reviewed upstream change with the pinned schema and payload bytes.
   Do not silently reinterpret a changed contract under version 1.
2. Exercise the development build against real PR-88 sessions, including save, game restart,
   inventory moves, equipment changes, account settings, and backup recovery.
3. After Sunrise merges or explicitly endorses the contract, update the pin if necessary, rerun the
   complete parity and recovery suites, and make the release decision.

## SQLite release gates

SQLite release requires all of the following:

- An officially accepted Sunrise schema and payload format. **Pending upstream.**
- Exact schema and column-layout validation. **Implemented.**
- JSON/SQLite semantic parity fixtures. **Implemented.**
- SQLite-native backup, guided recovery, and verified restoration. **Implemented.**
- Revision checks inside the write transaction. **Implemented.**
- Protection against a running game overwriting the database at shutdown. **Implemented.**
- Tests for high-bit SOIDs, item and plug ordering, null plugs, unknown versions, corrupt data,
  reload, reset, raw JSON editing, undo/redo, and coordinated JSON/SQLite saves. **Implemented; live
  session coverage remains part of release review.**

Unknown, corrupt, or newer database and payload formats block account editing.

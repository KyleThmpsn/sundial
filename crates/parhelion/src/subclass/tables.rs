//! The stock socket-entry-list and display tables, which grow by a row for each subclass with a
//! list of its own, and name each row by the same hash. Beside them, the ability tables, which
//! grow by a row for each ability with an entity of its own.
use sundial::package_authoring::{
    PackageManager,
    ability_definition::{self, DEFINITION_TABLE_CLASS, Definition, IDENTITY_TABLE_CLASS},
    investment_schema::ITEM_INDEX_ROW_SIZE,
};
use tiger_pkg::TagHash;

use super::native::{
    SOCKET_ENTRY_LIST_CLASS, SOCKET_ENTRY_LIST_ROW_CLASS, SOCKET_ENTRY_LIST_TABLE_CLASS,
    SUPER_ENTRY_KIND, TALENT_DISPLAY_ROW_CLASS, TALENT_DISPLAY_TABLE_CLASS, entries,
};
use crate::error::{invalid, validation};
use crate::tag_payload::{append_index_row, read_tag, read_u32, terminal_index_table_layout};
use crate::{AuthoringResult, ReplacementSpec};

/// The tag of the socket-entry list at `index` in the stock table.
pub(crate) fn list_tag(table: &[u8], index: u16) -> AuthoringResult<TagHash> {
    let (count, _, rows) = terminal_index_table_layout(
        table,
        SOCKET_ENTRY_LIST_ROW_CLASS,
        "socket-entry-list table",
    )?;
    let index = usize::from(index);
    if index >= count {
        return Err(invalid(format!(
            "Socket-entry list {index} is outside the table"
        )));
    }
    Ok(TagHash(read_u32(
        table,
        rows + index * ITEM_INDEX_ROW_SIZE + 16,
    )?))
}

#[derive(Clone)]
pub(crate) struct SubclassTables {
    pub(crate) list_table_tag: TagHash,
    pub(crate) display_table_tag: TagHash,
    pub(crate) lists: Vec<u8>,
    pub(crate) displays: Vec<u8>,
    /// Stock lists that carry a super lane, which Sunrise keeps selection state for.
    pub(crate) super_lane_lists: usize,
    /// The ability rows a pool record names its ability by (globals slot 69), and their
    /// identities (root slot 105).
    ability_table_tag: TagHash,
    ability_identity_table_tag: TagHash,
    abilities: Vec<u8>,
    ability_identities: Vec<u8>,
    stock_ability_rows: usize,
}

impl SubclassTables {
    pub(crate) fn load(
        manager: &PackageManager,
        (list_table_tag, display_table_tag): (TagHash, TagHash),
        (ability_table_tag, ability_identity_table_tag): (TagHash, TagHash),
    ) -> AuthoringResult<Self> {
        for (tag, class, label) in [
            (
                ability_table_tag,
                DEFINITION_TABLE_CLASS,
                "Ability definition table",
            ),
            (
                ability_identity_table_tag,
                IDENTITY_TABLE_CLASS,
                "Ability identity table",
            ),
            (
                list_table_tag,
                SOCKET_ENTRY_LIST_TABLE_CLASS,
                "Socket-entry-list table",
            ),
            (
                display_table_tag,
                TALENT_DISPLAY_TABLE_CLASS,
                "Subclass display table",
            ),
        ] {
            let entry = manager
                .get_entry(tag)
                .ok_or_else(|| invalid(format!("{label} {tag} is not live")))?;
            if entry.reference != class {
                return Err(invalid(format!(
                    "{label} {tag} has class 0x{:08X}, expected 0x{class:08X}",
                    entry.reference
                )));
            }
        }
        let lists = read_tag(manager, list_table_tag, "socket-entry-list table")?;
        let displays = read_tag(manager, display_table_tag, "subclass display table")?;
        let (list_count, _, _) = terminal_index_table_layout(
            &lists,
            SOCKET_ENTRY_LIST_ROW_CLASS,
            "socket-entry-list table",
        )?;
        let (display_count, _, _) = terminal_index_table_layout(
            &displays,
            TALENT_DISPLAY_ROW_CLASS,
            "subclass display table",
        )?;
        if list_count != display_count {
            return Err(invalid(format!(
                "The socket-entry-list table has {list_count} rows and its display table {display_count}"
            )));
        }
        let abilities = read_tag(manager, ability_table_tag, "ability definition table")?;
        let ability_identities = read_tag(
            manager,
            ability_identity_table_tag,
            "ability identity table",
        )?;
        let stock_ability_rows = ability_definition::definitions(&abilities, &ability_identities)
            .map_err(invalid)?
            .len();
        let mut tables = Self {
            list_table_tag,
            display_table_tag,
            lists,
            displays,
            super_lane_lists: 0,
            ability_table_tag,
            ability_identity_table_tag,
            abilities,
            ability_identities,
            stock_ability_rows,
        };
        for index in 0..list_count {
            let (tag, _) = tables.row_tags(u16::try_from(index).unwrap_or(u16::MAX))?;
            if manager
                .get_entry(tag)
                .is_none_or(|entry| entry.reference != SOCKET_ENTRY_LIST_CLASS)
            {
                continue;
            }
            // The empty and cut-down lists carry no super lane, and may carry no entry rows.
            let list = read_tag(manager, tag, "socket-entry list")?;
            if entries(&list)
                .is_ok_and(|entries| entries.iter().any(|entry| entry.kind == SUPER_ENTRY_KIND))
            {
                tables.super_lane_lists += 1;
            }
        }
        Ok(tables)
    }

    pub(crate) fn count(&self) -> AuthoringResult<usize> {
        Ok(terminal_index_table_layout(
            &self.lists,
            SOCKET_ENTRY_LIST_ROW_CLASS,
            "socket-entry-list table",
        )?
        .0)
    }

    /// A row's list tag and display tag.
    pub(super) fn row_tags(&self, index: u16) -> AuthoringResult<(TagHash, TagHash)> {
        let index = usize::from(index);
        let (count, _, rows) = terminal_index_table_layout(
            &self.lists,
            SOCKET_ENTRY_LIST_ROW_CLASS,
            "socket-entry-list table",
        )?;
        let (_, _, display_rows) = terminal_index_table_layout(
            &self.displays,
            TALENT_DISPLAY_ROW_CLASS,
            "subclass display table",
        )?;
        if index >= count {
            return Err(invalid(format!(
                "Socket-entry list {index} is outside the table"
            )));
        }
        let row = rows + index * ITEM_INDEX_ROW_SIZE;
        let display_row = display_rows + index * ITEM_INDEX_ROW_SIZE;
        if read_u32(&self.lists, row)? != read_u32(&self.displays, display_row)? {
            return Err(invalid(format!(
                "Socket-entry list {index} and its display row name different lists"
            )));
        }
        Ok((
            TagHash(read_u32(&self.lists, row + 16)?),
            TagHash(read_u32(&self.displays, display_row + 16)?),
        ))
    }

    /// The entity ability row `row` names, through `assignments`, the entity assignment table.
    pub(crate) fn ability_entity(
        &self,
        assignments: &[u8],
        row: u8,
    ) -> AuthoringResult<Option<TagHash>> {
        Ok(
            ability_definition::entity(&self.abilities, assignments, row)
                .map_err(invalid)?
                .map(TagHash),
        )
    }

    /// Every ability row's identity hash.
    pub(crate) fn ability_identities(&self) -> AuthoringResult<Vec<u32>> {
        Ok(
            ability_definition::definitions(&self.abilities, &self.ability_identities)
                .map_err(invalid)?
                .into_iter()
                .map(|row| row.identity)
                .collect(),
        )
    }

    /// Adds an ability row copied from `template` under `identity` and `pattern`, and returns
    /// the new row.
    pub(crate) fn append_ability(
        &mut self,
        template: u8,
        identity: u32,
        pattern: u32,
    ) -> AuthoringResult<u8> {
        let (abilities, identities, row) = ability_definition::append(
            &self.abilities,
            &self.ability_identities,
            template,
            Definition { identity, pattern },
        )
        .map_err(invalid)?;
        self.abilities = abilities;
        self.ability_identities = identities;
        Ok(row)
    }

    /// The tables a build replaces: the lists and displays, and the ability tables once a row was
    /// added to them.
    pub(crate) fn replacements(self) -> AuthoringResult<Vec<ReplacementSpec>> {
        let ability_rows =
            ability_definition::definitions(&self.abilities, &self.ability_identities)
                .map_err(invalid)?
                .len();
        let mut replacements = vec![
            ReplacementSpec {
                tag: self.list_table_tag,
                payload: self.lists,
            },
            ReplacementSpec {
                tag: self.display_table_tag,
                payload: self.displays,
            },
        ];
        if ability_rows != self.stock_ability_rows {
            replacements.extend([
                ReplacementSpec {
                    tag: self.ability_table_tag,
                    payload: self.abilities,
                },
                ReplacementSpec {
                    tag: self.ability_identity_table_tag,
                    payload: self.ability_identities,
                },
            ]);
        }
        Ok(replacements)
    }

    /// Adds rows for an authored list named `hash`, copied from the base list's rows at
    /// `base_index`, and returns the list's index.
    pub(crate) fn append(
        &mut self,
        (base_index, hash): (u16, u32),
        list_tag: TagHash,
        display_tag: TagHash,
    ) -> AuthoringResult<u16> {
        let index = u16::try_from(self.count()?)
            .map_err(|_| invalid("Socket-entry-list index does not fit 16 bits"))?;
        self.lists = append_index_row(
            std::mem::take(&mut self.lists),
            usize::from(base_index),
            hash,
            list_tag,
            SOCKET_ENTRY_LIST_ROW_CLASS,
            "socket-entry-list table",
        )?;
        self.displays = append_index_row(
            std::mem::take(&mut self.displays),
            usize::from(base_index),
            hash,
            display_tag,
            TALENT_DISPLAY_ROW_CLASS,
            "subclass display table",
        )?;
        if self.row_tags(index)? != (list_tag, display_tag) {
            return Err(validation("Authored subclass list rows are inconsistent"));
        }
        Ok(index)
    }
}

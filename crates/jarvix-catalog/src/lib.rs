//! System Catalog and Schema metadata manager for Jarvix.

use std::collections::HashMap;
use jarvix_common::constants::INVALID_PAGE_ID;
use jarvix_common::error::{JarvixError, Result};
use jarvix_common::schema::Schema;
use jarvix_common::types::{Oid, PageId};

#[derive(Debug, Clone)]
pub struct TableInfo {
    pub oid: Oid,
    pub name: String,
    pub schema: Schema,
    pub first_page_id: PageId,
}

#[derive(Debug, Clone)]
pub struct IndexInfo {
    pub oid: Oid,
    pub name: String,
    pub table_name: String,
    pub column_idx: usize,
    pub is_unique: bool,
    pub root_page_id: PageId,
}

pub struct Catalog {
    tables: HashMap<String, TableInfo>,
    tables_by_oid: HashMap<Oid, String>,
    indexes: HashMap<String, IndexInfo>,
    next_oid: Oid,
}

impl Catalog {
    pub fn new() -> Self {
        Self {
            tables: HashMap::new(),
            tables_by_oid: HashMap::new(),
            indexes: HashMap::new(),
            next_oid: 1000,
        }
    }

    /// Creates and registers a new table schema in the catalog.
    pub fn create_table(
        &mut self,
        name: impl Into<String>,
        schema: Schema,
        first_page_id: PageId,
    ) -> Result<&TableInfo> {
        let name_str = name.into();
        let key = name_str.to_lowercase();

        if self.tables.contains_key(&key) {
            return Err(JarvixError::Catalog(format!(
                "Table '{}' already exists in catalog",
                name_str
            )));
        }

        let oid = self.next_oid;
        self.next_oid += 1;

        let table_info = TableInfo {
            oid,
            name: name_str.clone(),
            schema,
            first_page_id,
        };

        self.tables.insert(key.clone(), table_info);
        self.tables_by_oid.insert(oid, key.clone());

        Ok(self.tables.get(&key).unwrap())
    }

    pub fn get_table(&self, name: &str) -> Option<&TableInfo> {
        self.tables.get(&name.to_lowercase())
    }

    pub fn get_table_by_oid(&self, oid: Oid) -> Option<&TableInfo> {
        let key = self.tables_by_oid.get(&oid)?;
        self.tables.get(key)
    }

    pub fn drop_table(&mut self, name: &str) -> Result<()> {
        let key = name.to_lowercase();
        let info = self.tables.remove(&key).ok_or_else(|| {
            JarvixError::Catalog(format!("Table '{}' does not exist in catalog", name))
        })?;

        self.tables_by_oid.remove(&info.oid);

        // Also drop associated indexes
        self.indexes.retain(|_, idx| !idx.table_name.eq_ignore_ascii_case(name));

        Ok(())
    }

    /// Registers a secondary or unique index in the catalog.
    pub fn create_index(
        &mut self,
        index_name: impl Into<String>,
        table_name: &str,
        column_name: &str,
        is_unique: bool,
    ) -> Result<&IndexInfo> {
        let idx_name_str = index_name.into();
        let idx_key = idx_name_str.to_lowercase();

        if self.indexes.contains_key(&idx_key) {
            return Err(JarvixError::Catalog(format!(
                "Index '{}' already exists",
                idx_name_str
            )));
        }

        let (tbl_name, col_idx) = {
            let table = self.get_table(table_name).ok_or_else(|| {
                JarvixError::Catalog(format!("Table '{}' does not exist", table_name))
            })?;

            let col_idx = table.schema.column_index(column_name).ok_or_else(|| {
                JarvixError::Catalog(format!(
                    "Column '{}' does not exist in table '{}'",
                    column_name, table_name
                ))
            })?;

            (table.name.clone(), col_idx)
        };

        let oid = self.next_oid;
        self.next_oid += 1;

        let index_info = IndexInfo {
            oid,
            name: idx_name_str.clone(),
            table_name: tbl_name,
            column_idx: col_idx,
            is_unique,
            root_page_id: INVALID_PAGE_ID,
        };

        self.indexes.insert(idx_key.clone(), index_info);
        Ok(self.indexes.get(&idx_key).unwrap())
    }

    pub fn set_index_root(&mut self, index_name: &str, root_page_id: PageId) -> Result<()> {
        let idx = self.indexes.get_mut(&index_name.to_lowercase()).ok_or_else(|| {
            JarvixError::Catalog(format!("Index '{}' does not exist", index_name))
        })?;
        idx.root_page_id = root_page_id;
        Ok(())
    }

    pub fn get_index(&self, name: &str) -> Option<&IndexInfo> {
        self.indexes.get(&name.to_lowercase())
    }

    pub fn get_table_indexes(&self, table_name: &str) -> Vec<&IndexInfo> {
        self.indexes
            .values()
            .filter(|idx| idx.table_name.eq_ignore_ascii_case(table_name))
            .collect()
    }
}

impl Default for Catalog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jarvix_common::schema::Column;
    use jarvix_common::types::DataType;

    #[test]
    fn test_catalog_table_and_index_lifecycle() {
        let mut catalog = Catalog::new();

        let schema = Schema::new(vec![
            Column::new("id", DataType::Integer, false),
            Column::new("username", DataType::Varchar(64), false),
            Column::new("email", DataType::Text, true),
        ]);

        let tbl = catalog.create_table("users", schema, 1).unwrap();
        assert_eq!(tbl.name, "users");
        assert_eq!(tbl.oid, 1000);

        // Retrieve table
        let retrieved = catalog.get_table("USERS").unwrap();
        assert_eq!(retrieved.schema.column_count(), 3);

        // Create index on username
        let idx = catalog.create_index("idx_username", "users", "username", true).unwrap();
        assert_eq!(idx.name, "idx_username");
        assert_eq!(idx.column_idx, 1);
        assert!(idx.is_unique);
        assert_eq!(idx.root_page_id, INVALID_PAGE_ID);

        let table_indexes = catalog.get_table_indexes("users");
        assert_eq!(table_indexes.len(), 1);

        catalog.set_index_root("idx_username", 5).unwrap();
        assert_eq!(catalog.get_index("idx_username").unwrap().root_page_id, 5);

        // Drop table
        assert!(catalog.drop_table("users").is_ok());
        assert!(catalog.get_table("users").is_none());
        assert!(catalog.get_table_indexes("users").is_empty());
    }
}

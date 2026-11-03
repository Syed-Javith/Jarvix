use jarvix_common::error::{JarvixError, Result};
use jarvix_common::types::{RecordId, Value};
use jarvix_common::tuple::Tuple;
use jarvix_index::BTreeIndex;
use jarvix_optimizer::LogicalPlan;
use jarvix_storage::PageType;

use crate::context::ExecutionContext;

pub struct DdlExecutor;

impl DdlExecutor {
    pub fn execute(plan: &LogicalPlan, context: &ExecutionContext) -> Result<String> {
        match plan {
            LogicalPlan::CreateTable {
                name,
                schema,
                if_not_exists,
            } => {
                let mut cat = context.catalog.write().unwrap();
                if cat.get_table(name).is_some() {
                    if *if_not_exists {
                        return Ok(format!("NOTICE: table \"{}\" already exists, skipping", name));
                    }
                    return Err(JarvixError::Catalog(format!(
                        "relation \"{}\" already exists",
                        name
                    )));
                }

                // Allocate initial physical heap page
                let (first_pid, page_guard) = context.bpm.new_page(PageType::Heap)?;
                drop(page_guard);

                cat.create_table(name.clone(), schema.clone(), first_pid)?;
                Ok("CREATE TABLE".to_string())
            }

            LogicalPlan::DropTable { name, if_exists } => {
                let mut cat = context.catalog.write().unwrap();
                if cat.get_table(name).is_none() {
                    if *if_exists {
                        return Ok(format!("NOTICE: table \"{}\" does not exist, skipping", name));
                    }
                    return Err(JarvixError::Catalog(format!(
                        "table \"{}\" does not exist",
                        name
                    )));
                }

                cat.drop_table(name)?;
                Ok("DROP TABLE".to_string())
            }

            LogicalPlan::CreateIndex {
                name,
                table_name,
                column_idx,
                unique,
            } => {
                let (first_pid, col_name) = {
                    let cat = context.catalog.read().unwrap();
                    let tbl = cat.get_table(table_name).ok_or_else(|| {
                        JarvixError::Catalog(format!("table \"{}\" does not exist", table_name))
                    })?;
                    let col = tbl.schema.column(*column_idx).ok_or_else(|| {
                        JarvixError::Catalog(format!("Column index {} out of bounds", column_idx))
                    })?;
                    (tbl.first_page_id, col.name.clone())
                };

                {
                    let mut cat = context.catalog.write().unwrap();
                    cat.create_index(name.clone(), table_name, &col_name, *unique)?;
                }

                // Initialize B+Tree index and populate with existing rows
                let btree = BTreeIndex::new(name.clone(), context.bpm.clone());
                let max_page = context.bpm.disk_manager().num_pages();

                for pid in first_pid..max_page {
                    let page_guard = context.bpm.fetch_page(pid)?;
                    if page_guard.page_type() == PageType::Heap {
                        for slot in 0..page_guard.slot_count() as u16 {
                            if let Some(bytes) = page_guard.get_tuple(slot) {
                                let rid = RecordId::new(pid, slot);
                                let tuple = Tuple::decode(bytes, Some(rid))?;
                                if let Some(key_val) = tuple.value(*column_idx) {
                                    let int_key = match key_val {
                                        Value::Integer(i) => *i as i64,
                                        Value::BigInt(i) => *i,
                                        Value::SmallInt(i) => *i as i64,
                                        Value::TinyInt(i) => *i as i64,
                                        _ => continue,
                                    };
                                    btree.insert(int_key, rid)?;
                                }
                            }
                        }
                    }
                }

                let final_root = btree.root_page_id();
                {
                    let mut cat = context.catalog.write().unwrap();
                    cat.set_index_root(name, final_root)?;
                }

                Ok("CREATE INDEX".to_string())
            }

            _ => Err(JarvixError::Execution("Non-DDL plan in DdlExecutor".to_string())),
        }
    }
}

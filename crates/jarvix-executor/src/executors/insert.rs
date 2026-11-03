use jarvix_analyzer::BoundExpr;
use jarvix_common::constants::INVALID_LSN;
use jarvix_common::error::{JarvixError, Result};
use jarvix_common::schema::{Column, Schema};
use jarvix_common::types::{DataType, Oid, RecordId, Value};
use jarvix_common::tuple::Tuple;
use jarvix_index::BTreeIndex;
use jarvix_storage::PageType;
use jarvix_txn::lock_manager::LockMode;
use jarvix_wal::LogRecord;

use crate::batch::VectorBatch;
use crate::context::ExecutionContext;
use crate::eval::eval_expr;
use crate::executors::Executor;

pub struct InsertExecutor {
    table_oid: Oid,
    table_name: String,
    column_indices: Vec<usize>,
    values: Vec<Vec<BoundExpr>>,
    context: ExecutionContext,
    executed: bool,
    schema: Schema,
}

impl InsertExecutor {
    pub fn new(
        table_oid: Oid,
        table_name: String,
        column_indices: Vec<usize>,
        values: Vec<Vec<BoundExpr>>,
        context: ExecutionContext,
    ) -> Self {
        Self {
            table_oid,
            table_name,
            column_indices,
            values,
            context,
            executed: false,
            schema: Schema::new(vec![Column::new("rows_inserted", DataType::BigInt, false)]),
        }
    }
}

impl Executor for InsertExecutor {
    fn init(&mut self) -> Result<()> {
        // Strict 2PL Exclusive lock on table
        self.context.lock_mgr.acquire_table(
            self.context.txn_id,
            LockMode::Exclusive,
            self.table_oid,
        )?;

        if let Some(txn) = self.context.txn_mgr.get_transaction(self.context.txn_id) {
            txn.record_lock(jarvix_txn::lock_manager::LockKey::Table(self.table_oid));
        }

        self.executed = false;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<VectorBatch>> {
        if self.executed {
            return Ok(None);
        }

        let (mut current_page_id, table_schema, mut indexes) = {
            let cat = self.context.catalog.read().unwrap();
            let tbl = cat.get_table(&self.table_name).ok_or_else(|| {
                JarvixError::Execution(format!("Table '{}' not found", self.table_name))
            })?;
            let idxs: Vec<_> = cat.get_table_indexes(&self.table_name).into_iter().cloned().collect();
            (tbl.first_page_id, tbl.schema.clone(), idxs)
        };

        let dummy_tuple = Tuple::new(vec![]);
        let mut inserted_count = 0usize;

        for row_exprs in &self.values {
            let mut full_row = vec![Value::Null; table_schema.column_count()];
            for (i, expr) in row_exprs.iter().enumerate() {
                let target_col = self.column_indices[i];
                let val = eval_expr(expr, &dummy_tuple)?;
                full_row[target_col] = val;
            }

            let tuple = Tuple::new(full_row.clone());
            let bytes = tuple.encode()?;

            // Insert into slotted page
            let rid = {
                let mut inserted_rid = None;
                let target_pid = current_page_id;

                while inserted_rid.is_none() {
                    let mut page_guard = self.context.bpm.fetch_page_mut(target_pid)?;
                    match page_guard.insert_tuple(&bytes) {
                        Ok(slot) => {
                            inserted_rid = Some(RecordId::new(target_pid, slot));
                            current_page_id = target_pid;
                        }
                        Err(_) => {
                            // Page is full, allocate a new page
                            drop(page_guard);
                            let (new_pid, mut new_page) =
                                self.context.bpm.new_page(PageType::Heap)?;
                            let slot = new_page.insert_tuple(&bytes)?;
                            inserted_rid = Some(RecordId::new(new_pid, slot));
                            current_page_id = new_pid;
                        }
                    }
                }
                inserted_rid.unwrap()
            };

            // Append WAL Insert record
            let prev_lsn = self
                .context
                .txn_mgr
                .get_transaction(self.context.txn_id)
                .map(|t| t.prev_lsn())
                .unwrap_or(INVALID_LSN);

            let log_record = LogRecord::new_insert(
                self.context.txn_id,
                prev_lsn,
                rid.page_id,
                rid.slot_id,
                bytes.clone(),
            );
            let lsn = self.context.wal_mgr.append_record(log_record)?;

            if let Some(txn) = self.context.txn_mgr.get_transaction(self.context.txn_id) {
                txn.set_prev_lsn(lsn);
            }

            // Update associated secondary/unique B+Tree indexes
            for idx_info in &mut indexes {
                let key_val = &full_row[idx_info.column_idx];
                let int_key = match key_val {
                    Value::Integer(i) => *i as i64,
                    Value::BigInt(i) => *i,
                    Value::SmallInt(i) => *i as i64,
                    Value::TinyInt(i) => *i as i64,
                    _ => continue,
                };

                let btree = BTreeIndex::with_root(
                    &idx_info.name,
                    self.context.bpm.clone(),
                    idx_info.root_page_id,
                );
                btree.insert(int_key, rid)?;

                let new_root = btree.root_page_id();
                if new_root != idx_info.root_page_id {
                    idx_info.root_page_id = new_root;
                    let mut cat = self.context.catalog.write().unwrap();
                    cat.set_index_root(&idx_info.name, new_root)?;
                }
            }

            inserted_count += 1;
        }

        self.executed = true;
        let result_tuple = Tuple::new(vec![Value::BigInt(inserted_count as i64)]);
        Ok(Some(VectorBatch::with_tuples(vec![result_tuple])))
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }
}

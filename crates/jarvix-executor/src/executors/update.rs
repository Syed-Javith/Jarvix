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

pub struct UpdateExecutor {
    table_oid: Oid,
    table_name: String,
    assignments: Vec<(usize, BoundExpr)>,
    scan: Box<dyn Executor>,
    context: ExecutionContext,
    executed: bool,
    schema: Schema,
}

impl UpdateExecutor {
    pub fn new(
        table_oid: Oid,
        table_name: String,
        assignments: Vec<(usize, BoundExpr)>,
        scan: Box<dyn Executor>,
        context: ExecutionContext,
    ) -> Self {
        Self {
            table_oid,
            table_name,
            assignments,
            scan,
            context,
            executed: false,
            schema: Schema::new(vec![Column::new("rows_updated", DataType::BigInt, false)]),
        }
    }
}

impl Executor for UpdateExecutor {
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

        self.scan.init()?;
        self.executed = false;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<VectorBatch>> {
        if self.executed {
            return Ok(None);
        }

        let indexes: Vec<_> = {
            let cat = self.context.catalog.read().unwrap();
            cat.get_table_indexes(&self.table_name).into_iter().cloned().collect()
        };

        let mut updated_count = 0usize;

        while let Some(batch) = self.scan.next()? {
            for tuple in batch.tuples() {
                let old_rid = tuple.rid.ok_or_else(|| {
                    JarvixError::Execution("Missing RecordId on tuple for update".to_string())
                })?;

                let old_bytes = tuple.encode()?;

                // Calculate updated values
                let mut new_values = tuple.values.clone();
                for (col_idx, expr) in &self.assignments {
                    new_values[*col_idx] = eval_expr(expr, tuple)?;
                }

                let new_tuple = Tuple::new(new_values.clone());
                let new_bytes = new_tuple.encode()?;

                // Delete old tuple and insert new tuple (Out-of-place MVCC style)
                {
                    let mut page_guard = self.context.bpm.fetch_page_mut(old_rid.page_id)?;
                    page_guard.delete_tuple(old_rid.slot_id)?;
                }

                let new_rid = {
                    let mut inserted_rid = None;
                    let target_pid = old_rid.page_id;

                    if let Ok(mut page_guard) = self.context.bpm.fetch_page_mut(target_pid) {
                        if let Ok(slot) = page_guard.insert_tuple(&new_bytes) {
                            inserted_rid = Some(RecordId::new(target_pid, slot));
                        }
                    }

                    if inserted_rid.is_none() {
                        let (new_pid, mut new_page) =
                            self.context.bpm.new_page(PageType::Heap)?;
                        let slot = new_page.insert_tuple(&new_bytes)?;
                        inserted_rid = Some(RecordId::new(new_pid, slot));
                    }

                    inserted_rid.unwrap()
                };

                // Append WAL Update record
                let prev_lsn = self
                    .context
                    .txn_mgr
                    .get_transaction(self.context.txn_id)
                    .map(|t| t.prev_lsn())
                    .unwrap_or(INVALID_LSN);

                let log_record = LogRecord::new_update(
                    self.context.txn_id,
                    prev_lsn,
                    new_rid.page_id,
                    new_rid.slot_id,
                    old_bytes,
                    new_bytes,
                );
                let lsn = self.context.wal_mgr.append_record(log_record)?;

                if let Some(txn) = self.context.txn_mgr.get_transaction(self.context.txn_id) {
                    txn.set_prev_lsn(lsn);
                }

                // Update secondary indexes
                for idx_info in &indexes {
                    let old_val = &tuple.values[idx_info.column_idx];
                    let new_val = &new_values[idx_info.column_idx];

                    if old_val != new_val {
                        let old_int = match old_val {
                            Value::Integer(i) => *i as i64,
                            Value::BigInt(i) => *i,
                            Value::SmallInt(i) => *i as i64,
                            Value::TinyInt(i) => *i as i64,
                            _ => continue,
                        };
                        let new_int = match new_val {
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
                        let _ = btree.delete(old_int);
                        btree.insert(new_int, new_rid)?;

                        let updated_root = btree.root_page_id();
                        if updated_root != idx_info.root_page_id {
                            let mut cat = self.context.catalog.write().unwrap();
                            cat.set_index_root(&idx_info.name, updated_root)?;
                        }
                    }
                }

                updated_count += 1;
            }
        }

        self.executed = true;
        let result_tuple = Tuple::new(vec![Value::BigInt(updated_count as i64)]);
        Ok(Some(VectorBatch::with_tuples(vec![result_tuple])))
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }
}

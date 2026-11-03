use jarvix_common::constants::INVALID_LSN;
use jarvix_common::error::{JarvixError, Result};
use jarvix_common::schema::{Column, Schema};
use jarvix_common::types::{DataType, Oid, Value};
use jarvix_common::tuple::Tuple;
use jarvix_index::BTreeIndex;
use jarvix_txn::lock_manager::LockMode;
use jarvix_wal::LogRecord;

use crate::batch::VectorBatch;
use crate::context::ExecutionContext;
use crate::executors::Executor;

pub struct DeleteExecutor {
    table_oid: Oid,
    table_name: String,
    scan: Box<dyn Executor>,
    context: ExecutionContext,
    executed: bool,
    schema: Schema,
}

impl DeleteExecutor {
    pub fn new(
        table_oid: Oid,
        table_name: String,
        scan: Box<dyn Executor>,
        context: ExecutionContext,
    ) -> Self {
        Self {
            table_oid,
            table_name,
            scan,
            context,
            executed: false,
            schema: Schema::new(vec![Column::new("rows_deleted", DataType::BigInt, false)]),
        }
    }
}

impl Executor for DeleteExecutor {
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

        let mut deleted_count = 0usize;

        while let Some(batch) = self.scan.next()? {
            for tuple in batch.tuples() {
                let rid = tuple.rid.ok_or_else(|| {
                    JarvixError::Execution("Missing RecordId on tuple for delete".to_string())
                })?;

                let bytes = tuple.encode()?;

                // Delete from page
                {
                    let mut page_guard = self.context.bpm.fetch_page_mut(rid.page_id)?;
                    page_guard.delete_tuple(rid.slot_id)?;
                }

                // Append WAL Delete record
                let prev_lsn = self
                    .context
                    .txn_mgr
                    .get_transaction(self.context.txn_id)
                    .map(|t| t.prev_lsn())
                    .unwrap_or(INVALID_LSN);

                let log_record = LogRecord::new_delete(
                    self.context.txn_id,
                    prev_lsn,
                    rid.page_id,
                    rid.slot_id,
                    bytes,
                );
                let lsn = self.context.wal_mgr.append_record(log_record)?;

                if let Some(txn) = self.context.txn_mgr.get_transaction(self.context.txn_id) {
                    txn.set_prev_lsn(lsn);
                }

                // Remove from B+Tree indexes
                for idx_info in &indexes {
                    let key_val = &tuple.values[idx_info.column_idx];
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
                    let _ = btree.delete(int_key);
                }

                deleted_count += 1;
            }
        }

        self.executed = true;
        let result_tuple = Tuple::new(vec![Value::BigInt(deleted_count as i64)]);
        Ok(Some(VectorBatch::with_tuples(vec![result_tuple])))
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }
}

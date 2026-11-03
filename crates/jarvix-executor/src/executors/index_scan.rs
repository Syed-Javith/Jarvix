use jarvix_analyzer::BoundExpr;
use jarvix_common::error::{JarvixError, Result};
use jarvix_common::schema::Schema;
use jarvix_common::types::{Oid, RecordId, Value};
use jarvix_common::tuple::Tuple;
use jarvix_index::BTreeIndex;
use jarvix_txn::lock_manager::LockMode;

use crate::batch::VectorBatch;
use crate::context::ExecutionContext;
use crate::eval::eval_expr;
use crate::executors::Executor;

pub struct IndexScanExecutor {
    table_name: String,
    table_oid: Oid,
    index_name: String,
    _column_idx: usize,
    key: Value,
    filter: Option<BoundExpr>,
    context: ExecutionContext,
    schema: Schema,
    matching_rids: Vec<RecordId>,
    cursor: usize,
}

impl IndexScanExecutor {
    pub fn new(
        table_name: String,
        table_oid: Oid,
        _index_oid: Oid,
        index_name: String,
        column_idx: usize,
        key: Value,
        filter: Option<BoundExpr>,
        context: ExecutionContext,
    ) -> Self {
        Self {
            table_name,
            table_oid,
            index_name,
            _column_idx: column_idx,
            key,
            filter,
            context,
            schema: Schema::new(vec![]),
            matching_rids: Vec::new(),
            cursor: 0,
        }
    }
}

impl Executor for IndexScanExecutor {
    fn init(&mut self) -> Result<()> {
        let (schema, root_pid) = {
            let cat = self.context.catalog.read().unwrap();
            let tbl = cat.get_table(&self.table_name).ok_or_else(|| {
                JarvixError::Execution(format!("Table '{}' not found", self.table_name))
            })?;
            let idx = cat.get_index(&self.index_name).ok_or_else(|| {
                JarvixError::Execution(format!("Index '{}' not found", self.index_name))
            })?;
            (tbl.schema.clone(), idx.root_page_id)
        };

        // Strict 2PL Shared lock on table
        self.context.lock_mgr.acquire_table(
            self.context.txn_id,
            LockMode::Shared,
            self.table_oid,
        )?;

        if let Some(txn) = self.context.txn_mgr.get_transaction(self.context.txn_id) {
            txn.record_lock(jarvix_txn::lock_manager::LockKey::Table(self.table_oid));
        }

        self.schema = schema;

        // Perform B+Tree point search
        let int_key = match &self.key {
            Value::Integer(i) => *i as i64,
            Value::BigInt(i) => *i,
            Value::SmallInt(i) => *i as i64,
            Value::TinyInt(i) => *i as i64,
            _ => {
                return Err(JarvixError::Execution(
                    "Only numeric BTree keys supported for IndexScan".to_string(),
                ))
            }
        };

        let btree = BTreeIndex::with_root(&self.index_name, self.context.bpm.clone(), root_pid);
        if let Some(rid) = btree.search(int_key)? {
            self.matching_rids = vec![rid];
        } else {
            self.matching_rids = Vec::new();
        }

        self.cursor = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<VectorBatch>> {
        if self.cursor >= self.matching_rids.len() {
            return Ok(None);
        }

        let mut batch = VectorBatch::new();

        while self.cursor < self.matching_rids.len() {
            let rid = self.matching_rids[self.cursor];
            self.cursor += 1;

            let page_guard = self.context.bpm.fetch_page(rid.page_id)?;
            if let Some(bytes) = page_guard.get_tuple(rid.slot_id) {
                let tuple = Tuple::decode(bytes, Some(rid))?;

                let matches = match &self.filter {
                    Some(f) => {
                        let val = eval_expr(f, &tuple)?;
                        val == Value::Boolean(true)
                    }
                    None => true,
                };

                if matches {
                    batch.push(tuple);
                }
            }
        }

        if batch.is_empty() {
            Ok(None)
        } else {
            Ok(Some(batch))
        }
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }
}

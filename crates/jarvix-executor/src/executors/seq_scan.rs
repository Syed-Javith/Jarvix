use jarvix_analyzer::BoundExpr;
use jarvix_common::error::{JarvixError, Result};
use jarvix_common::schema::Schema;
use jarvix_common::types::{Oid, PageId, RecordId, SlotId, Value};
use jarvix_common::tuple::Tuple;
use jarvix_storage::PageType;
use jarvix_txn::lock_manager::LockMode;

use crate::batch::{VectorBatch, DEFAULT_BATCH_SIZE};
use crate::context::ExecutionContext;
use crate::eval::eval_expr;
use crate::executors::Executor;

pub struct SeqScanExecutor {
    table_name: String,
    table_oid: Oid,
    filter: Option<BoundExpr>,
    context: ExecutionContext,
    schema: Schema,
    current_page_id: PageId,
    max_page_id: PageId,
    current_slot_id: SlotId,
    is_exhausted: bool,
}

impl SeqScanExecutor {
    pub fn new(
        table_name: String,
        table_oid: Oid,
        filter: Option<BoundExpr>,
        context: ExecutionContext,
    ) -> Self {
        Self {
            table_name,
            table_oid,
            filter,
            context,
            schema: Schema::new(vec![]),
            current_page_id: 0,
            max_page_id: 0,
            current_slot_id: 0,
            is_exhausted: false,
        }
    }
}

impl Executor for SeqScanExecutor {
    fn init(&mut self) -> Result<()> {
        let (first_pid, schema) = {
            let cat = self.context.catalog.read().unwrap();
            let tbl = cat.get_table(&self.table_name).ok_or_else(|| {
                JarvixError::Execution(format!("Table '{}' not found", self.table_name))
            })?;
            (tbl.first_page_id, tbl.schema.clone())
        };

        // Acquire Shared Table Lock (Strict 2PL)
        self.context.lock_mgr.acquire_table(
            self.context.txn_id,
            LockMode::Shared,
            self.table_oid,
        )?;

        // Record lock in transaction handle
        if let Some(txn) = self.context.txn_mgr.get_transaction(self.context.txn_id) {
            txn.record_lock(jarvix_txn::lock_manager::LockKey::Table(self.table_oid));
        }

        self.schema = schema;
        self.current_page_id = first_pid;
        self.max_page_id = self.context.bpm.disk_manager().num_pages();
        self.current_slot_id = 0;
        self.is_exhausted = false;

        Ok(())
    }

    fn next(&mut self) -> Result<Option<VectorBatch>> {
        if self.is_exhausted {
            return Ok(None);
        }

        let mut batch = VectorBatch::new();

        while self.current_page_id < self.max_page_id {
            let page_guard = self.context.bpm.fetch_page(self.current_page_id)?;

            if page_guard.page_type() == PageType::Heap {
                let slot_count = page_guard.slot_count() as SlotId;

                while self.current_slot_id < slot_count {
                    let slot = self.current_slot_id;
                    self.current_slot_id += 1;

                    if let Some(bytes) = page_guard.get_tuple(slot) {
                        let rid = RecordId::new(self.current_page_id, slot);
                        let tuple = Tuple::decode(bytes, Some(rid))?;

                        // Evaluate filter predicate if present
                        let matches = match &self.filter {
                            Some(f) => {
                                let val = eval_expr(f, &tuple)?;
                                val == Value::Boolean(true)
                            }
                            None => true,
                        };

                        if matches {
                            batch.push(tuple);
                            if batch.len() >= DEFAULT_BATCH_SIZE {
                                return Ok(Some(batch));
                            }
                        }
                    }
                }
            }

            self.current_page_id += 1;
            self.current_slot_id = 0;
        }

        self.is_exhausted = true;
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

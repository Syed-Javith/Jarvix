use jarvix_common::error::{JarvixError, Result};
use jarvix_common::schema::Schema;
use jarvix_common::types::Value;
use jarvix_optimizer::LogicalPlan;

use crate::context::ExecutionContext;
use crate::executors::ddl::DdlExecutor;
use crate::executors::delete::DeleteExecutor;
use crate::executors::index_scan::IndexScanExecutor;
use crate::executors::insert::InsertExecutor;
use crate::executors::limit::LimitExecutor;
use crate::executors::projection::ProjectionExecutor;
use crate::executors::seq_scan::SeqScanExecutor;
use crate::executors::update::UpdateExecutor;
use crate::executors::Executor;

#[derive(Debug, Clone)]
pub enum ExecutionResult {
    Select {
        schema: Schema,
        rows: Vec<Vec<Value>>,
    },
    Dml {
        rows_affected: usize,
    },
    Ddl {
        message: String,
    },
}

pub struct ExecutionEngine;

impl ExecutionEngine {
    /// Executes a logical plan to completion and produces an ExecutionResult.
    pub fn execute(plan: LogicalPlan, ctx: ExecutionContext) -> Result<ExecutionResult> {
        match plan {
            LogicalPlan::CreateTable { .. }
            | LogicalPlan::DropTable { .. }
            | LogicalPlan::CreateIndex { .. } => {
                let msg = DdlExecutor::execute(&plan, &ctx)?;
                Ok(ExecutionResult::Ddl { message: msg })
            }
            LogicalPlan::Insert { .. } | LogicalPlan::Update { .. } | LogicalPlan::Delete { .. } => {
                let mut executor = Self::build_pipeline(plan, ctx)?;
                executor.init()?;

                let mut rows_affected = 0;
                while let Some(batch) = executor.next()? {
                    for tuple in batch.tuples() {
                        if let Some(Value::BigInt(cnt)) = tuple.value(0) {
                            rows_affected += *cnt as usize;
                        }
                    }
                }

                Ok(ExecutionResult::Dml { rows_affected })
            }
            _ => {
                // Query execution (SELECT, Scans, Projections, Limits)
                let mut executor = Self::build_pipeline(plan, ctx)?;
                executor.init()?;

                let schema = executor.schema().clone();
                let mut rows = Vec::new();

                while let Some(batch) = executor.next()? {
                    for tuple in batch.into_tuples() {
                        rows.push(tuple.values);
                    }
                }

                Ok(ExecutionResult::Select { schema, rows })
            }
        }
    }

    /// Recursively builds the physical execution tree from a logical plan.
    fn build_pipeline(plan: LogicalPlan, ctx: ExecutionContext) -> Result<Box<dyn Executor>> {
        match plan {
            LogicalPlan::SeqScan {
                table_oid,
                table_name,
                filter,
            } => Ok(Box::new(SeqScanExecutor::new(
                table_name, table_oid, filter, ctx,
            ))),

            LogicalPlan::IndexScan {
                table_oid,
                table_name,
                index_oid,
                index_name,
                column_idx,
                key,
                filter,
            } => Ok(Box::new(IndexScanExecutor::new(
                table_name, table_oid, index_oid, index_name, column_idx, key, filter, ctx,
            ))),

            LogicalPlan::Projection {
                projection_indices,
                input,
            } => {
                let child = Self::build_pipeline(*input, ctx)?;
                Ok(Box::new(ProjectionExecutor::new(projection_indices, child)))
            }

            LogicalPlan::Limit { limit, input } => {
                let child = Self::build_pipeline(*input, ctx)?;
                Ok(Box::new(LimitExecutor::new(limit, child)))
            }

            LogicalPlan::Insert {
                table_oid,
                table_name,
                column_indices,
                values,
            } => Ok(Box::new(InsertExecutor::new(
                table_oid,
                table_name,
                column_indices,
                values,
                ctx,
            ))),

            LogicalPlan::Update {
                table_oid,
                table_name,
                assignments,
                scan,
            } => {
                let child = Self::build_pipeline(*scan, ctx.clone())?;
                Ok(Box::new(UpdateExecutor::new(
                    table_oid,
                    table_name,
                    assignments,
                    child,
                    ctx,
                )))
            }

            LogicalPlan::Delete {
                table_oid,
                table_name,
                scan,
            } => {
                let child = Self::build_pipeline(*scan, ctx.clone())?;
                Ok(Box::new(DeleteExecutor::new(
                    table_oid,
                    table_name,
                    child,
                    ctx,
                )))
            }

            LogicalPlan::CreateTable { .. }
            | LogicalPlan::DropTable { .. }
            | LogicalPlan::CreateIndex { .. } => Err(JarvixError::Execution(
                "DDL plans should not be converted to pipeline executors".to_string(),
            )),
        }
    }
}

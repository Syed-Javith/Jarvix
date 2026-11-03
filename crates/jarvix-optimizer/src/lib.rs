//! Query Optimizer and Plan Generator for Jarvix.
//!
//! Converts BoundStatements produced by semantic analysis into optimized LogicalPlans.
//! Implements index selection, filter pushdown, and projection pruning.

use jarvix_analyzer::{BoundExpr, BoundStatement};
use jarvix_catalog::Catalog;
use jarvix_common::error::Result;
use jarvix_common::schema::Schema;
use jarvix_common::types::{Oid, Value};
use jarvix_sql::ast::BinaryOperator;

#[derive(Debug, Clone, PartialEq)]
pub enum LogicalPlan {
    CreateTable {
        name: String,
        schema: Schema,
        if_not_exists: bool,
    },
    DropTable {
        name: String,
        if_exists: bool,
    },
    CreateIndex {
        name: String,
        table_name: String,
        column_idx: usize,
        unique: bool,
    },
    Insert {
        table_oid: Oid,
        table_name: String,
        column_indices: Vec<usize>,
        values: Vec<Vec<BoundExpr>>,
    },
    Update {
        table_oid: Oid,
        table_name: String,
        assignments: Vec<(usize, BoundExpr)>,
        scan: Box<LogicalPlan>,
    },
    Delete {
        table_oid: Oid,
        table_name: String,
        scan: Box<LogicalPlan>,
    },
    SeqScan {
        table_oid: Oid,
        table_name: String,
        filter: Option<BoundExpr>,
    },
    IndexScan {
        table_oid: Oid,
        table_name: String,
        index_oid: Oid,
        index_name: String,
        column_idx: usize,
        key: Value,
        filter: Option<BoundExpr>,
    },
    Projection {
        projection_indices: Vec<usize>,
        input: Box<LogicalPlan>,
    },
    Limit {
        limit: usize,
        input: Box<LogicalPlan>,
    },
}

pub struct QueryOptimizer;

impl QueryOptimizer {
    /// Transforms a bound SQL statement into an optimized logical execution plan.
    pub fn optimize(bound: BoundStatement, catalog: &Catalog) -> Result<LogicalPlan> {
        match bound {
            BoundStatement::CreateTable {
                name,
                schema,
                if_not_exists,
            } => Ok(LogicalPlan::CreateTable {
                name,
                schema,
                if_not_exists,
            }),
            BoundStatement::DropTable { name, if_exists } => {
                Ok(LogicalPlan::DropTable { name, if_exists })
            }
            BoundStatement::CreateIndex {
                name,
                table_name,
                column_idx,
                unique,
            } => Ok(LogicalPlan::CreateIndex {
                name,
                table_name,
                column_idx,
                unique,
            }),
            BoundStatement::Insert {
                table_oid,
                table_name,
                column_indices,
                values,
            } => Ok(LogicalPlan::Insert {
                table_oid,
                table_name,
                column_indices,
                values,
            }),
            BoundStatement::Update {
                table_oid,
                table_name,
                assignments,
                filter,
            } => {
                let scan = Self::optimize_scan(table_oid, &table_name, filter, catalog);
                Ok(LogicalPlan::Update {
                    table_oid,
                    table_name,
                    assignments,
                    scan: Box::new(scan),
                })
            }
            BoundStatement::Delete {
                table_oid,
                table_name,
                filter,
            } => {
                let scan = Self::optimize_scan(table_oid, &table_name, filter, catalog);
                Ok(LogicalPlan::Delete {
                    table_oid,
                    table_name,
                    scan: Box::new(scan),
                })
            }
            BoundStatement::Select {
                table_oid,
                table_name,
                projection_indices,
                filter,
                limit,
            } => {
                let scan_node = Self::optimize_scan(table_oid, &table_name, filter, catalog);

                let mut current_plan = LogicalPlan::Projection {
                    projection_indices,
                    input: Box::new(scan_node),
                };

                if let Some(l) = limit {
                    current_plan = LogicalPlan::Limit {
                        limit: l,
                        input: Box::new(current_plan),
                    };
                }

                Ok(current_plan)
            }
        }
    }

    /// Selects between SeqScan and IndexScan based on available indexes and filter predicates.
    fn optimize_scan(
        table_oid: Oid,
        table_name: &str,
        filter: Option<BoundExpr>,
        catalog: &Catalog,
    ) -> LogicalPlan {
        if let Some(ref expr) = filter {
            // Check if filter contains an equality match on an indexed column
            let indexes = catalog.get_table_indexes(table_name);
            if !indexes.is_empty() {
                if let Some((col_idx, key, remaining_filter)) = Self::extract_index_match(expr, &indexes) {
                    // Match found! Choose the index
                    let index_info = indexes.iter().find(|idx| idx.column_idx == col_idx).unwrap();
                    return LogicalPlan::IndexScan {
                        table_oid,
                        table_name: table_name.to_string(),
                        index_oid: index_info.oid,
                        index_name: index_info.name.clone(),
                        column_idx: col_idx,
                        key,
                        filter: remaining_filter,
                    };
                }
            }
        }

        LogicalPlan::SeqScan {
            table_oid,
            table_name: table_name.to_string(),
            filter,
        }
    }

    /// Recursively checks if an expression contains an indexable equality: `col = value`
    fn extract_index_match(
        expr: &BoundExpr,
        indexes: &[&jarvix_catalog::IndexInfo],
    ) -> Option<(usize, Value, Option<BoundExpr>)> {
        match expr {
            BoundExpr::BinaryOp {
                left,
                op: BinaryOperator::Eq,
                right,
                ..
            } => {
                match (left.as_ref(), right.as_ref()) {
                    (BoundExpr::ColumnRef { col_idx, .. }, BoundExpr::Literal(val))
                    | (BoundExpr::Literal(val), BoundExpr::ColumnRef { col_idx, .. }) => {
                        if indexes.iter().any(|idx| idx.column_idx == *col_idx) {
                            return Some((*col_idx, val.clone(), None));
                        }
                    }
                    _ => {}
                }
                None
            }
            BoundExpr::BinaryOp {
                left,
                op: BinaryOperator::And,
                right,
                ..
            } => {
                // Try left side
                if let Some((col, val, rem)) = Self::extract_index_match(left, indexes) {
                    let new_rem = match rem {
                        Some(r) => Some(BoundExpr::BinaryOp {
                            left: Box::new(r),
                            op: BinaryOperator::And,
                            right: right.clone(),
                            return_type: jarvix_common::types::DataType::Boolean,
                        }),
                        None => Some(*right.clone()),
                    };
                    return Some((col, val, new_rem));
                }
                // Try right side
                if let Some((col, val, rem)) = Self::extract_index_match(right, indexes) {
                    let new_rem = match rem {
                        Some(r) => Some(BoundExpr::BinaryOp {
                            left: left.clone(),
                            op: BinaryOperator::And,
                            right: Box::new(r),
                            return_type: jarvix_common::types::DataType::Boolean,
                        }),
                        None => Some(*left.clone()),
                    };
                    return Some((col, val, new_rem));
                }
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jarvix_analyzer::SemanticAnalyzer;
    use jarvix_common::schema::Column;
    use jarvix_common::types::DataType;
    use jarvix_sql::Parser;

    fn setup_test_catalog() -> Catalog {
        let mut catalog = Catalog::new();
        let schema = Schema::new(vec![
            Column::new("id", DataType::Integer, false),
            Column::new("name", DataType::Varchar(50), false),
            Column::new("score", DataType::Integer, true),
        ]);
        catalog.create_table("users", schema, 1).unwrap();
        catalog.create_index("users_id_idx", "users", "id", true).unwrap();
        catalog
    }

    #[test]
    fn test_optimize_seq_scan() {
        let catalog = setup_test_catalog();
        let sql = "SELECT id, name FROM users WHERE score > 50;";
        let stmt = Parser::parse_sql(sql).unwrap();
        let bound = SemanticAnalyzer::analyze(&stmt, &catalog).unwrap();
        let plan = QueryOptimizer::optimize(bound, &catalog).unwrap();

        match plan {
            LogicalPlan::Projection { projection_indices, input } => {
                assert_eq!(projection_indices, vec![0, 1]);
                match *input {
                    LogicalPlan::SeqScan { table_name, filter, .. } => {
                        assert_eq!(table_name, "users");
                        assert!(filter.is_some());
                    }
                    _ => panic!("Expected SeqScan"),
                }
            }
            _ => panic!("Expected Projection"),
        }
    }

    #[test]
    fn test_optimize_index_scan() {
        let catalog = setup_test_catalog();
        let sql = "SELECT name FROM users WHERE id = 42;";
        let stmt = Parser::parse_sql(sql).unwrap();
        let bound = SemanticAnalyzer::analyze(&stmt, &catalog).unwrap();
        let plan = QueryOptimizer::optimize(bound, &catalog).unwrap();

        match plan {
            LogicalPlan::Projection { projection_indices, input } => {
                assert_eq!(projection_indices, vec![1]);
                match *input {
                    LogicalPlan::IndexScan {
                        table_name,
                        index_name,
                        column_idx,
                        key,
                        filter,
                        ..
                    } => {
                        assert_eq!(table_name, "users");
                        assert_eq!(index_name, "users_id_idx");
                        assert_eq!(column_idx, 0);
                        assert_eq!(key, Value::Integer(42));
                        assert!(filter.is_none());
                    }
                    _ => panic!("Expected IndexScan"),
                }
            }
            _ => panic!("Expected Projection"),
        }
    }
}

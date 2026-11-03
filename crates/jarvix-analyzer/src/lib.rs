//! Semantic Analyzer and Catalog Binding for Jarvix.

use jarvix_catalog::Catalog;
use jarvix_common::error::{JarvixError, Result};
use jarvix_common::schema::{Column, Schema};
use jarvix_common::types::{DataType, Oid, Value};
use jarvix_sql::ast::{BinaryOperator, Expr, SelectItem, Statement, UnaryOperator};

#[derive(Debug, Clone, PartialEq)]
pub enum BoundExpr {
    Literal(Value),
    ColumnRef {
        col_idx: usize,
        col_name: String,
        data_type: DataType,
    },
    BinaryOp {
        left: Box<BoundExpr>,
        op: BinaryOperator,
        right: Box<BoundExpr>,
        return_type: DataType,
    },
    UnaryOp {
        op: UnaryOperator,
        expr: Box<BoundExpr>,
        return_type: DataType,
    },
}

impl BoundExpr {
    pub fn return_type(&self) -> DataType {
        match self {
            BoundExpr::Literal(v) => v.data_type(),
            BoundExpr::ColumnRef { data_type, .. } => *data_type,
            BoundExpr::BinaryOp { return_type, .. } => *return_type,
            BoundExpr::UnaryOp { return_type, .. } => *return_type,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BoundStatement {
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
    Select {
        table_oid: Oid,
        table_name: String,
        projection_indices: Vec<usize>,
        filter: Option<BoundExpr>,
        limit: Option<usize>,
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
        filter: Option<BoundExpr>,
    },
    Delete {
        table_oid: Oid,
        table_name: String,
        filter: Option<BoundExpr>,
    },
}

pub struct SemanticAnalyzer;

impl SemanticAnalyzer {
    pub fn analyze(stmt: &Statement, catalog: &Catalog) -> Result<BoundStatement> {
        match stmt {
            Statement::CreateTable {
                name,
                columns,
                if_not_exists,
            } => {
                let schema_cols = columns
                    .iter()
                    .map(|c| Column::new(c.name.clone(), c.data_type, c.nullable))
                    .collect();
                Ok(BoundStatement::CreateTable {
                    name: name.clone(),
                    schema: Schema::new(schema_cols),
                    if_not_exists: *if_not_exists,
                })
            }
            Statement::DropTable { name, if_exists } => Ok(BoundStatement::DropTable {
                name: name.clone(),
                if_exists: *if_exists,
            }),
            Statement::CreateIndex {
                name,
                table,
                column,
                unique,
            } => {
                let tbl = catalog.get_table(table).ok_or_else(|| {
                    JarvixError::Semantic(format!("Table '{}' not found in catalog", table))
                })?;
                let col_idx = tbl.schema.column_index(column).ok_or_else(|| {
                    JarvixError::Semantic(format!(
                        "Column '{}' not found in table '{}'",
                        column, table
                    ))
                })?;
                Ok(BoundStatement::CreateIndex {
                    name: name.clone(),
                    table_name: tbl.name.clone(),
                    column_idx: col_idx,
                    unique: *unique,
                })
            }
            Statement::Select {
                projections,
                from,
                filter,
                limit,
            } => {
                let tbl = catalog.get_table(from).ok_or_else(|| {
                    JarvixError::Semantic(format!("Table '{}' not found", from))
                })?;

                let mut proj_indices = Vec::new();
                for item in projections {
                    match item {
                        SelectItem::Wildcard => {
                            for i in 0..tbl.schema.column_count() {
                                proj_indices.push(i);
                            }
                        }
                        SelectItem::Expr { expr, .. } => match expr {
                            Expr::Column(name) => {
                                let idx = tbl.schema.column_index(name).ok_or_else(|| {
                                    JarvixError::Semantic(format!(
                                        "Column '{}' not found in table '{}'",
                                        name, from
                                    ))
                                })?;
                                proj_indices.push(idx);
                            }
                            _ => {
                                return Err(JarvixError::Semantic(
                                    "Complex expressions in projection not yet supported".to_string(),
                                ));
                            }
                        },
                    }
                }

                let bound_filter = match filter {
                    Some(e) => Some(Self::bind_expr(e, &tbl.schema)?),
                    None => None,
                };

                Ok(BoundStatement::Select {
                    table_oid: tbl.oid,
                    table_name: tbl.name.clone(),
                    projection_indices: proj_indices,
                    filter: bound_filter,
                    limit: *limit,
                })
            }
            Statement::Insert {
                table,
                columns,
                values,
            } => {
                let tbl = catalog.get_table(table).ok_or_else(|| {
                    JarvixError::Semantic(format!("Table '{}' not found", table))
                })?;

                let col_indices = match columns {
                    Some(cols) => {
                        let mut indices = Vec::new();
                        for col in cols {
                            let idx = tbl.schema.column_index(col).ok_or_else(|| {
                                JarvixError::Semantic(format!(
                                    "Column '{}' not found in table '{}'",
                                    col, table
                                ))
                            })?;
                            indices.push(idx);
                        }
                        indices
                    }
                    None => (0..tbl.schema.column_count()).collect(),
                };

                let mut bound_values = Vec::new();
                for row in values {
                    if row.len() != col_indices.len() {
                        return Err(JarvixError::Semantic(format!(
                            "Insert row length {} does not match column count {}",
                            row.len(),
                            col_indices.len()
                        )));
                    }
                    let mut bound_row = Vec::new();
                    for expr in row {
                        bound_row.push(Self::bind_expr(expr, &tbl.schema)?);
                    }
                    bound_values.push(bound_row);
                }

                Ok(BoundStatement::Insert {
                    table_oid: tbl.oid,
                    table_name: tbl.name.clone(),
                    column_indices: col_indices,
                    values: bound_values,
                })
            }
            Statement::Update {
                table,
                assignments,
                filter,
            } => {
                let tbl = catalog.get_table(table).ok_or_else(|| {
                    JarvixError::Semantic(format!("Table '{}' not found", table))
                })?;

                let mut bound_assigns = Vec::new();
                for (col, expr) in assignments {
                    let idx = tbl.schema.column_index(col).ok_or_else(|| {
                        JarvixError::Semantic(format!(
                            "Column '{}' not found in table '{}'",
                            col, table
                        ))
                    })?;
                    let bound_e = Self::bind_expr(expr, &tbl.schema)?;
                    bound_assigns.push((idx, bound_e));
                }

                let bound_filter = match filter {
                    Some(e) => Some(Self::bind_expr(e, &tbl.schema)?),
                    None => None,
                };

                Ok(BoundStatement::Update {
                    table_oid: tbl.oid,
                    table_name: tbl.name.clone(),
                    assignments: bound_assigns,
                    filter: bound_filter,
                })
            }
            Statement::Delete { table, filter } => {
                let tbl = catalog.get_table(table).ok_or_else(|| {
                    JarvixError::Semantic(format!("Table '{}' not found", table))
                })?;

                let bound_filter = match filter {
                    Some(e) => Some(Self::bind_expr(e, &tbl.schema)?),
                    None => None,
                };

                Ok(BoundStatement::Delete {
                    table_oid: tbl.oid,
                    table_name: tbl.name.clone(),
                    filter: bound_filter,
                })
            }
        }
    }

    fn bind_expr(expr: &Expr, schema: &Schema) -> Result<BoundExpr> {
        match expr {
            Expr::Literal(val) => Ok(BoundExpr::Literal(val.clone())),
            Expr::Column(name) => {
                let idx = schema.column_index(name).ok_or_else(|| {
                    JarvixError::Semantic(format!("Column '{}' not found in schema", name))
                })?;
                let col = schema.column(idx).unwrap();
                Ok(BoundExpr::ColumnRef {
                    col_idx: idx,
                    col_name: col.name.clone(),
                    data_type: col.data_type,
                })
            }
            Expr::BinaryOp { left, op, right } => {
                let bound_left = Self::bind_expr(left, schema)?;
                let bound_right = Self::bind_expr(right, schema)?;

                let return_type = match op {
                    BinaryOperator::Eq
                    | BinaryOperator::NotEq
                    | BinaryOperator::Lt
                    | BinaryOperator::LtEq
                    | BinaryOperator::Gt
                    | BinaryOperator::GtEq
                    | BinaryOperator::And
                    | BinaryOperator::Or => DataType::Boolean,
                    BinaryOperator::Plus
                    | BinaryOperator::Minus
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide => bound_left.return_type(),
                };

                Ok(BoundExpr::BinaryOp {
                    left: Box::new(bound_left),
                    op: *op,
                    right: Box::new(bound_right),
                    return_type,
                })
            }
            Expr::UnaryOp { op, expr } => {
                let bound_inner = Self::bind_expr(expr, schema)?;
                let return_type = match op {
                    UnaryOperator::Not => DataType::Boolean,
                    UnaryOperator::Neg => bound_inner.return_type(),
                };
                Ok(BoundExpr::UnaryOp {
                    op: *op,
                    expr: Box::new(bound_inner),
                    return_type,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jarvix_sql::Parser;

    #[test]
    fn test_semantic_analysis_select() {
        let mut catalog = Catalog::new();
        let schema = Schema::new(vec![
            Column::new("id", DataType::Integer, false),
            Column::new("name", DataType::Varchar(50), false),
            Column::new("age", DataType::Integer, true),
        ]);
        catalog.create_table("users", schema, 1).unwrap();

        let sql = "SELECT id, age FROM users WHERE age >= 21;";
        let stmt = Parser::parse_sql(sql).unwrap();

        let bound = SemanticAnalyzer::analyze(&stmt, &catalog).unwrap();
        match bound {
            BoundStatement::Select {
                table_oid,
                projection_indices,
                filter,
                ..
            } => {
                assert_eq!(table_oid, 1000);
                assert_eq!(projection_indices, vec![0, 2]); // 'id' (0), 'age' (2)
                assert!(filter.is_some());
            }
            _ => panic!("Expected BoundStatement::Select"),
        }
    }
}

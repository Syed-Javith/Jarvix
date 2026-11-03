use jarvix_analyzer::BoundExpr;
use jarvix_common::error::{JarvixError, Result};
use jarvix_common::tuple::Tuple;
use jarvix_common::types::Value;
use jarvix_sql::ast::{BinaryOperator, UnaryOperator};

/// Evaluates a bound expression against a tuple row.
pub fn eval_expr(expr: &BoundExpr, tuple: &Tuple) -> Result<Value> {
    match expr {
        BoundExpr::Literal(val) => Ok(val.clone()),
        BoundExpr::ColumnRef { col_idx, .. } => tuple
            .value(*col_idx)
            .cloned()
            .ok_or_else(|| JarvixError::Execution(format!("Column index {} out of bounds", col_idx))),
        BoundExpr::BinaryOp { left, op, right, .. } => {
            let left_val = eval_expr(left, tuple)?;
            let right_val = eval_expr(right, tuple)?;
            eval_binary_op(&left_val, *op, &right_val)
        }
        BoundExpr::UnaryOp { op, expr, .. } => {
            let val = eval_expr(expr, tuple)?;
            eval_unary_op(*op, &val)
        }
    }
}

fn eval_binary_op(left: &Value, op: BinaryOperator, right: &Value) -> Result<Value> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }

    match op {
        BinaryOperator::Eq => Ok(Value::Boolean(left == right)),
        BinaryOperator::NotEq => Ok(Value::Boolean(left != right)),
        BinaryOperator::Lt => Ok(Value::Boolean(left < right)),
        BinaryOperator::LtEq => Ok(Value::Boolean(left <= right)),
        BinaryOperator::Gt => Ok(Value::Boolean(left > right)),
        BinaryOperator::GtEq => Ok(Value::Boolean(left >= right)),
        BinaryOperator::And => match (left, right) {
            (Value::Boolean(a), Value::Boolean(b)) => Ok(Value::Boolean(*a && *b)),
            _ => Err(JarvixError::Execution("AND expects boolean operands".to_string())),
        },
        BinaryOperator::Or => match (left, right) {
            (Value::Boolean(a), Value::Boolean(b)) => Ok(Value::Boolean(*a || *b)),
            _ => Err(JarvixError::Execution("OR expects boolean operands".to_string())),
        },
        BinaryOperator::Plus => match (left, right) {
            (Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a + b)),
            (Value::BigInt(a), Value::BigInt(b)) => Ok(Value::BigInt(a + b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (Value::Double(a), Value::Double(b)) => Ok(Value::Double(a + b)),
            _ => Err(JarvixError::Execution("Unsupported operands for +".to_string())),
        },
        BinaryOperator::Minus => match (left, right) {
            (Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a - b)),
            (Value::BigInt(a), Value::BigInt(b)) => Ok(Value::BigInt(a - b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
            (Value::Double(a), Value::Double(b)) => Ok(Value::Double(a - b)),
            _ => Err(JarvixError::Execution("Unsupported operands for -".to_string())),
        },
        BinaryOperator::Multiply => match (left, right) {
            (Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a * b)),
            (Value::BigInt(a), Value::BigInt(b)) => Ok(Value::BigInt(a * b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
            (Value::Double(a), Value::Double(b)) => Ok(Value::Double(a * b)),
            _ => Err(JarvixError::Execution("Unsupported operands for *".to_string())),
        },
        BinaryOperator::Divide => match (left, right) {
            (Value::Integer(a), Value::Integer(b)) => {
                if *b == 0 {
                    return Err(JarvixError::Execution("Division by zero".to_string()));
                }
                Ok(Value::Integer(a / b))
            }
            (Value::BigInt(a), Value::BigInt(b)) => {
                if *b == 0 {
                    return Err(JarvixError::Execution("Division by zero".to_string()));
                }
                Ok(Value::BigInt(a / b))
            }
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
            (Value::Double(a), Value::Double(b)) => Ok(Value::Double(a / b)),
            _ => Err(JarvixError::Execution("Unsupported operands for /".to_string())),
        },
    }
}

fn eval_unary_op(op: UnaryOperator, val: &Value) -> Result<Value> {
    if val.is_null() {
        return Ok(Value::Null);
    }
    match op {
        UnaryOperator::Not => match val {
            Value::Boolean(b) => Ok(Value::Boolean(!b)),
            _ => Err(JarvixError::Execution("NOT expects boolean operand".to_string())),
        },
        UnaryOperator::Neg => match val {
            Value::Integer(i) => Ok(Value::Integer(-i)),
            Value::BigInt(i) => Ok(Value::BigInt(-i)),
            Value::Float(f) => Ok(Value::Float(-f)),
            Value::Double(d) => Ok(Value::Double(-d)),
            _ => Err(JarvixError::Execution("Negation expects numeric operand".to_string())),
        },
    }
}

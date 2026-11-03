//! Vectorized Query Execution Engine for Jarvix.
//!
//! Features vectorized Volcano execution model, index scans, filter pushdowns,
//! updates, deletes, strict 2PL locking, and ARIES-compliant WAL logging.

pub mod batch;
pub mod context;
pub mod engine;
pub mod eval;
pub mod executors;

pub use batch::VectorBatch;
pub use context::ExecutionContext;
pub use engine::{ExecutionEngine, ExecutionResult};
pub use executors::Executor;

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};
    use jarvix_analyzer::SemanticAnalyzer;
    use jarvix_buffer::BufferPoolManager;
    use jarvix_catalog::Catalog;
    use jarvix_common::types::Value;
    use jarvix_optimizer::QueryOptimizer;
    use jarvix_sql::Parser;
    use jarvix_storage::DiskManager;
    use jarvix_txn::lock_manager::LockManager;
    use jarvix_txn::transaction_manager::TransactionManager;
    use jarvix_txn::undo::UndoLogManager;
    use jarvix_wal::WalManager;

    use super::*;

    fn setup_test_engine() -> (
        Arc<RwLock<Catalog>>,
        BufferPoolManager,
        Arc<TransactionManager>,
        Arc<WalManager>,
        Arc<LockManager>,
        std::path::PathBuf,
    ) {
        let test_dir = std::env::temp_dir().join(format!("jarvix_exec_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&test_dir).unwrap();

        let db_path = test_dir.join("test.db");
        let wal_path = test_dir.join("test.wal");

        let disk_mgr = Arc::new(DiskManager::open(&db_path).unwrap());
        let bpm = BufferPoolManager::new(64, disk_mgr);
        let catalog = Arc::new(RwLock::new(Catalog::new()));
        let wal_mgr = Arc::new(WalManager::open(&wal_path).unwrap());
        let lock_mgr = Arc::new(LockManager::new());
        let undo_mgr = Arc::new(UndoLogManager::new());
        let txn_mgr = Arc::new(TransactionManager::new(
            wal_mgr.clone(),
            lock_mgr.clone(),
            undo_mgr,
        ));

        (catalog, bpm, txn_mgr, wal_mgr, lock_mgr, test_dir)
    }

    #[test]
    fn test_executor_full_crud_lifecycle() {
        let (catalog, bpm, txn_mgr, wal_mgr, lock_mgr, _test_dir) = setup_test_engine();

        // 1. Begin Txn 1: CREATE TABLE
        println!("Starting step 1: CREATE TABLE");
        let txn1 = txn_mgr.begin(jarvix_txn::IsolationLevel::ReadCommitted).unwrap();
        let ctx1 = ExecutionContext::new(
            bpm.clone(),
            catalog.clone(),
            txn_mgr.clone(),
            wal_mgr.clone(),
            lock_mgr.clone(),
            txn1.txn_id(),
        );

        let ddl_sql = "CREATE TABLE employees (id INT, name VARCHAR, salary INT);";
        let stmt = Parser::parse_sql(ddl_sql).unwrap();
        let bound = SemanticAnalyzer::analyze(&stmt, &catalog.read().unwrap()).unwrap();
        let plan = QueryOptimizer::optimize(bound, &catalog.read().unwrap()).unwrap();
        let res = ExecutionEngine::execute(plan, ctx1).unwrap();
        match res {
            ExecutionResult::Ddl { message } => assert_eq!(message, "CREATE TABLE"),
            _ => panic!("Expected DDL result"),
        }
        txn_mgr.commit(&txn1).unwrap();
        println!("Finished step 1");

        // 2. Begin Txn 2: INSERT Rows
        println!("Starting step 2: INSERT");
        let txn2 = txn_mgr.begin(jarvix_txn::IsolationLevel::ReadCommitted).unwrap();
        let ctx2 = ExecutionContext::new(
            bpm.clone(),
            catalog.clone(),
            txn_mgr.clone(),
            wal_mgr.clone(),
            lock_mgr.clone(),
            txn2.txn_id(),
        );

        let insert_sql = "INSERT INTO employees VALUES (101, 'Alice', 95000), (102, 'Bob', 80000), (103, 'Charlie', 120000);";
        let stmt = Parser::parse_sql(insert_sql).unwrap();
        let bound = SemanticAnalyzer::analyze(&stmt, &catalog.read().unwrap()).unwrap();
        let plan = QueryOptimizer::optimize(bound, &catalog.read().unwrap()).unwrap();
        let res = ExecutionEngine::execute(plan, ctx2).unwrap();
        match res {
            ExecutionResult::Dml { rows_affected } => assert_eq!(rows_affected, 3),
            _ => panic!("Expected DML result"),
        }
        txn_mgr.commit(&txn2).unwrap();
        println!("Finished step 2");

        // 3. Begin Txn 3: SELECT with WHERE filter
        println!("Starting step 3: SELECT");
        let txn3 = txn_mgr.begin(jarvix_txn::IsolationLevel::ReadCommitted).unwrap();
        println!("Step 3: txn3 begun");
        let ctx3 = ExecutionContext::new(
            bpm.clone(),
            catalog.clone(),
            txn_mgr.clone(),
            wal_mgr.clone(),
            lock_mgr.clone(),
            txn3.txn_id(),
        );

        let select_sql = "SELECT id, name, salary FROM employees WHERE salary > 90000;";
        let stmt = Parser::parse_sql(select_sql).unwrap();
        println!("Step 3: parsed SQL");
        let bound = SemanticAnalyzer::analyze(&stmt, &catalog.read().unwrap()).unwrap();
        println!("Step 3: analyzed SQL");
        let plan = QueryOptimizer::optimize(bound, &catalog.read().unwrap()).unwrap();
        println!("Step 3: optimized SQL");
        let res = ExecutionEngine::execute(plan, ctx3).unwrap();
        println!("Step 3: executed engine");
        match res {
            ExecutionResult::Select { rows, .. } => {
                assert_eq!(rows.len(), 2); // Alice (95000) and Charlie (120000)
            }
            _ => panic!("Expected Select result"),
        }
        txn_mgr.commit(&txn3).unwrap();

        // 4. Begin Txn 4: UPDATE salary
        let txn4 = txn_mgr.begin(jarvix_txn::IsolationLevel::ReadCommitted).unwrap();
        let ctx4 = ExecutionContext::new(
            bpm.clone(),
            catalog.clone(),
            txn_mgr.clone(),
            wal_mgr.clone(),
            lock_mgr.clone(),
            txn4.txn_id(),
        );

        let update_sql = "UPDATE employees SET salary = 100000 WHERE id = 101;";
        let stmt = Parser::parse_sql(update_sql).unwrap();
        let bound = SemanticAnalyzer::analyze(&stmt, &catalog.read().unwrap()).unwrap();
        let plan = QueryOptimizer::optimize(bound, &catalog.read().unwrap()).unwrap();
        let res = ExecutionEngine::execute(plan, ctx4).unwrap();
        match res {
            ExecutionResult::Dml { rows_affected } => assert_eq!(rows_affected, 1),
            _ => panic!("Expected DML result"),
        }
        txn_mgr.commit(&txn4).unwrap();

        // 5. Begin Txn 5: DELETE Bob
        let txn5 = txn_mgr.begin(jarvix_txn::IsolationLevel::ReadCommitted).unwrap();
        let ctx5 = ExecutionContext::new(
            bpm.clone(),
            catalog.clone(),
            txn_mgr.clone(),
            wal_mgr.clone(),
            lock_mgr.clone(),
            txn5.txn_id(),
        );

        let delete_sql = "DELETE FROM employees WHERE id = 102;";
        let stmt = Parser::parse_sql(delete_sql).unwrap();
        let bound = SemanticAnalyzer::analyze(&stmt, &catalog.read().unwrap()).unwrap();
        let plan = QueryOptimizer::optimize(bound, &catalog.read().unwrap()).unwrap();
        let res = ExecutionEngine::execute(plan, ctx5).unwrap();
        match res {
            ExecutionResult::Dml { rows_affected } => assert_eq!(rows_affected, 1),
            _ => panic!("Expected DML result"),
        }
        txn_mgr.commit(&txn5).unwrap();

        // 6. Verify remaining rows
        let txn6 = txn_mgr.begin(jarvix_txn::IsolationLevel::ReadCommitted).unwrap();
        let ctx6 = ExecutionContext::new(
            bpm.clone(),
            catalog.clone(),
            txn_mgr.clone(),
            wal_mgr.clone(),
            lock_mgr.clone(),
            txn6.txn_id(),
        );

        let select_all = "SELECT id, name, salary FROM employees;";
        let stmt = Parser::parse_sql(select_all).unwrap();
        let bound = SemanticAnalyzer::analyze(&stmt, &catalog.read().unwrap()).unwrap();
        let plan = QueryOptimizer::optimize(bound, &catalog.read().unwrap()).unwrap();
        let res = ExecutionEngine::execute(plan, ctx6).unwrap();
        match res {
            ExecutionResult::Select { rows, .. } => {
                assert_eq!(rows.len(), 2); // Alice and Charlie remain
                let alice_row = rows.iter().find(|r| r[0] == Value::Integer(101)).unwrap();
                assert_eq!(alice_row[2], Value::Integer(100000)); // Updated salary
            }
            _ => panic!("Expected Select result"),
        }
        txn_mgr.commit(&txn6).unwrap();
    }

    #[test]
    fn test_executor_index_scan_lifecycle() {
        let (catalog, bpm, txn_mgr, wal_mgr, lock_mgr, _test_dir) = setup_test_engine();

        // 1. Create table
        let txn1 = txn_mgr.begin(jarvix_txn::IsolationLevel::ReadCommitted).unwrap();
        let ctx1 = ExecutionContext::new(
            bpm.clone(),
            catalog.clone(),
            txn_mgr.clone(),
            wal_mgr.clone(),
            lock_mgr.clone(),
            txn1.txn_id(),
        );

        let ddl_sql = "CREATE TABLE products (id INT, name VARCHAR, price INT);";
        let stmt = Parser::parse_sql(ddl_sql).unwrap();
        let bound = SemanticAnalyzer::analyze(&stmt, &catalog.read().unwrap()).unwrap();
        let plan = QueryOptimizer::optimize(bound, &catalog.read().unwrap()).unwrap();
        ExecutionEngine::execute(plan, ctx1).unwrap();
        txn_mgr.commit(&txn1).unwrap();

        // 2. Create index
        let txn2 = txn_mgr.begin(jarvix_txn::IsolationLevel::ReadCommitted).unwrap();
        let ctx2 = ExecutionContext::new(
            bpm.clone(),
            catalog.clone(),
            txn_mgr.clone(),
            wal_mgr.clone(),
            lock_mgr.clone(),
            txn2.txn_id(),
        );

        let ddl_idx = "CREATE INDEX idx_prod_id ON products (id);";
        let stmt = Parser::parse_sql(ddl_idx).unwrap();
        let bound = SemanticAnalyzer::analyze(&stmt, &catalog.read().unwrap()).unwrap();
        let plan = QueryOptimizer::optimize(bound, &catalog.read().unwrap()).unwrap();
        ExecutionEngine::execute(plan, ctx2).unwrap();
        txn_mgr.commit(&txn2).unwrap();

        // 3. Insert rows
        let txn3 = txn_mgr.begin(jarvix_txn::IsolationLevel::ReadCommitted).unwrap();
        let ctx3 = ExecutionContext::new(
            bpm.clone(),
            catalog.clone(),
            txn_mgr.clone(),
            wal_mgr.clone(),
            lock_mgr.clone(),
            txn3.txn_id(),
        );

        let insert_sql = "INSERT INTO products VALUES (10, 'Book', 25), (20, 'Laptop', 1200), (30, 'Phone', 800);";
        let stmt = Parser::parse_sql(insert_sql).unwrap();
        let bound = SemanticAnalyzer::analyze(&stmt, &catalog.read().unwrap()).unwrap();
        let plan = QueryOptimizer::optimize(bound, &catalog.read().unwrap()).unwrap();
        ExecutionEngine::execute(plan, ctx3).unwrap();
        txn_mgr.commit(&txn3).unwrap();

        // 4. Query via index scan (WHERE id = 20)
        let txn4 = txn_mgr.begin(jarvix_txn::IsolationLevel::ReadCommitted).unwrap();
        let ctx4 = ExecutionContext::new(
            bpm.clone(),
            catalog.clone(),
            txn_mgr.clone(),
            wal_mgr.clone(),
            lock_mgr.clone(),
            txn4.txn_id(),
        );

        let select_sql = "SELECT id, name, price FROM products WHERE id = 20;";
        let stmt = Parser::parse_sql(select_sql).unwrap();
        let bound = SemanticAnalyzer::analyze(&stmt, &catalog.read().unwrap()).unwrap();
        let plan = QueryOptimizer::optimize(bound, &catalog.read().unwrap()).unwrap();

        // Verify the optimizer chose IndexScan
        match &plan {
            jarvix_optimizer::LogicalPlan::Projection { input, .. } => {
                match input.as_ref() {
                    jarvix_optimizer::LogicalPlan::IndexScan { index_name, .. } => {
                        assert_eq!(index_name, "idx_prod_id");
                    }
                    _ => panic!("Expected IndexScan in plan"),
                }
            }
            _ => panic!("Expected Projection"),
        }

        let res = ExecutionEngine::execute(plan, ctx4).unwrap();
        match res {
            ExecutionResult::Select { rows, .. } => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][0], Value::Integer(20));
                assert_eq!(rows[0][1], Value::Text("Laptop".to_string()));
                assert_eq!(rows[0][2], Value::Integer(1200));
            }
            _ => panic!("Expected Select result"),
        }
        txn_mgr.commit(&txn4).unwrap();
    }
}

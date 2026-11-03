use std::io::Write;
use std::net::TcpStream;
use std::sync::{Arc, RwLock};

use jarvix_analyzer::SemanticAnalyzer;
use jarvix_buffer::BufferPoolManager;
use jarvix_catalog::Catalog;
use jarvix_common::error::Result;
use jarvix_common::types::Value;
use jarvix_executor::{ExecutionContext, ExecutionEngine, ExecutionResult};
use jarvix_optimizer::QueryOptimizer;
use jarvix_protocol::{
    BackendMessage, FieldDescription, FrontendMessage, FrontendParser,
};
use jarvix_sql::Parser;
use jarvix_txn::lock_manager::LockManager;
use jarvix_txn::mvcc::IsolationLevel;
use jarvix_txn::transaction_manager::{Transaction, TransactionManager};
use jarvix_wal::WalManager;

pub struct ClientSession {
    stream: TcpStream,
    bpm: BufferPoolManager,
    catalog: Arc<RwLock<Catalog>>,
    txn_mgr: Arc<TransactionManager>,
    wal_mgr: Arc<WalManager>,
    lock_mgr: Arc<LockManager>,
    active_txn: Option<Arc<Transaction>>,
}

impl ClientSession {
    pub fn new(
        stream: TcpStream,
        bpm: BufferPoolManager,
        catalog: Arc<RwLock<Catalog>>,
        txn_mgr: Arc<TransactionManager>,
        wal_mgr: Arc<WalManager>,
        lock_mgr: Arc<LockManager>,
    ) -> Self {
        Self {
            stream,
            bpm,
            catalog,
            txn_mgr,
            wal_mgr,
            lock_mgr,
            active_txn: None,
        }
    }

    /// Drives the full lifecycle of a client connection session.
    pub fn run(&mut self) -> Result<()> {
        self.handle_handshake()?;
        self.command_loop()?;
        Ok(())
    }

    fn send_message(&mut self, msg: BackendMessage) -> Result<()> {
        let bytes = msg.encode()?;
        self.stream.write_all(&bytes)?;
        self.stream.flush()?;
        Ok(())
    }

    fn handle_handshake(&mut self) -> Result<()> {
        // Read initial packet (could be SSLRequest or StartupMessage)
        let mut msg = FrontendParser::read_startup(&mut self.stream)?;

        if let FrontendMessage::SslRequest = msg {
            // Reply with 'N' (SSL not supported)
            self.stream.write_all(b"N")?;
            self.stream.flush()?;
            // Then read the actual StartupMessage
            msg = FrontendParser::read_startup(&mut self.stream)?;
        }

        match msg {
            FrontendMessage::Startup { .. } => {
                self.send_message(BackendMessage::AuthenticationOk)?;
                self.send_message(BackendMessage::ParameterStatus {
                    name: "server_version".to_string(),
                    value: "14.0 (Jarvix DB Engine)".to_string(),
                })?;
                self.send_message(BackendMessage::ParameterStatus {
                    name: "client_encoding".to_string(),
                    value: "UTF8".to_string(),
                })?;
                self.send_message(BackendMessage::BackendKeyData {
                    process_id: std::process::id(),
                    secret_key: 4242,
                })?;
                self.send_message(BackendMessage::ReadyForQuery('I'))?;
                Ok(())
            }
            _ => Err(jarvix_common::error::JarvixError::Protocol(
                "Expected StartupMessage".to_string(),
            )),
        }
    }

    fn command_loop(&mut self) -> Result<()> {
        loop {
            let msg = match FrontendParser::read_message(&mut self.stream) {
                Ok(m) => m,
                Err(_) => break, // Connection closed by client
            };

            match msg {
                FrontendMessage::Query(sql) => {
                    self.execute_query(&sql)?;
                }
                FrontendMessage::Terminate => {
                    break;
                }
                _ => {
                    self.send_message(BackendMessage::ReadyForQuery(self.status_char()))?;
                }
            }
        }

        // Clean up any uncommitted active transaction
        if let Some(txn) = self.active_txn.take() {
            let _ = self.txn_mgr.abort(&txn);
        }

        Ok(())
    }

    fn status_char(&self) -> char {
        if self.active_txn.is_some() {
            'T'
        } else {
            'I'
        }
    }

    fn execute_query(&mut self, sql: &str) -> Result<()> {
        let trimmed = sql.trim().trim_end_matches(';').trim();
        if trimmed.is_empty() {
            return self.send_message(BackendMessage::ReadyForQuery(self.status_char()));
        }

        // Transaction control statements
        if trimmed.eq_ignore_ascii_case("BEGIN") {
            if self.active_txn.is_none() {
                let txn = self.txn_mgr.begin(IsolationLevel::ReadCommitted)?;
                self.active_txn = Some(txn);
            }
            self.send_message(BackendMessage::CommandComplete("BEGIN".to_string()))?;
            return self.send_message(BackendMessage::ReadyForQuery('T'));
        } else if trimmed.eq_ignore_ascii_case("COMMIT") {
            if let Some(txn) = self.active_txn.take() {
                self.txn_mgr.commit(&txn)?;
            }
            self.send_message(BackendMessage::CommandComplete("COMMIT".to_string()))?;
            return self.send_message(BackendMessage::ReadyForQuery('I'));
        } else if trimmed.eq_ignore_ascii_case("ROLLBACK") {
            if let Some(txn) = self.active_txn.take() {
                self.txn_mgr.abort(&txn)?;
            }
            self.send_message(BackendMessage::CommandComplete("ROLLBACK".to_string()))?;
            return self.send_message(BackendMessage::ReadyForQuery('I'));
        }

        // Determine if auto-commit applies
        let is_auto_commit = self.active_txn.is_none();
        let txn = match &self.active_txn {
            Some(t) => Arc::clone(t),
            None => self.txn_mgr.begin(IsolationLevel::ReadCommitted)?,
        };

        let ctx = ExecutionContext::new(
            self.bpm.clone(),
            self.catalog.clone(),
            self.txn_mgr.clone(),
            self.wal_mgr.clone(),
            self.lock_mgr.clone(),
            txn.txn_id(),
        );

        let execution_result = (|| -> Result<ExecutionResult> {
            let stmt = Parser::parse_sql(trimmed)?;
            let bound = SemanticAnalyzer::analyze(&stmt, &self.catalog.read().unwrap())?;
            let plan = QueryOptimizer::optimize(bound, &self.catalog.read().unwrap())?;
            ExecutionEngine::execute(plan, ctx)
        })();

        match execution_result {
            Ok(result) => {
                match result {
                    ExecutionResult::Select { schema, rows } => {
                        // Send RowDescription
                        let fields = schema
                            .columns
                            .iter()
                            .map(|c| FieldDescription::new(&c.name, c.data_type))
                            .collect();
                        self.send_message(BackendMessage::RowDescription(fields))?;

                        // Send DataRows
                        for row in &rows {
                            let cols = row
                                .iter()
                                .map(|val| match val {
                                    Value::Null => None,
                                    _ => Some(val.to_string().into_bytes()),
                                })
                                .collect();
                            self.send_message(BackendMessage::DataRow(cols))?;
                        }

                        let tag = format!("SELECT {}", rows.len());
                        self.send_message(BackendMessage::CommandComplete(tag))?;
                    }
                    ExecutionResult::Dml { rows_affected } => {
                        let upper = trimmed.to_uppercase();
                        let tag = if upper.starts_with("INSERT") {
                            format!("INSERT 0 {}", rows_affected)
                        } else if upper.starts_with("UPDATE") {
                            format!("UPDATE {}", rows_affected)
                        } else if upper.starts_with("DELETE") {
                            format!("DELETE {}", rows_affected)
                        } else {
                            format!("DML {}", rows_affected)
                        };
                        self.send_message(BackendMessage::CommandComplete(tag))?;
                    }
                    ExecutionResult::Ddl { message } => {
                        self.send_message(BackendMessage::CommandComplete(message))?;
                    }
                }

                if is_auto_commit {
                    self.txn_mgr.commit(&txn)?;
                }

                self.send_message(BackendMessage::ReadyForQuery(self.status_char()))?;
            }
            Err(e) => {
                if is_auto_commit {
                    let _ = self.txn_mgr.abort(&txn);
                }
                self.send_message(BackendMessage::ErrorResponse {
                    severity: "ERROR".to_string(),
                    code: "42000".to_string(),
                    message: e.to_string(),
                })?;
                self.send_message(BackendMessage::ReadyForQuery(self.status_char()))?;
            }
        }

        Ok(())
    }
}

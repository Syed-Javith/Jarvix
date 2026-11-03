use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;

use jarvix_buffer::BufferPoolManager;
use jarvix_catalog::Catalog;
use jarvix_common::error::Result;
use jarvix_storage::DiskManager;
use jarvix_txn::lock_manager::LockManager;
use jarvix_txn::transaction_manager::TransactionManager;
use jarvix_txn::undo::UndoLogManager;
use jarvix_wal::{RecoveryManager, WalManager};

use crate::session::ClientSession;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub data_dir: PathBuf,
    pub buffer_pool_size: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 5432,
            data_dir: PathBuf::from("./data"),
            buffer_pool_size: 1024,
        }
    }
}

pub struct JarvixServer {
    config: ServerConfig,
    bpm: BufferPoolManager,
    catalog: Arc<RwLock<Catalog>>,
    txn_mgr: Arc<TransactionManager>,
    wal_mgr: Arc<WalManager>,
    lock_mgr: Arc<LockManager>,
    is_running: Arc<AtomicBool>,
}

impl JarvixServer {
    /// Bootstraps storage, WAL, recovery, buffer pool, catalog, and transaction subsystem.
    pub fn new(config: ServerConfig) -> Result<Self> {
        std::fs::create_dir_all(&config.data_dir)?;

        let db_path = config.data_dir.join("jarvix.db");
        let wal_path = config.data_dir.join("jarvix.wal");

        let disk_mgr = Arc::new(DiskManager::open(&db_path)?);
        let bpm = BufferPoolManager::new(config.buffer_pool_size, disk_mgr);
        let catalog = Arc::new(RwLock::new(Catalog::new()));
        let wal_mgr = Arc::new(WalManager::open(&wal_path)?);
        let lock_mgr = Arc::new(LockManager::new());
        let undo_mgr = Arc::new(UndoLogManager::new());
        let txn_mgr = Arc::new(TransactionManager::new(
            wal_mgr.clone(),
            lock_mgr.clone(),
            undo_mgr,
        ));

        // ARIES Crash Recovery check & replay
        if wal_path.exists() && std::fs::metadata(&wal_path)?.len() > 0 {
            let recovery = RecoveryManager::new(wal_mgr.clone(), bpm.clone());
            recovery.recover()?;
        }

        Ok(Self {
            config,
            bpm,
            catalog,
            txn_mgr,
            wal_mgr,
            lock_mgr,
            is_running: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn bpm(&self) -> &BufferPoolManager {
        &self.bpm
    }

    pub fn catalog(&self) -> &Arc<RwLock<Catalog>> {
        &self.catalog
    }

    pub fn txn_mgr(&self) -> &Arc<TransactionManager> {
        &self.txn_mgr
    }

    pub fn wal_mgr(&self) -> &Arc<WalManager> {
        &self.wal_mgr
    }

    pub fn lock_mgr(&self) -> &Arc<LockManager> {
        &self.lock_mgr
    }

    /// Starts the server listening for incoming client connections.
    pub fn start(&self) -> Result<()> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        let listener = TcpListener::bind(&addr)?;
        self.is_running.store(true, Ordering::SeqCst);

        println!("Jarvix Server listening on {}", addr);

        while self.is_running.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((stream, _peer_addr)) => {
                    let bpm = self.bpm.clone();
                    let catalog = Arc::clone(&self.catalog);
                    let txn_mgr = Arc::clone(&self.txn_mgr);
                    let wal_mgr = Arc::clone(&self.wal_mgr);
                    let lock_mgr = Arc::clone(&self.lock_mgr);

                    thread::spawn(move || {
                        let mut session = ClientSession::new(
                            stream, bpm, catalog, txn_mgr, wal_mgr, lock_mgr,
                        );
                        let _ = session.run();
                    });
                }
                Err(e) => {
                    if !self.is_running.load(Ordering::Acquire) {
                        break;
                    }
                    eprintln!("Accept error: {}", e);
                }
            }
        }

        Ok(())
    }

    pub fn stop(&self) {
        self.is_running.store(false, Ordering::Release);
    }
}

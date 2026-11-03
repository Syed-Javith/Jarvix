use std::env;
use std::io::{self, BufRead, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;
use byteorder::{BigEndian, ReadBytesExt};

use jarvix_analyzer::SemanticAnalyzer;
use jarvix_executor::{ExecutionContext, ExecutionEngine, ExecutionResult};
use jarvix_optimizer::QueryOptimizer;
use jarvix_protocol::PG_PROTOCOL_V3;
use jarvix_server::{JarvixServer, ServerConfig};
use jarvix_sql::Parser;
use jarvix_txn::mvcc::IsolationLevel;
use jarvix_txn::transaction_manager::Transaction;

fn print_banner() {
    println!(r#"
       ██╗ █████╗ ██████╗ ██╗   ██╗██╗██╗  ██╗
       ██║██╔══██╗██╔══██╗██║   ██║██║╚██╗██╔╝
       ██║███████║██████╔╝██║   ██║██║ ╚███╔╝ 
  ██   ██║██╔══██║██╔══██╗╚██╗ ██╔╝██║ ██╔██╗ 
  ╚█████╔╝██║  ██║██║  ██║ ╚████╔╝ ██║██╔╝ ██╗
   ╚════╝ ╚═╝  ╚═╝╚═╝  ╚═╝  ╚═══╝  ╚═╝╚═╝  ╚═╝
   Jarvix Relational Database Management System
   PostgreSQL v3 Wire Compatible | Rust Native Engine
    "#);
    println!("Type \\q to quit, \\d to list tables, \\help for help.\n");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    // Check for --help
    if args.iter().any(|a| a == "--help") {
        print_banner();
        println!("Usage:");
        println!("  jarvix-cli [options]                                (Embedded Interactive REPL)");
        println!("  jarvix-cli --server [--port <port>] [--data-dir <path>]  (Start TCP Daemon)");
        println!("  jarvix-cli -h <host> -p <port>                      (Connect to Remote Server)");
        println!("  jarvix-cli -c \"<SQL>\"                                (Execute query and exit)");
        println!();
        println!("Options:");
        println!("  -h, --host <host>       Remote server host");
        println!("  -p, --port <port>       Port (default: 5432)");
        println!("  -c, --command <sql>     Execute single SQL query and exit");
        println!("      --data-dir <path>   Database storage directory (default: ./data)");
        println!("      --server            Launch standalone Jarvix TCP daemon");
        println!("      --help              Show this help message");
        return Ok(());
    }

    // 1. Dedicated Server Daemon Mode
    if args.iter().any(|a| a == "--server") {
        let mut port = 5432;
        let mut data_dir = PathBuf::from("./data");

        let mut i = 1;
        while i < args.len() {
            if args[i] == "--port" && i + 1 < args.len() {
                port = args[i + 1].parse().unwrap_or(5432);
                i += 1;
            } else if args[i] == "--data-dir" && i + 1 < args.len() {
                data_dir = PathBuf::from(&args[i + 1]);
                i += 1;
            }
            i += 1;
        }

        let config = ServerConfig {
            host: "127.0.0.1".to_string(),
            port,
            data_dir,
            buffer_pool_size: 1024,
        };

        let server = JarvixServer::new(config)?;
        println!("Jarvix Database Daemon running on port {}...", port);
        server.start()?;
        return Ok(());
    }

    // 2. Remote TCP Client Mode (if -h is explicitly passed)
    let remote_host = args
        .iter()
        .position(|a| a == "-h" || a == "--host")
        .and_then(|idx| args.get(idx + 1).cloned());

    if let Some(host) = remote_host {
        let port = args
            .iter()
            .position(|a| a == "-p" || a == "--port")
            .and_then(|idx| args.get(idx + 1))
            .and_then(|p| p.parse().ok())
            .unwrap_or(5432);

        let one_shot = args
            .iter()
            .position(|a| a == "-c" || a == "--command")
            .and_then(|idx| args.get(idx + 1).cloned());

        return run_remote_client(&host, port, one_shot);
    }

    // 3. Default Native In-Process / Embedded Mode
    let mut data_dir = PathBuf::from("./data");
    let mut one_shot_sql: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        if args[i] == "--data-dir" && i + 1 < args.len() {
            data_dir = PathBuf::from(&args[i + 1]);
            i += 1;
        } else if (args[i] == "-c" || args[i] == "--command") && i + 1 < args.len() {
            one_shot_sql = Some(args[i + 1].clone());
            i += 1;
        }
        i += 1;
    }

    run_embedded_repl(data_dir, one_shot_sql)
}

/// Runs the interactive embedded SQL REPL.
fn run_embedded_repl(
    data_dir: PathBuf,
    one_shot_sql: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 5432,
        data_dir,
        buffer_pool_size: 1024,
    };

    let server = JarvixServer::new(config)?;

    if let Some(sql) = one_shot_sql {
        let mut active_txn = None;
        execute_embedded_query(&server, &sql, &mut active_txn)?;
        return Ok(());
    }

    print_banner();

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut query_buf = String::new();
    let mut active_txn: Option<Arc<Transaction>> = None;

    loop {
        if query_buf.is_empty() {
            if active_txn.is_some() {
                print!("jarvix=*# ");
            } else {
                print!("jarvix=# ");
            }
        } else {
            print!("jarvix-# ");
        }
        io::stdout().flush()?;

        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break; // EOF
        }

        let trimmed = line.trim();
        if trimmed == "\\q" {
            println!("Goodbye.");
            if let Some(txn) = active_txn.take() {
                let _ = server.txn_mgr().abort(&txn);
            }
            break;
        } else if trimmed == "\\help" {
            println!("Jarvix SQL Syntax:");
            println!("  CREATE TABLE <name> (<col> <type>, ...);");
            println!("  CREATE INDEX <name> ON <table> (<col>);");
            println!("  INSERT INTO <table> VALUES (...);");
            println!("  SELECT <cols> FROM <table> [WHERE <cond>] [LIMIT <n>];");
            println!("  UPDATE <table> SET <col> = <val> [WHERE <cond>];");
            println!("  DELETE FROM <table> [WHERE <cond>];");
            println!("  BEGIN; / COMMIT; / ROLLBACK;");
            println!("Meta-commands:");
            println!("  \\d    List all tables in catalog");
            println!("  \\q    Exit interactive CLI");
            println!("  \\help Show this help");
            continue;
        } else if trimmed == "\\d" {
            println!("\nJarvix Schema Catalog: active session tables managed.\n");
            continue;
        }

        query_buf.push_str(&line);

        if query_buf.trim().ends_with(';') {
            let sql_to_run = query_buf.trim().to_string();
            query_buf.clear();

            if let Err(e) = execute_embedded_query(&server, &sql_to_run, &mut active_txn) {
                eprintln!("ERROR: {}\n", e);
            }
        }
    }

    Ok(())
}

fn execute_embedded_query(
    server: &JarvixServer,
    sql: &str,
    active_txn: &mut Option<Arc<Transaction>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    // Transaction control
    if trimmed.eq_ignore_ascii_case("BEGIN") {
        if active_txn.is_none() {
            let txn = server.txn_mgr().begin(IsolationLevel::ReadCommitted)?;
            *active_txn = Some(txn);
        }
        println!("BEGIN\n");
        return Ok(());
    } else if trimmed.eq_ignore_ascii_case("COMMIT") {
        if let Some(txn) = active_txn.take() {
            server.txn_mgr().commit(&txn)?;
        }
        println!("COMMIT\n");
        return Ok(());
    } else if trimmed.eq_ignore_ascii_case("ROLLBACK") {
        if let Some(txn) = active_txn.take() {
            server.txn_mgr().abort(&txn)?;
        }
        println!("ROLLBACK\n");
        return Ok(());
    }

    let is_auto_commit = active_txn.is_none();
    let txn = match active_txn {
        Some(t) => Arc::clone(t),
        None => server.txn_mgr().begin(IsolationLevel::ReadCommitted)?,
    };

    let ctx = ExecutionContext::new(
        server.bpm().clone(),
        Arc::clone(server.catalog()),
        Arc::clone(server.txn_mgr()),
        Arc::clone(server.wal_mgr()),
        Arc::clone(server.lock_mgr()),
        txn.txn_id(),
    );

    let stmt = Parser::parse_sql(trimmed)?;
    let bound = SemanticAnalyzer::analyze(&stmt, &server.catalog().read().unwrap())?;
    let plan = QueryOptimizer::optimize(bound, &server.catalog().read().unwrap())?;
    let result = ExecutionEngine::execute(plan, ctx)?;

    match result {
        ExecutionResult::Select { schema, rows } => {
            let col_names: Vec<String> = schema.columns.iter().map(|c| c.name.clone()).collect();
            let string_rows: Vec<Vec<String>> = rows
                .into_iter()
                .map(|r| r.into_iter().map(|v| v.to_string()).collect())
                .collect();
            render_table(&col_names, &string_rows);
            println!("({} rows)\n", string_rows.len());
        }
        ExecutionResult::Dml { rows_affected } => {
            let upper = trimmed.to_uppercase();
            if upper.starts_with("INSERT") {
                println!("INSERT 0 {}\n", rows_affected);
            } else if upper.starts_with("UPDATE") {
                println!("UPDATE {}\n", rows_affected);
            } else if upper.starts_with("DELETE") {
                println!("DELETE {}\n", rows_affected);
            } else {
                println!("DML {}\n", rows_affected);
            }
        }
        ExecutionResult::Ddl { message } => {
            println!("{}\n", message);
        }
    }

    if is_auto_commit {
        server.txn_mgr().commit(&txn)?;
    }

    Ok(())
}

fn run_remote_client(
    host: &str,
    port: u16,
    one_shot: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let addr = format!("{}:{}", host, port);
    let mut stream = TcpStream::connect(&addr)?;

    // Handshake
    let payload = b"user\0jarvix\0database\0jarvix\0\0";
    let total_len = 4 + 4 + payload.len() as i32;
    stream.write_all(&total_len.to_be_bytes())?;
    stream.write_all(&PG_PROTOCOL_V3.to_be_bytes())?;
    stream.write_all(payload)?;
    stream.flush()?;

    loop {
        let tag = stream.read_u8()?;
        let len = stream.read_i32::<BigEndian>()?;
        let mut body = vec![0u8; (len - 4) as usize];
        stream.read_exact(&mut body)?;
        if tag == b'Z' {
            break;
        }
    }

    if let Some(sql) = one_shot {
        execute_remote_query(&mut stream, &sql)?;
        return Ok(());
    }

    print_banner();
    println!("[Connected to remote server at {}]\n", addr);

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut query_buf = String::new();

    loop {
        if query_buf.is_empty() {
            print!("jarvix=# ");
        } else {
            print!("jarvix-# ");
        }
        io::stdout().flush()?;

        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }

        let trimmed = line.trim();
        if trimmed == "\\q" {
            let _ = stream.write_all(b"X\0\0\0\x04");
            let _ = stream.flush();
            println!("Goodbye.");
            break;
        }

        query_buf.push_str(&line);
        if query_buf.trim().ends_with(';') {
            let sql = query_buf.trim().to_string();
            query_buf.clear();
            if let Err(e) = execute_remote_query(&mut stream, &sql) {
                eprintln!("ERROR: {}\n", e);
            }
        }
    }

    Ok(())
}

fn execute_remote_query(
    stream: &mut TcpStream,
    sql: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = sql.as_bytes();
    let q_len = 4 + bytes.len() + 1;
    stream.write_all(b"Q")?;
    stream.write_all(&(q_len as i32).to_be_bytes())?;
    stream.write_all(bytes)?;
    stream.write_all(&[0])?;
    stream.flush()?;

    let mut columns = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();

    loop {
        let tag = stream.read_u8()?;
        let len = stream.read_i32::<BigEndian>()?;
        let mut body = vec![0u8; (len - 4) as usize];
        if len > 4 {
            stream.read_exact(&mut body)?;
        }

        match tag {
            b'T' => {
                let num_fields = (&body[0..2]).read_i16::<BigEndian>()?;
                let mut offset = 2;
                columns.clear();
                for _ in 0..num_fields {
                    let null_idx = body[offset..].iter().position(|&b| b == 0).unwrap();
                    let col_name = String::from_utf8_lossy(&body[offset..offset + null_idx]).to_string();
                    columns.push(col_name);
                    offset += null_idx + 1 + 18;
                }
            }
            b'D' => {
                let num_cols = (&body[0..2]).read_i16::<BigEndian>()?;
                let mut offset = 2;
                let mut row = Vec::new();
                for _ in 0..num_cols {
                    let col_len = (&body[offset..offset + 4]).read_i32::<BigEndian>()?;
                    offset += 4;
                    if col_len == -1 {
                        row.push("NULL".to_string());
                    } else {
                        let val_bytes = &body[offset..offset + col_len as usize];
                        offset += col_len as usize;
                        row.push(String::from_utf8_lossy(val_bytes).to_string());
                    }
                }
                rows.push(row);
            }
            b'C' => {
                let tag_str = String::from_utf8_lossy(&body[..body.len().saturating_sub(1)]);
                if !columns.is_empty() {
                    render_table(&columns, &rows);
                    println!("({} rows)\n", rows.len());
                } else {
                    println!("{}\n", tag_str);
                }
            }
            b'E' => {
                let mut cursor = 0;
                let mut msg = String::new();
                while cursor < body.len() && body[cursor] != 0 {
                    let field_type = body[cursor];
                    cursor += 1;
                    if let Some(pos) = body[cursor..].iter().position(|&b| b == 0) {
                        let text = String::from_utf8_lossy(&body[cursor..cursor + pos]);
                        if field_type == b'M' {
                            msg = text.to_string();
                        }
                        cursor += pos + 1;
                    } else {
                        break;
                    }
                }
                eprintln!("ERROR: {}\n", msg);
            }
            b'Z' => break,
            _ => {}
        }
    }

    Ok(())
}

fn render_table(columns: &[String], rows: &[Vec<String>]) {
    if columns.is_empty() {
        return;
    }
    let mut widths: Vec<usize> = columns.iter().map(|c| c.len()).collect();

    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }

    let border: String = widths
        .iter()
        .map(|w| format!("+-{}-", "-".repeat(*w)))
        .collect::<Vec<_>>()
        .join("") + "+";

    println!("{}", border);

    print!("|");
    for (i, col) in columns.iter().enumerate() {
        print!(" {:<width$} |", col, width = widths[i]);
    }
    println!();
    println!("{}", border);

    for row in rows {
        print!("|");
        for (i, cell) in row.iter().enumerate() {
            print!(" {:<width$} |", cell, width = widths[i]);
        }
        println!();
    }

    println!("{}", border);
}

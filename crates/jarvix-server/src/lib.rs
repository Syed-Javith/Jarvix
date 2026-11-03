//! PostgreSQL-compatible Relational Database Server for Jarvix.

pub mod server;
pub mod session;

pub use server::{JarvixServer, ServerConfig};
pub use session::ClientSession;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;
    use jarvix_protocol::PG_PROTOCOL_V3;

    fn read_until_ready(client: &mut TcpStream) -> String {
        let mut full_output = Vec::new();
        let mut buf = [0u8; 1024];
        for _ in 0..20 {
            let n = client.read(&mut buf).unwrap();
            full_output.extend_from_slice(&buf[..n]);
            if full_output.windows(6).any(|w| w[0] == b'Z' && w[1..5] == [0, 0, 0, 5]) {
                break;
            }
        }
        String::from_utf8_lossy(&full_output).to_string()
    }

    #[test]
    fn test_server_wire_protocol_e2e() {
        let test_dir = std::env::temp_dir().join(format!(
            "jarvix_srv_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        // Use a test port unlikely to conflict
        let port = 15433;
        let config = ServerConfig {
            host: "127.0.0.1".to_string(),
            port,
            data_dir: test_dir,
            buffer_pool_size: 64,
        };

        let server = Arc::new(JarvixServer::new(config).unwrap());
        let server_clone = Arc::clone(&server);

        let _handle = thread::spawn(move || {
            let _ = server_clone.start();
        });

        // Allow server thread to bind
        thread::sleep(Duration::from_millis(250));

        // Connect client
        let mut client = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();

        // 1. Send StartupMessage
        let payload = b"user\0jarvix\0database\0test\0\0";
        let total_len = 4 + 4 + payload.len() as i32;
        client.write_all(&total_len.to_be_bytes()).unwrap();
        client.write_all(&PG_PROTOCOL_V3.to_be_bytes()).unwrap();
        client.write_all(payload).unwrap();
        client.flush().unwrap();

        // 2. Read Server Startup response until ReadyForQuery ('Z')
        let startup_resp = read_until_ready(&mut client);
        assert!(startup_resp.contains("Jarvix DB Engine"));

        // 3. Send Query: CREATE TABLE
        let ddl = "CREATE TABLE users (id INT, name VARCHAR);";
        let ddl_bytes = ddl.as_bytes();
        let q_len = 4 + ddl_bytes.len() + 1;
        client.write_all(b"Q").unwrap();
        client.write_all(&(q_len as i32).to_be_bytes()).unwrap();
        client.write_all(ddl_bytes).unwrap();
        client.write_all(&[0]).unwrap();
        client.flush().unwrap();

        let ddl_resp = read_until_ready(&mut client);
        assert!(ddl_resp.contains("CREATE TABLE"));

        // 4. Send Query: INSERT
        let insert_sql = "INSERT INTO users VALUES (1, 'Alice'), (2, 'Bob');";
        let ins_bytes = insert_sql.as_bytes();
        let q_len = 4 + ins_bytes.len() + 1;
        client.write_all(b"Q").unwrap();
        client.write_all(&(q_len as i32).to_be_bytes()).unwrap();
        client.write_all(ins_bytes).unwrap();
        client.write_all(&[0]).unwrap();
        client.flush().unwrap();

        let ins_resp = read_until_ready(&mut client);
        assert!(ins_resp.contains("INSERT 0 2"));

        // 5. Send Query: SELECT
        let select_sql = "SELECT id, name FROM users WHERE id = 1;";
        let sel_bytes = select_sql.as_bytes();
        let q_len = 4 + sel_bytes.len() + 1;
        client.write_all(b"Q").unwrap();
        client.write_all(&(q_len as i32).to_be_bytes()).unwrap();
        client.write_all(sel_bytes).unwrap();
        client.write_all(&[0]).unwrap();
        client.flush().unwrap();

        let sel_resp = read_until_ready(&mut client);
        assert!(sel_resp.contains("Alice"));
        assert!(sel_resp.contains("SELECT 1"));

        // 6. Terminate connection
        client.write_all(b"X\0\0\0\x04").unwrap();
        client.flush().unwrap();

        server.stop();
    }
}

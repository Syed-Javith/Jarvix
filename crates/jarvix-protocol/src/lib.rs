//! PostgreSQL v3 Wire Protocol Framing and Codec for Jarvix.

pub mod backend;
pub mod frontend;

pub use backend::{BackendMessage, FieldDescription};
pub use frontend::{FrontendMessage, FrontendParser, PG_PROTOCOL_V3, SSL_REQUEST_CODE};

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use jarvix_common::types::DataType;

    #[test]
    fn test_pg_protocol_startup_and_query_codec() {
        // 1. Simulate Startup Message
        let mut startup_bytes = Vec::new();
        let payload = b"user\0jarvix\0database\0test\0\0";
        let total_len = 4 + 4 + payload.len() as i32;
        startup_bytes.extend_from_slice(&total_len.to_be_bytes());
        startup_bytes.extend_from_slice(&PG_PROTOCOL_V3.to_be_bytes());
        startup_bytes.extend_from_slice(payload);

        let mut cursor = Cursor::new(startup_bytes);
        let msg = FrontendParser::read_startup(&mut cursor).unwrap();
        match msg {
            FrontendMessage::Startup { parameters, .. } => {
                assert_eq!(parameters.get("user").unwrap(), "jarvix");
                assert_eq!(parameters.get("database").unwrap(), "test");
            }
            _ => panic!("Expected Startup message"),
        }

        // 2. Simulate Query Message
        let mut query_bytes = Vec::new();
        query_bytes.push(b'Q');
        let sql = "SELECT * FROM users;";
        let q_len = 4 + sql.len() + 1;
        query_bytes.extend_from_slice(&(q_len as i32).to_be_bytes());
        query_bytes.extend_from_slice(sql.as_bytes());
        query_bytes.push(0);

        let mut cursor = Cursor::new(query_bytes);
        let msg = FrontendParser::read_message(&mut cursor).unwrap();
        match msg {
            FrontendMessage::Query(s) => assert_eq!(s, "SELECT * FROM users;"),
            _ => panic!("Expected Query message"),
        }

        // 3. Test Backend Message Encoding
        let auth_ok = BackendMessage::AuthenticationOk.encode().unwrap();
        assert_eq!(auth_ok[0], b'R');

        let ready = BackendMessage::ReadyForQuery('I').encode().unwrap();
        assert_eq!(ready, vec![b'Z', 0, 0, 0, 5, b'I']);

        let row_desc = BackendMessage::RowDescription(vec![
            FieldDescription::new("id", DataType::Integer),
            FieldDescription::new("name", DataType::Varchar(50)),
        ]).encode().unwrap();
        assert_eq!(row_desc[0], b'T');

        let data_row = BackendMessage::DataRow(vec![
            Some(b"42".to_vec()),
            None, // NULL
        ]).encode().unwrap();
        assert_eq!(data_row[0], b'D');
    }
}

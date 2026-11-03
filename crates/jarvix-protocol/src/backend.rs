use std::io::Write;
use byteorder::{BigEndian, WriteBytesExt};
use jarvix_common::error::Result;
use jarvix_common::types::DataType;

#[derive(Debug, Clone, PartialEq)]
pub struct FieldDescription {
    pub name: String,
    pub type_oid: i32,
    pub type_size: i16,
}

impl FieldDescription {
    pub fn new(name: impl Into<String>, data_type: DataType) -> Self {
        let (type_oid, type_size) = match data_type {
            DataType::Boolean => (16, 1),
            DataType::SmallInt | DataType::TinyInt => (21, 2),
            DataType::Integer => (23, 4),
            DataType::BigInt | DataType::Timestamp => (20, 8),
            DataType::Float => (700, 4),
            DataType::Double => (701, 8),
            DataType::Varchar(_) => (1043, -1),
            DataType::Text | DataType::Null => (25, -1),
        };

        Self {
            name: name.into(),
            type_oid,
            type_size,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BackendMessage {
    AuthenticationOk,
    ParameterStatus {
        name: String,
        value: String,
    },
    BackendKeyData {
        process_id: u32,
        secret_key: u32,
    },
    ReadyForQuery(char),
    RowDescription(Vec<FieldDescription>),
    DataRow(Vec<Option<Vec<u8>>>),
    CommandComplete(String),
    ErrorResponse {
        severity: String,
        code: String,
        message: String,
    },
}

impl BackendMessage {
    /// Serializes backend message into Postgres v3 wire format bytes.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut buf = Vec::new();

        match self {
            BackendMessage::AuthenticationOk => {
                buf.write_u8(b'R')?;
                buf.write_i32::<BigEndian>(8)?;
                buf.write_i32::<BigEndian>(0)?; // 0 = AuthOK
            }

            BackendMessage::ParameterStatus { name, value } => {
                buf.write_u8(b'S')?;
                let len = 4 + name.len() + 1 + value.len() + 1;
                buf.write_i32::<BigEndian>(len as i32)?;
                buf.write_all(name.as_bytes())?;
                buf.write_u8(0)?;
                buf.write_all(value.as_bytes())?;
                buf.write_u8(0)?;
            }

            BackendMessage::BackendKeyData {
                process_id,
                secret_key,
            } => {
                buf.write_u8(b'K')?;
                buf.write_i32::<BigEndian>(12)?;
                buf.write_u32::<BigEndian>(*process_id)?;
                buf.write_u32::<BigEndian>(*secret_key)?;
            }

            BackendMessage::ReadyForQuery(status) => {
                buf.write_u8(b'Z')?;
                buf.write_i32::<BigEndian>(5)?;
                buf.write_u8(*status as u8)?;
            }

            BackendMessage::RowDescription(fields) => {
                buf.write_u8(b'T')?;
                let mut body = Vec::new();
                body.write_i16::<BigEndian>(fields.len() as i16)?;

                for field in fields {
                    body.write_all(field.name.as_bytes())?;
                    body.write_u8(0)?; // Null-terminated column name
                    body.write_i32::<BigEndian>(0)?; // Table OID (0 = not specified)
                    body.write_i16::<BigEndian>(0)?; // Column index
                    body.write_i32::<BigEndian>(field.type_oid)?;
                    body.write_i16::<BigEndian>(field.type_size)?;
                    body.write_i32::<BigEndian>(-1)?; // Type modifier
                    body.write_i16::<BigEndian>(0)?; // Format code (0 = text format)
                }

                buf.write_i32::<BigEndian>((4 + body.len()) as i32)?;
                buf.write_all(&body)?;
            }

            BackendMessage::DataRow(columns) => {
                buf.write_u8(b'D')?;
                let mut body = Vec::new();
                body.write_i16::<BigEndian>(columns.len() as i16)?;

                for col in columns {
                    match col {
                        Some(bytes) => {
                            body.write_i32::<BigEndian>(bytes.len() as i32)?;
                            body.write_all(bytes)?;
                        }
                        None => {
                            body.write_i32::<BigEndian>(-1)?; // -1 represents NULL in PostgreSQL
                        }
                    }
                }

                buf.write_i32::<BigEndian>((4 + body.len()) as i32)?;
                buf.write_all(&body)?;
            }

            BackendMessage::CommandComplete(tag) => {
                buf.write_u8(b'C')?;
                let len = 4 + tag.len() + 1;
                buf.write_i32::<BigEndian>(len as i32)?;
                buf.write_all(tag.as_bytes())?;
                buf.write_u8(0)?;
            }

            BackendMessage::ErrorResponse {
                severity,
                code,
                message,
            } => {
                buf.write_u8(b'E')?;
                let mut body = Vec::new();

                body.write_u8(b'S')?;
                body.write_all(severity.as_bytes())?;
                body.write_u8(0)?;

                body.write_u8(b'C')?;
                body.write_all(code.as_bytes())?;
                body.write_u8(0)?;

                body.write_u8(b'M')?;
                body.write_all(message.as_bytes())?;
                body.write_u8(0)?;

                body.write_u8(0)?; // Message terminator

                buf.write_i32::<BigEndian>((4 + body.len()) as i32)?;
                buf.write_all(&body)?;
            }
        }

        Ok(buf)
    }
}

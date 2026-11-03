use std::collections::HashMap;
use std::io::{Read, Cursor};
use byteorder::{BigEndian, ReadBytesExt};
use jarvix_common::error::{JarvixError, Result};

pub const SSL_REQUEST_CODE: u32 = 80877103;
pub const PG_PROTOCOL_V3: u32 = 196608;

#[derive(Debug, Clone, PartialEq)]
pub enum FrontendMessage {
    Startup {
        version: u32,
        parameters: HashMap<String, String>,
    },
    SslRequest,
    Query(String),
    Terminate,
    Unknown(u8),
}

pub struct FrontendParser;

impl FrontendParser {
    /// Reads and parses the initial startup message or SSL request from client stream.
    pub fn read_startup<R: Read>(reader: &mut R) -> Result<FrontendMessage> {
        let len = reader.read_i32::<BigEndian>()?;
        if len < 8 {
            return Err(JarvixError::Protocol(format!(
                "Invalid startup packet length {}",
                len
            )));
        }

        let mut body = vec![0u8; (len - 4) as usize];
        reader.read_exact(&mut body)?;

        let mut cursor = Cursor::new(&body);
        let proto_code = cursor.read_u32::<BigEndian>()?;

        if proto_code == SSL_REQUEST_CODE {
            return Ok(FrontendMessage::SslRequest);
        }

        if proto_code != PG_PROTOCOL_V3 {
            return Err(JarvixError::Protocol(format!(
                "Unsupported protocol version: {}",
                proto_code
            )));
        }

        // Parse null-terminated key-value pairs
        let mut parameters = HashMap::new();
        let payload = &body[4..];
        let mut parts = payload.split(|&b| b == 0);

        while let Some(key_bytes) = parts.next() {
            if key_bytes.is_empty() {
                break;
            }
            if let Some(val_bytes) = parts.next() {
                let key = String::from_utf8_lossy(key_bytes).to_string();
                let val = String::from_utf8_lossy(val_bytes).to_string();
                parameters.insert(key, val);
            }
        }

        Ok(FrontendMessage::Startup {
            version: proto_code,
            parameters,
        })
    }

    /// Reads regular framed messages ('Q', 'X', etc.) after startup handshake.
    pub fn read_message<R: Read>(reader: &mut R) -> Result<FrontendMessage> {
        let mut tag_buf = [0u8; 1];
        reader.read_exact(&mut tag_buf)?;
        let tag = tag_buf[0];

        let len = reader.read_i32::<BigEndian>()?;
        if len < 4 {
            return Err(JarvixError::Protocol(format!(
                "Invalid message length {} for tag '{}'",
                len, tag as char
            )));
        }

        let body_len = (len - 4) as usize;
        let mut body = vec![0u8; body_len];
        if body_len > 0 {
            reader.read_exact(&mut body)?;
        }

        match tag {
            b'Q' => {
                // Query: null-terminated string
                let sql = if let Some(&0) = body.last() {
                    String::from_utf8_lossy(&body[..body.len() - 1]).to_string()
                } else {
                    String::from_utf8_lossy(&body).to_string()
                };
                Ok(FrontendMessage::Query(sql))
            }
            b'X' => Ok(FrontendMessage::Terminate),
            _ => Ok(FrontendMessage::Unknown(tag)),
        }
    }
}

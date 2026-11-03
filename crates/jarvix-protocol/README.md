# `jarvix-protocol`

## 1. Responsibilities & Purpose

`jarvix-protocol` implements the PostgreSQL Wire Protocol Version 3.0 codec.

It exists to:
- Decode incoming frontend client messages (`FrontendMessage`: `Startup`, `SslRequest`, `Query`, `Terminate`).
- Encode outgoing backend response packets (`BackendMessage`: `AuthenticationOk`, `ParameterStatus`, `BackendKeyData`, `RowDescription`, `DataRow`, `CommandComplete`, `ReadyForQuery`, `ErrorResponse`).
- Enable native connectivity with any standard PostgreSQL client (`psql`, JDBC, Python `psycopg2`, Node `pg`, etc.) without requiring custom client libraries.

---

## 2. File Organization

| File | Purpose |
|---|---|
| `src/frontend.rs` | `FrontendMessage` and `FrontendParser`: Parses startup packets, SSL negotiation, and simple query commands from client byte streams. |
| `src/backend.rs` | `BackendMessage`: Binary encoder for all PostgreSQL server response packets. |
| `src/lib.rs` | Crate root exposing frontend and backend protocol types. |

---

## 3. Subsystem Interactions & Dependencies

```mermaid
flowchart TD
    CLIENT["PostgreSQL Client (psql, DBeaver)"]
    SRV["jarvix-server"]
    PROTO["jarvix-protocol\n(FrontendParser, BackendMessage)"]
    COMMON["jarvix-common"]

    CLIENT <-->|TCP (Port 5433)| SRV
    SRV <--> PROTO
    PROTO --> COMMON
```

- **Depends On**: `jarvix-common`, `byteorder`.
- **Depended On By**: `jarvix-server`.

---

## 4. Rust Learning for Database Engineers

### 4.1 Safe Binary Serialization with `byteorder`
```rust
pub fn encode(&self) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    match self {
        BackendMessage::ReadyForQuery(status) => {
            buf.push(b'Z');
            buf.write_i32::<BigEndian>(5)?; // 4 bytes length + 1 byte payload
            buf.push(*status as u8);
        }
        ...
    }
    Ok(buf)
}
```
- Network wire protocols require exact byte-level formatting.
- `byteorder::WriteBytesExt` serializes integers to big-endian network byte order reliably across any operating system or CPU architecture.

### 4.2 Stream Processing with `Read` and `Write` Traits
By implementing functions using generic `R: std::io::Read` and `W: std::io::Write`, the protocol encoder and decoder can operate over raw `TcpStream`s in production or in-memory `Cursor<Vec<u8>>` buffers during automated unit tests!

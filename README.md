# soupbintcp-rs

A Rust implementation of the Nasdaq **SoupBinTCP** protocol (3.0 / 4.x — the wire format is identical), split into a transport-agnostic codec crate and blocking client/server crates.

SoupBinTCP is the framing/session protocol exchanges such as Nasdaq use to wrap order-entry protocols (OUCH, RASH, FIX) over TCP. These crates run the protocol mechanics — framing, login handshake, heartbeats, liveness detection, graceful termination — and surface everything else to your application.

## Workspace layout

| Crate | Purpose |
|---|---|
| `soupbintcp-core` | Wire format only: packet framing, encode/decode, error types. No I/O. |
| `soupbintcp-client` | Blocking client: connect, login, sequenced-data event stream, heartbeats, logout. |
| `soupbintcp-server` | Blocking server: accept loop, per-connection `Connection`, listener callbacks, heartbeats. |

## Protocol summary

Every packet in both directions has the same structure:

```
+----------------+-------------------+----------------------+
| Length         | Type              | Payload              |
| 2 bytes, BE    | 1 byte ASCII      | (Length - 1) bytes   |
+----------------+-------------------+----------------------+
```

`Length` includes the Type byte but not the 2 length bytes. All multi-byte fields are big-endian. Sequenced Data packets carry **no explicit sequence number on the wire** — both sides count locally, starting at 1 per session.

| Direction | Type | Name |
|---|---|---|
| Client → Server | `L` | Login Request |
| Client → Server | `U` | Unsequenced Data |
| Client → Server | `R` | Client Heartbeat |
| Client → Server | `O` | Logout Request |
| Both | `+` | Debug |
| Server → Client | `A` | Login Accepted |
| Server → Client | `J` | Login Rejected |
| Server → Client | `S` | Sequenced Data |
| Server → Client | `H` | Server Heartbeat |
| Server → Client | `Z` | End of Session |

## Usage

### Server

Accepts every login and echoes each Unsequenced Data packet back as Sequenced Data (`soupbintcp-server/examples/echo_server.rs`):

```rust
use std::sync::Arc;
use soupbintcp_server::{
    Config, ConnectionHandle, LoginAccepted, LoginRequest, LoginResponse, ServerListener,
    SoupBinTcpServer,
};

struct EchoListener {
    handle: ConnectionHandle,
}

impl ServerListener for EchoListener {
    fn login_request(&self, request: &LoginRequest) -> LoginResponse {
        LoginResponse::Accepted(LoginAccepted::new("ECHO", "1"))
    }

    fn unsequenced_data(&self, data: &[u8]) {
        let _ = self.handle.send_sequenced(data);
    }

    fn logout_request(&self) {}
    fn heartbeat_timeout(&self) {}
    fn debug(&self, text: &str) {}
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = SoupBinTcpServer::bind("127.0.0.1:21300", Config::default())?;
    server.run(|_peer_addr, handle| Arc::new(EchoListener { handle }))
}
```

### Client

Connect, log in, print events, log out (`soupbintcp-client/examples/basic.rs`):

```rust
use soupbintcp_client::{LoginRequest, SoupBinTCPClient};

let client = SoupBinTCPClient::connect("127.0.0.1:21300")?;
let (session, rx) = client.login(LoginRequest::new("USER", "P@SSWORD1", "", "1"))?;

for event in rx {
    // Event::SequencedData, Event::Disconnected, ...
}

session.logout()?;
```

Run the pair locally:

```sh
cargo run --example echo_server -p soupbintcp-server &
cargo run --example basic -p soupbintcp-client
```

## Design notes

- **Blocking, thread-per-connection I/O** — deliberate at SoupBinTCP's connection counts (dozens, not thousands). The module boundaries keep `soupbintcp-core` reusable by a future async (tokio) variant.
- **Client**: reader thread feeds a crossbeam channel of `Event`s; heartbeats and a liveness watchdog are library responsibilities. Reconnect/backoff, durable sequence checkpointing, and gap-fill deduplication are intentionally left to the application — the library exposes the hooks (session ID + sequence number from Login Accepted, per-packet counting, `Event::Disconnected`).
- **Server**: each connection has a reader thread (frame decode → listener callbacks, liveness watchdog via read timeout) and a writer thread that owns the write half and drains a bounded outbound channel — giving ordered, non-interleaved frames, free heartbeat timing via `recv_timeout`, and explicit backpressure. Authentication, session ID generation, sequencing, and replay/gap-fill are application concerns delivered through the `ServerListener` trait.
- **Defense**: malformed frames, implausible lengths, and pre-login non-login packets close the connection rather than panic.

## References

- [Nasdaq SoupBinTCP 3.0 specification](https://www.nasdaq.com/docs/SoupBinTCP_3.0)
- [NASDAQtrader SoupBinTCP spec (PDF)](https://www.nasdaqtrader.com/content/technicalsupport/specifications/dataproducts/soupbintcp.pdf)
- [paritytrading/nassau](https://github.com/paritytrading/nassau) — Java SoupBinTCP client & server; the design target for these crates
- [Wireshark SoupBinTCP wiki](https://wiki.wireshark.org/SoupBinTCP)

Always defer to the specific venue's published connectivity specification where it differs.

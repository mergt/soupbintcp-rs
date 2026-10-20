use std::sync::Arc;

use soupbintcp_server::{
    Config, ConnectionHandle, LoginAccepted, LoginRequest, LoginResponse, ServerListener,
    SoupBinTcpServer,
};

/// Accepts every login and echoes each Unsequenced Data packet back to the
/// client as Sequenced Data. Shows the thinnest possible application layer:
/// auth, session IDs, and sequencing are placeholders here.
struct EchoListener {
    handle: ConnectionHandle,
}

impl ServerListener for EchoListener {
    fn login_request(&self, request: &LoginRequest) -> LoginResponse {
        println!("login from user {}", request.username);
        LoginResponse::Accepted(LoginAccepted::new("ECHO", "1"))
    }

    fn unsequenced_data(&self, data: &[u8]) {
        let _ = self.handle.send_sequenced(data);
    }

    fn logout_request(&self) {
        println!("logout");
    }

    fn heartbeat_timeout(&self) {
        println!("heartbeat timeout, closing connection");
    }

    fn debug(&self, text: &str) {
        println!("debug from client: {text}");
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = SoupBinTcpServer::bind("127.0.0.1:21300", Config::default())?;
    println!("listening on {}", server.local_addr()?);

    server.run(|peer_addr, handle| {
        println!("new connection: {peer_addr}");
        Arc::new(EchoListener { handle })
    })?;

    Ok(())
}

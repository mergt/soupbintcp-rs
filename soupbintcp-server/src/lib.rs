pub mod server;
pub use server::{
    Config, Connection, ConnectionHandle, LoginResponse, ServerListener, SoupBinTcpServer,
};
pub use soupbintcp_core::{LoginAccepted, LoginRejected, LoginRequest};

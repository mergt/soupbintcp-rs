use std::{
    io::Write,
    net::TcpStream,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use soupbintcp_core::{ClientHeartbeat, LogoutRequest, Result, UnsequencedData};

pub struct Session {
    stream: TcpStream,
    pub session_id: String,
    pub sequence_number: u64,
    logged_out: Arc<AtomicBool>,
}

impl Session {
    pub(crate) fn new(
        stream: TcpStream,
        session_id: String,
        sequence_number: u64,
        logged_out: Arc<AtomicBool>,
    ) -> Self {
        Self {
            stream,
            session_id,
            sequence_number,
            logged_out,
        }
    }

    pub fn logout(mut self) -> Result<()> {
        self.logged_out.store(true, Ordering::Release);
        self.stream.write_all(&LogoutRequest::to_wire())?;
        Ok(())
    }
    pub fn heartbeat(&mut self) -> Result<()> {
        self.stream.write_all(&ClientHeartbeat::to_wire())?;
        Ok(())
    }

    pub fn send_unsequenced(&mut self, payload: &[u8]) -> Result<()> {
        self.stream.write_all(&UnsequencedData::to_wire(payload)?)?;
        Ok(())
    }
}

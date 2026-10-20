use std::{
    io::{ErrorKind, Read, Write},
    net::TcpStream,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use crossbeam::channel::{Receiver, unbounded};

use crate::{Error, Event, LoginRequest, Result, Session, event::into_event};

fn read_packet(stream: &mut TcpStream) -> Result<Event> {
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf)?;
    let len = u16::from_be_bytes(len_buf) as usize;
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body)?;
    if body.is_empty() {
        return Err(Error::MalformedPacket("empty packet".into()));
    }
    into_event(body[0], &body[1..])
}

pub struct SoupBinTCPClient {
    stream: TcpStream,
}

impl SoupBinTCPClient {
    pub fn connect(addr: &str) -> Result<Self> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        Ok(Self { stream })
    }

    pub fn login(mut self, req: LoginRequest) -> Result<(Session, Receiver<Event>)> {
        self.stream.write_all(&req.to_wire())?;

        self.stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        let response = read_packet(&mut self.stream);
        self.stream.set_read_timeout(None)?;
        let event = response?;

        let (session_id, sequence_number) = match event {
            Event::LoginAcceptedEvent(resp) => {
                let sequence = resp
                    .sequence_number
                    .parse::<u64>()
                    .map_err(|e| Error::MalformedPacket(e.to_string()))?;
                (resp.session, sequence)
            }
            Event::LoginRejectedEvent(resp) => return Err(Error::LoginRejected(resp.reason)),
            e => {
                return Err(Error::MalformedPacket(format!(
                    "expected login response, got {e:?}"
                )));
            }
        };

        let (tx, rx) = unbounded();
        let logged_out = Arc::new(AtomicBool::new(false));
        let reader_logged_out = Arc::clone(&logged_out);
        let mut reader = self.stream.try_clone()?;
        thread::spawn(move || {
            loop {
                match read_packet(&mut reader) {
                    Ok(event) => {
                        if tx.send(event).is_err() {
                            break;
                        }
                    }
                    Err(Error::Io(e)) => {
                        if reader_logged_out.load(Ordering::Acquire) {
                            let _ = tx.send(Event::LoggedOut);
                            break;
                        }
                        let reason = if e.kind() == ErrorKind::UnexpectedEof {
                            "server closed connection".to_string()
                        } else {
                            e.to_string()
                        };

                        let _ = tx.send(Event::Disconnected(reason));
                        break;
                    }
                    Err(e) => {
                        let _ = tx.send(Event::Disconnected(e.to_string()));
                        break;
                    }
                }
            }
        });
        Ok((
            Session::new(self.stream, session_id, sequence_number, logged_out),
            rx,
        ))
    }
}

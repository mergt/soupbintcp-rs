use std::{
    io::{ErrorKind, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use crossbeam::channel::{Receiver, RecvTimeoutError, Sender, TrySendError, bounded};

use soupbintcp_core::{
    Debug, EndOfSession, Error, LoginAccepted, LoginRejected, LoginRequest, Packet, Result,
    SequencedData, ServerHeartbeat,
};

pub fn read_packet(stream: &mut TcpStream) -> Result<Packet> {
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf)?;
    let len = u16::from_be_bytes(len_buf) as usize;
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body)?;
    if body.is_empty() {
        return Err(Error::MalformedPacket("empty packet".into()));
    }
    Packet::decode(body[0], &body[1..])
}

pub enum LoginResponse {
    Accepted(LoginAccepted),
    Rejected(LoginRejected),
}

pub trait ServerListener: Send + Sync {
    fn login_request(&self, request: &LoginRequest) -> LoginResponse;
    fn login_accepted(&self, _accepted: &LoginAccepted) {}
    fn unsequenced_data(&self, data: &[u8]);
    fn logout_request(&self);
    fn heartbeat_timeout(&self);
    fn debug(&self, text: &str);
}

#[derive(Clone)]
pub struct Config {
    pub login_timeout: Duration,
    pub heartbeat_interval: Duration,
    pub liveness_timeout: Duration,
    pub outbound_queue_bound: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            login_timeout: Duration::from_secs(10),
            heartbeat_interval: Duration::from_secs(1),
            liveness_timeout: Duration::from_secs(15),
            outbound_queue_bound: 1024,
        }
    }
}

enum Outbound {
    Frame(Vec<u8>),
    Close,
}

/// Thread-safe sender for outbound packets on a connection.
/// Usable only after the login has been accepted.
#[derive(Clone)]
pub struct ConnectionHandle {
    tx: Sender<Outbound>,
    active: Arc<AtomicBool>,
}

impl ConnectionHandle {
    fn send_frame(&self, frame: Result<Vec<u8>>) -> Result<()> {
        if !self.active.load(Ordering::Acquire) {
            return Err(Error::MalformedPacket("connection is not logged in".into()));
        }
        self.tx
            .try_send(Outbound::Frame(frame?))
            .map_err(|e| match e {
                TrySendError::Full(_) => Error::Io(std::io::Error::new(
                    ErrorKind::WriteZero,
                    "outbound queue full",
                )),
                TrySendError::Disconnected(_) => Error::Io(std::io::Error::new(
                    ErrorKind::BrokenPipe,
                    "connection closed",
                )),
            })
    }

    pub fn send_sequenced(&self, payload: &[u8]) -> Result<()> {
        self.send_frame(SequencedData::to_wire(payload))
    }

    pub fn debug(&self, text: &str) -> Result<()> {
        self.send_frame(Debug::to_wire(text))
    }

    pub fn end_session(&self) -> Result<()> {
        self.send_frame(Ok(EndOfSession::to_wire().to_vec()))?;
        let _ = self.tx.try_send(Outbound::Close);
        Ok(())
    }
}

/// One accepted SoupBinTCP connection. `Connection` drives the protocol on a
/// single socket; use `SoupBinTcpServer` for a library-owned accept loop.
pub struct Connection {
    stream: TcpStream,
    listener: Arc<dyn ServerListener>,
    config: Config,
    tx: Sender<Outbound>,
    rx: Option<Receiver<Outbound>>,
    active: Arc<AtomicBool>,
}

impl Connection {
    pub fn new(
        stream: TcpStream,
        listener: Arc<dyn ServerListener>,
        config: Config,
    ) -> Result<Self> {
        Self::with_listener_factory(stream, config, move |_| listener)
    }

    fn with_listener_factory<F>(
        stream: TcpStream,
        config: Config,
        listener_factory: F,
    ) -> Result<Self>
    where
        F: FnOnce(ConnectionHandle) -> Arc<dyn ServerListener>,
    {
        stream.set_nodelay(true)?;
        let (tx, rx) = bounded(config.outbound_queue_bound);
        let active = Arc::new(AtomicBool::new(false));
        let handle = ConnectionHandle {
            tx: tx.clone(),
            active: Arc::clone(&active),
        };
        let listener = listener_factory(handle);
        Ok(Self {
            stream,
            listener,
            config,
            tx,
            rx: Some(rx),
            active,
        })
    }

    pub fn handle(&self) -> ConnectionHandle {
        ConnectionHandle {
            tx: self.tx.clone(),
            active: Arc::clone(&self.active),
        }
    }

    /// Runs the connection to completion: login handshake, then the
    /// steady-state read loop. Blocks until the connection closes.
    pub fn run(mut self) -> Result<()> {
        self.stream
            .set_read_timeout(Some(self.config.login_timeout))?;

        // Login phase: only Login Request (and Debug) packets are valid here.
        let login_request = loop {
            match read_packet(&mut self.stream)? {
                Packet::LoginRequest(req) => break req,
                Packet::Debug(text) => self.listener.debug(&text),
                other => {
                    return Err(Error::UnknownPacket(format!(
                        "expected login request, got {other:?}"
                    )));
                }
            }
        };

        let accepted = match self.listener.login_request(&login_request) {
            LoginResponse::Rejected(rejected) => {
                self.stream.write_all(&rejected.to_wire()?)?;
                return Ok(()); // spec: close after reject
            }
            LoginResponse::Accepted(accepted) => accepted,
        };

        // Login Accepted must be the first non-debug packet sent by the server.
        // Activate the handle before writing it: any frames the application
        // queues now sit in the channel and are drained after this write.
        self.active.store(true, Ordering::Release);
        self.stream.write_all(&accepted.to_wire())?;

        // From here the writer thread is the only writer on the socket.
        let rx = self.rx.take().expect("run() called twice");
        let mut writer_stream = self.stream.try_clone()?;
        let heartbeat = self.config.heartbeat_interval;
        thread::spawn(move || {
            loop {
                match rx.recv_timeout(heartbeat) {
                    Ok(Outbound::Frame(frame)) => {
                        if writer_stream.write_all(&frame).is_err() {
                            break;
                        }
                    }
                    Ok(Outbound::Close) => break,
                    // An empty channel for one heartbeat interval == outbound idle.
                    Err(RecvTimeoutError::Timeout) => {
                        if writer_stream
                            .write_all(&ServerHeartbeat::to_wire())
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        });

        self.listener.login_accepted(&accepted);

        self.stream
            .set_read_timeout(Some(self.config.liveness_timeout))?;
        let result = self.read_loop();

        self.active.store(false, Ordering::Release);
        let _ = self.tx.try_send(Outbound::Close);
        result
    }

    fn read_loop(&mut self) -> Result<()> {
        loop {
            match read_packet(&mut self.stream) {
                Ok(Packet::UnsequencedData(data)) => self.listener.unsequenced_data(&data),
                Ok(Packet::ClientHeartbeat) => {} // any inbound packet resets the watchdog
                Ok(Packet::Debug(text)) => self.listener.debug(&text),
                Ok(Packet::LogoutRequest) => {
                    self.listener.logout_request();
                    return Ok(());
                }
                Ok(other) => {
                    return Err(Error::UnknownPacket(format!(
                        "unexpected packet from client: {other:?}"
                    )));
                }
                Err(Error::Io(e))
                    if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut =>
                {
                    self.listener.heartbeat_timeout();
                    return Ok(());
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// A blocking SoupBinTCP server that owns the accept loop and spawns one
/// thread per connection.
pub struct SoupBinTcpServer {
    listener: TcpListener,
    config: Config,
}

impl SoupBinTcpServer {
    pub fn bind<A: ToSocketAddrs>(address: A, config: Config) -> Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(address)?,
            config,
        })
    }

    pub fn from_listener(listener: TcpListener, config: Config) -> Self {
        Self { listener, config }
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.listener.local_addr()?)
    }

    /// Runs the accept loop until it fails. The factory is called once per
    /// connection with its peer address and outbound handle. Asynchronous
    /// connection errors are reported on stderr.
    pub fn run<F>(self, listener_factory: F) -> Result<()>
    where
        F: Fn(SocketAddr, ConnectionHandle) -> Arc<dyn ServerListener> + Send + Sync + 'static,
    {
        let Self { listener, config } = self;
        let listener_factory = Arc::new(listener_factory);

        for stream in listener.incoming() {
            let stream = stream?;
            let peer_addr = stream.peer_addr()?;
            let config = config.clone();
            let listener_factory = Arc::clone(&listener_factory);

            thread::spawn(move || {
                let result = Connection::with_listener_factory(stream, config, |handle| {
                    listener_factory(peer_addr, handle)
                })
                .and_then(Connection::run);
                if let Err(error) = result {
                    eprintln!("SoupBinTCP connection {peer_addr} failed: {error}");
                }
            });
        }

        Ok(())
    }
}

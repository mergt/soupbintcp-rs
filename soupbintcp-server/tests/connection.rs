use std::{
    net::TcpListener,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use crossbeam::channel::{Receiver, Sender, unbounded};
use soupbintcp_client::{Error, Event, LoginRequest as ClientLoginRequest, SoupBinTCPClient};
use soupbintcp_server::{
    Config, Connection, ConnectionHandle, LoginAccepted, LoginRejected, LoginRequest,
    LoginResponse, ServerListener, SoupBinTcpServer,
};

const TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug)]
enum Ev {
    Login(String),
    Data(Vec<u8>),
    Logout,
    HeartbeatTimeout,
    Debug(String),
}

struct TestListener {
    reject: bool,
    echo: bool,
    accepted_payload: Option<Vec<u8>>,
    events: Sender<Ev>,
    handle: Mutex<Option<ConnectionHandle>>,
}

impl TestListener {
    fn new(reject: bool, echo: bool) -> (Arc<Self>, Receiver<Ev>) {
        Self::with_options(reject, echo, None)
    }

    fn sending_on_accept(payload: &[u8]) -> (Arc<Self>, Receiver<Ev>) {
        Self::with_options(false, false, Some(payload.to_vec()))
    }

    fn with_options(
        reject: bool,
        echo: bool,
        accepted_payload: Option<Vec<u8>>,
    ) -> (Arc<Self>, Receiver<Ev>) {
        let (tx, rx) = unbounded();
        (
            Arc::new(Self {
                reject,
                echo,
                accepted_payload,
                events: tx,
                handle: Mutex::new(None),
            }),
            rx,
        )
    }

    fn handle(&self) -> ConnectionHandle {
        self.handle.lock().unwrap().clone().expect("no handle yet")
    }
}

impl ServerListener for TestListener {
    fn login_request(&self, request: &LoginRequest) -> LoginResponse {
        self.events
            .send(Ev::Login(request.username.clone()))
            .unwrap();
        if self.reject {
            LoginResponse::Rejected(LoginRejected::new('A'))
        } else {
            LoginResponse::Accepted(LoginAccepted::new("SESSION1", "1"))
        }
    }

    fn login_accepted(&self, _accepted: &LoginAccepted) {
        if let Some(payload) = &self.accepted_payload {
            self.handle().send_sequenced(payload).unwrap();
        }
    }

    fn unsequenced_data(&self, data: &[u8]) {
        self.events.send(Ev::Data(data.to_vec())).unwrap();
        if self.echo {
            let _ = self.handle().send_sequenced(data);
        }
    }

    fn logout_request(&self) {
        self.events.send(Ev::Logout).unwrap();
    }

    fn heartbeat_timeout(&self) {
        self.events.send(Ev::HeartbeatTimeout).unwrap();
    }

    fn debug(&self, text: &str) {
        self.events.send(Ev::Debug(text.to_string())).unwrap();
    }
}

fn start(listener: Arc<TestListener>, config: Config) -> String {
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = tcp.local_addr().unwrap().to_string();
    thread::spawn(move || {
        let (stream, _) = tcp.accept().unwrap();
        let conn = Connection::new(stream, listener.clone(), config).unwrap();
        *listener.handle.lock().unwrap() = Some(conn.handle());
        let _ = conn.run();
    });
    addr
}

fn start_server(server: SoupBinTcpServer, listener: Arc<TestListener>) -> String {
    let addr = server.local_addr().unwrap().to_string();
    thread::spawn(move || {
        server
            .run(move |_, handle| {
                *listener.handle.lock().unwrap() = Some(handle);
                listener.clone()
            })
            .unwrap();
    });
    addr
}

fn start_wrapped(listener: Arc<TestListener>, config: Config) -> String {
    let server = SoupBinTcpServer::bind("127.0.0.1:0", config).unwrap();
    start_server(server, listener)
}

fn fast_config() -> Config {
    Config {
        login_timeout: Duration::from_secs(2),
        heartbeat_interval: Duration::from_millis(50),
        liveness_timeout: Duration::from_secs(60),
        outbound_queue_bound: 16,
    }
}

fn login_req() -> ClientLoginRequest {
    ClientLoginRequest::new("USER", "P@SSWORD", "", "1")
}

fn expect_login(server_rx: &Receiver<Ev>) {
    assert!(matches!(
        server_rx.recv_timeout(TIMEOUT).unwrap(),
        Ev::Login(user) if user == "USER"
    ));
}

#[test]
fn login_accepted_hands_session_to_client() {
    let (listener, server_rx) = TestListener::new(false, false);
    let addr = start(listener, fast_config());

    let client = SoupBinTCPClient::connect(&addr).unwrap();
    let (session, _rx) = client.login(login_req()).unwrap();

    assert_eq!(session.session_id, "SESSION1");
    assert_eq!(session.sequence_number, 1);
    expect_login(&server_rx);
}

#[test]
fn login_accepted_callback_sends_after_handshake() {
    let (listener, _server_rx) = TestListener::sending_on_accept(b"ready");
    let addr = start(listener, fast_config());

    let client = SoupBinTCPClient::connect(&addr).unwrap();
    let (_session, rx) = client.login(login_req()).unwrap();

    assert!(matches!(
        rx.recv_timeout(TIMEOUT).unwrap(),
        Event::SequencedDataEvent(d) if d == b"ready"
    ));
}

#[test]
fn login_rejected_returns_error_and_closes() {
    let (listener, _server_rx) = TestListener::new(true, false);
    let addr = start(listener, fast_config());

    let client = SoupBinTCPClient::connect(&addr).unwrap();
    let result = client.login(login_req());

    assert!(matches!(result, Err(Error::LoginRejected(_))));
}

#[test]
fn server_heartbeats_when_idle() {
    let (listener, _server_rx) = TestListener::new(false, false);
    let addr = start(listener, fast_config());

    let client = SoupBinTCPClient::connect(&addr).unwrap();
    let (_session, rx) = client.login(login_req()).unwrap();

    assert!(matches!(
        rx.recv_timeout(TIMEOUT).unwrap(),
        Event::HeartbeatEvent
    ));
}

#[test]
fn unsequenced_data_roundtrips_as_sequenced_echo() {
    let (listener, server_rx) = TestListener::new(false, true);
    let addr = start(listener, fast_config());

    let client = SoupBinTCPClient::connect(&addr).unwrap();
    let (mut session, rx) = client.login(login_req()).unwrap();
    expect_login(&server_rx);
    session.send_unsequenced(b"ping").unwrap();

    assert!(matches!(
        server_rx.recv_timeout(TIMEOUT).unwrap(),
        Ev::Data(d) if d == b"ping"
    ));
    assert!(matches!(
        rx.recv_timeout(TIMEOUT).unwrap(),
        Event::SequencedDataEvent(d) if d == b"ping"
    ));
}

#[test]
fn server_wrapper_runs_accept_loop_and_echoes() {
    let (listener, server_rx) = TestListener::new(false, true);
    let mut config = fast_config();
    config.heartbeat_interval = Duration::from_secs(60);
    let addr = start_wrapped(listener, config);

    let client = SoupBinTCPClient::connect(&addr).unwrap();
    let (mut session, rx) = client.login(login_req()).unwrap();
    expect_login(&server_rx);
    session.send_unsequenced(b"wrapped").unwrap();

    assert!(matches!(
        server_rx.recv_timeout(TIMEOUT).unwrap(),
        Ev::Data(d) if d == b"wrapped"
    ));
    assert!(matches!(
        rx.recv_timeout(TIMEOUT).unwrap(),
        Event::SequencedDataEvent(d) if d == b"wrapped"
    ));
}

#[test]
fn server_wrapper_from_existing_listener_accepts_login() {
    let (listener, server_rx) = TestListener::new(false, false);
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    let server = SoupBinTcpServer::from_listener(tcp, fast_config());
    let addr = start_server(server, listener);

    let client = SoupBinTCPClient::connect(&addr).unwrap();
    let (session, _rx) = client.login(login_req()).unwrap();

    assert_eq!(session.session_id, "SESSION1");
    expect_login(&server_rx);
}

#[test]
fn logout_closes_connection_gracefully() {
    let (listener, server_rx) = TestListener::new(false, false);
    let addr = start(listener, fast_config());

    let client = SoupBinTCPClient::connect(&addr).unwrap();
    let (session, rx) = client.login(login_req()).unwrap();
    expect_login(&server_rx);
    session.logout().unwrap();

    assert!(matches!(
        server_rx.recv_timeout(TIMEOUT).unwrap(),
        Ev::Logout
    ));
    assert!(matches!(
        rx.recv_timeout(TIMEOUT).unwrap(),
        Event::LoggedOut
    ));
}

#[test]
fn silent_client_is_kicked_after_liveness_timeout() {
    let (listener, server_rx) = TestListener::new(false, false);
    let mut config = fast_config();
    config.heartbeat_interval = Duration::from_secs(60); // no 'H' noise
    config.liveness_timeout = Duration::from_millis(200);
    let addr = start(listener, config);

    let client = SoupBinTCPClient::connect(&addr).unwrap();
    let (_session, rx) = client.login(login_req()).unwrap();
    expect_login(&server_rx);

    assert!(matches!(
        server_rx.recv_timeout(TIMEOUT).unwrap(),
        Ev::HeartbeatTimeout
    ));
    assert!(matches!(
        rx.recv_timeout(TIMEOUT).unwrap(),
        Event::Disconnected(_)
    ));
}

#[test]
fn end_session_sends_end_of_session() {
    let (listener, _server_rx) = TestListener::new(false, false);
    let addr = start(listener.clone(), fast_config());

    let client = SoupBinTCPClient::connect(&addr).unwrap();
    let (_session, rx) = client.login(login_req()).unwrap();

    listener.handle().end_session().unwrap();

    assert!(matches!(
        rx.recv_timeout(TIMEOUT).unwrap(),
        Event::EndOfSessionEvent
    ));
}

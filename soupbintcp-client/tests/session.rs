use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread,
    time::Duration,
};

use soupbintcp_client::{Error, Event, LoginRequest, SoupBinTCPClient};

fn login_req() -> LoginRequest {
    LoginRequest::new("USER", "P@SSWORD", "", "1")
}

fn packet(payload: &[u8]) -> Vec<u8> {
    let mut pkt = (payload.len() as u16).to_be_bytes().to_vec();
    pkt.extend_from_slice(payload);
    pkt
}

fn read_login_request(conn: &mut TcpStream) {
    let mut hdr = [0u8; 2];
    conn.read_exact(&mut hdr).unwrap();
    let len = u16::from_be_bytes(hdr) as usize;
    let mut body = vec![0u8; len];
    conn.read_exact(&mut body).unwrap();
    assert_eq!(body[0], b'L');
}

#[test]
fn login_accepted_returns_session_and_delivers_events() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server = thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        read_login_request(&mut conn);

        // login accepted: session "SESSION1", sequence 1
        conn.write_all(&packet(b"ASESSION1  1                   "))
            .unwrap();
        // one sequenced data packet carrying "hello"
        conn.write_all(&packet(b"Shello")).unwrap();
    });

    let client = SoupBinTCPClient::connect(&addr.to_string()).unwrap();
    let (session, rx) = client.login(login_req()).unwrap();
    assert_eq!(session.session_id, "SESSION1");
    assert_eq!(session.sequence_number, 1);

    let event = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(event, Event::SequencedDataEvent(d) if d == b"hello"));

    let event = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(event, Event::Disconnected(_)));

    // channel is closed after disconnect, no busy loop
    assert!(rx.recv_timeout(Duration::from_secs(2)).is_err());

    server.join().unwrap();
}

#[test]
fn logout_reports_clean_close() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server = thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        read_login_request(&mut conn);
        conn.write_all(&packet(b"ASESSION1  1                   "))
            .unwrap();

        // expect a logout request, then close per spec (no reply to 'O')
        let mut hdr = [0u8; 2];
        conn.read_exact(&mut hdr).unwrap();
        let len = u16::from_be_bytes(hdr) as usize;
        let mut body = vec![0u8; len];
        conn.read_exact(&mut body).unwrap();
        assert_eq!(body[0], b'O');
    });

    let client = SoupBinTCPClient::connect(&addr.to_string()).unwrap();
    let (session, rx) = client.login(login_req()).unwrap();
    session.logout().unwrap();

    let event = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(event, Event::LoggedOut));

    server.join().unwrap();
}

#[test]
fn login_rejected_returns_error() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server = thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        read_login_request(&mut conn);
        conn.write_all(&packet(b"JA")).unwrap(); // rejected: not authorized
    });

    let client = SoupBinTCPClient::connect(&addr.to_string()).unwrap();
    let result = client.login(login_req());
    assert!(matches!(result, Err(Error::LoginRejected(_))));

    server.join().unwrap();
}

#[test]
fn login_times_out_when_server_is_silent() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server = thread::spawn(move || {
        let (_conn, _) = listener.accept().unwrap();
        // hold the connection open but never send a login response
        thread::sleep(Duration::from_secs(6));
    });

    let client = SoupBinTCPClient::connect(&addr.to_string()).unwrap();
    let result = client.login(login_req());
    assert!(result.is_err());

    server.join().unwrap();
}

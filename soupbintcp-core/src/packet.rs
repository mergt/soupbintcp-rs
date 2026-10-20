use crate::error::{Error, Result};

fn pad_ascii(s: &str, len: usize) -> Vec<u8> {
    let mut bytes = s.as_bytes().to_vec();
    bytes.resize(len, b' ');
    bytes.truncate(len);
    bytes
}

fn trim_ascii(buf: &[u8]) -> String {
    String::from_utf8_lossy(buf).trim().to_string()
}

fn framed(packet_type: u8, payload: &[u8]) -> Result<Vec<u8>> {
    let len = 1 + payload.len();
    let len = u16::try_from(len).map_err(|_| {
        Error::MalformedPacket(format!(
            "payload too large: {} bytes (max 65534)",
            payload.len()
        ))
    })?;

    let mut pkt = Vec::with_capacity(2 + len as usize);
    pkt.extend_from_slice(&len.to_be_bytes());
    pkt.push(packet_type);
    pkt.extend_from_slice(payload);
    Ok(pkt)
}

fn framed_empty(packet_type: u8) -> [u8; 3] {
    let mut pkt = [0u8; 3];
    pkt[0..2].copy_from_slice(&1u16.to_be_bytes());
    pkt[2] = packet_type;
    pkt
}

const LOGIN_TYPE: u8 = b'L';
const USERNAME_LEN: usize = 6;
const PASSWORD_LEN: usize = 10;
const SESSION_LEN: usize = 10;
const SEQ_NUM_LEN: usize = 20;
pub const LOGIN_PAYLOAD_LEN: usize = USERNAME_LEN + PASSWORD_LEN + SESSION_LEN + SEQ_NUM_LEN;
const LOGIN_WIRE_LEN: usize = 2 + 1 + LOGIN_PAYLOAD_LEN;

#[derive(Debug)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    pub requested_session: String,
    pub requested_sequence_number: String,
}

impl LoginRequest {
    pub fn new(
        username: &str,
        password: &str,
        requested_session: &str,
        requested_sequence_number: &str,
    ) -> Self {
        Self {
            username: username.to_string(),
            password: password.to_string(),
            requested_session: requested_session.to_string(),
            requested_sequence_number: requested_sequence_number.to_string(),
        }
    }

    pub fn to_wire(&self) -> [u8; LOGIN_WIRE_LEN] {
        let len: u16 = (1 + LOGIN_PAYLOAD_LEN) as u16;
        let mut pkt = [0u8; LOGIN_WIRE_LEN];
        pkt[0..2].copy_from_slice(&len.to_be_bytes());
        pkt[2] = LOGIN_TYPE;
        pkt[3..9].copy_from_slice(&pad_ascii(&self.username, USERNAME_LEN));
        pkt[9..19].copy_from_slice(&pad_ascii(&self.password, PASSWORD_LEN));
        pkt[19..29].copy_from_slice(&pad_ascii(&self.requested_session, SESSION_LEN));
        pkt[29..49].copy_from_slice(&pad_ascii(&self.requested_sequence_number, SEQ_NUM_LEN));
        pkt
    }

    pub fn parse(buf: &[u8]) -> Result<LoginRequest> {
        if buf.len() < LOGIN_PAYLOAD_LEN {
            return Err(Error::MalformedPacket(format!(
                "login request packet too short: {} bytes",
                buf.len()
            )));
        }
        Ok(LoginRequest {
            username: trim_ascii(&buf[0..6]),
            password: trim_ascii(&buf[6..16]),
            requested_session: trim_ascii(&buf[16..26]),
            requested_sequence_number: trim_ascii(&buf[26..46]),
        })
    }
}

const LOGIN_ACCEPTED_TYPE: u8 = b'A';
const LOGIN_ACCEPTED_SESSION_LEN: usize = 10;
const LOGIN_ACCEPTED_SEQUENCE_NUMBER_LEN: usize = 20;
const LOGIN_ACCEPTED_PAYLOAD_LEN: usize =
    LOGIN_ACCEPTED_SESSION_LEN + LOGIN_ACCEPTED_SEQUENCE_NUMBER_LEN;
const LOGIN_ACCEPTED_WIRE_LEN: usize = 2 + 1 + LOGIN_ACCEPTED_PAYLOAD_LEN;

#[derive(Debug)]
pub struct LoginAccepted {
    pub session: String,
    pub sequence_number: String,
}

impl LoginAccepted {
    pub fn new(session: &str, sequence_number: &str) -> Self {
        Self {
            session: session.to_string(),
            sequence_number: sequence_number.to_string(),
        }
    }

    pub fn to_wire(&self) -> [u8; LOGIN_ACCEPTED_WIRE_LEN] {
        let len: u16 = (1 + LOGIN_ACCEPTED_PAYLOAD_LEN) as u16;
        let mut pkt = [0u8; LOGIN_ACCEPTED_WIRE_LEN];
        pkt[0..2].copy_from_slice(&len.to_be_bytes());
        pkt[2] = LOGIN_ACCEPTED_TYPE;
        pkt[3..13].copy_from_slice(&pad_ascii(&self.session, LOGIN_ACCEPTED_SESSION_LEN));
        pkt[13..33].copy_from_slice(&pad_ascii(
            &self.sequence_number,
            LOGIN_ACCEPTED_SEQUENCE_NUMBER_LEN,
        ));
        pkt
    }

    pub fn parse(buf: &[u8]) -> Result<LoginAccepted> {
        if buf.len() < LOGIN_ACCEPTED_PAYLOAD_LEN {
            return Err(Error::MalformedPacket(format!(
                "login accepted packet too short: {} bytes",
                buf.len()
            )));
        }
        Ok(LoginAccepted {
            session: trim_ascii(&buf[0..10]),
            sequence_number: trim_ascii(&buf[10..30]),
        })
    }
}

const LOGIN_REJECTED_TYPE: u8 = b'J';
const LOGIN_REJECTED_PAYLOAD_LEN: usize = 1;
const LOGIN_REJECTED_WIRE_LEN: usize = 2 + 1 + LOGIN_REJECTED_PAYLOAD_LEN;

#[derive(Debug)]
pub struct LoginRejected {
    pub reason: String,
}

impl LoginRejected {
    pub fn new(reason: char) -> Self {
        Self {
            reason: reason.to_string(),
        }
    }

    pub fn to_wire(&self) -> Result<[u8; LOGIN_REJECTED_WIRE_LEN]> {
        let reason = *self
            .reason
            .as_bytes()
            .first()
            .ok_or_else(|| Error::MalformedPacket("login rejected reason is empty".into()))?;
        let mut pkt = [0u8; LOGIN_REJECTED_WIRE_LEN];
        pkt[0..2].copy_from_slice(&2u16.to_be_bytes());
        pkt[2] = LOGIN_REJECTED_TYPE;
        pkt[3] = reason;
        Ok(pkt)
    }

    pub fn parse(buf: &[u8]) -> Result<LoginRejected> {
        let Some(&reason) = buf.first() else {
            return Err(Error::MalformedPacket(
                "login rejected packet too short".into(),
            ));
        };

        Ok(LoginRejected {
            reason: (reason as char).to_string(),
        })
    }
}

const SEQUENCED_TYPE: u8 = b'S';

pub struct SequencedData;
impl SequencedData {
    pub fn to_wire(payload: &[u8]) -> Result<Vec<u8>> {
        framed(SEQUENCED_TYPE, payload)
    }
}

const UNSEQUENCED_TYPE: u8 = b'U';

pub struct UnsequencedData;
impl UnsequencedData {
    pub fn to_wire(payload: &[u8]) -> Result<Vec<u8>> {
        framed(UNSEQUENCED_TYPE, payload)
    }
}

const CLIENT_HEARTBEAT_TYPE: u8 = b'R';
const SERVER_HEARTBEAT_TYPE: u8 = b'H';

pub struct ClientHeartbeat;
impl ClientHeartbeat {
    pub fn to_wire() -> [u8; 3] {
        framed_empty(CLIENT_HEARTBEAT_TYPE)
    }
}

pub struct ServerHeartbeat;
impl ServerHeartbeat {
    pub fn to_wire() -> [u8; 3] {
        framed_empty(SERVER_HEARTBEAT_TYPE)
    }
}

const LOGOUT_TYPE: u8 = b'O';

pub struct LogoutRequest;
impl LogoutRequest {
    pub fn to_wire() -> [u8; 3] {
        framed_empty(LOGOUT_TYPE)
    }
}

const END_OF_SESSION_TYPE: u8 = b'Z';

pub struct EndOfSession;
impl EndOfSession {
    pub fn to_wire() -> [u8; 3] {
        framed_empty(END_OF_SESSION_TYPE)
    }
}

const DEBUG_TYPE: u8 = b'+';

pub struct Debug;
impl Debug {
    pub fn to_wire(text: &str) -> Result<Vec<u8>> {
        framed(DEBUG_TYPE, text.as_bytes())
    }
}

#[derive(Debug)]
pub enum Packet {
    LoginRequest(LoginRequest),
    LoginAccepted(LoginAccepted),
    LoginRejected(LoginRejected),
    SequencedData(Vec<u8>),
    UnsequencedData(Vec<u8>),
    ClientHeartbeat,
    ServerHeartbeat,
    LogoutRequest,
    EndOfSession,
    Debug(String),
}

impl Packet {
    /// Decode a packet payload (everything after the 2-byte length header).
    /// `packet_type` is the first byte of that payload, `buf` the rest.
    pub fn decode(packet_type: u8, buf: &[u8]) -> Result<Packet> {
        match packet_type {
            b'L' => Ok(Packet::LoginRequest(LoginRequest::parse(buf)?)),
            b'A' => Ok(Packet::LoginAccepted(LoginAccepted::parse(buf)?)),
            b'J' => Ok(Packet::LoginRejected(LoginRejected::parse(buf)?)),
            b'S' => Ok(Packet::SequencedData(buf.to_vec())),
            b'U' => Ok(Packet::UnsequencedData(buf.to_vec())),
            b'R' => Ok(Packet::ClientHeartbeat),
            b'H' => Ok(Packet::ServerHeartbeat),
            b'O' => Ok(Packet::LogoutRequest),
            b'Z' => Ok(Packet::EndOfSession),
            b'+' => Ok(Packet::Debug(trim_ascii(buf))),
            _ => Err(Error::UnknownPacket(format!(
                "unknown type: {}",
                packet_type
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_request_wire_layout() {
        let req = LoginRequest::new("USER", "P@SSWORD", "", "1");
        let pkt = req.to_wire();
        assert_eq!(pkt.len(), 49);
        assert_eq!(&pkt[0..2], &47u16.to_be_bytes());
        assert_eq!(pkt[2], b'L');
        assert_eq!(&pkt[3..9], b"USER  ");
        assert_eq!(&pkt[9..19], b"P@SSWORD  ");
        assert_eq!(&pkt[19..29], b"          ");
        assert_eq!(&pkt[29..49], b"1                   ");
    }

    #[test]
    fn login_request_parse_trims_fields() {
        let req = LoginRequest::new("USER", "P@SSWORD", "SESSION1", "42");
        let parsed = LoginRequest::parse(&req.to_wire()[3..]).unwrap();
        assert_eq!(parsed.username, "USER");
        assert_eq!(parsed.password, "P@SSWORD");
        assert_eq!(parsed.requested_session, "SESSION1");
        assert_eq!(parsed.requested_sequence_number, "42");
    }

    #[test]
    fn login_request_parse_rejects_short_buffer() {
        assert!(matches!(
            LoginRequest::parse(b"short"),
            Err(Error::MalformedPacket(_))
        ));
    }

    #[test]
    fn pad_ascii_pads_and_truncates() {
        assert_eq!(pad_ascii("ab", 4), b"ab  ");
        assert_eq!(pad_ascii("abcdef", 4), b"abcd");
        assert_eq!(pad_ascii("", 2), b"  ");
    }

    #[test]
    fn login_accepted_wire_layout() {
        let accepted = LoginAccepted::new("SESSION1", "1");
        let pkt = accepted.to_wire();
        assert_eq!(pkt.len(), 33);
        assert_eq!(&pkt[0..2], &31u16.to_be_bytes());
        assert_eq!(pkt[2], b'A');
        assert_eq!(&pkt[3..13], b"SESSION1  ");
        assert_eq!(&pkt[13..33], b"1                   ");
    }

    #[test]
    fn login_accepted_parse_trims_fields() {
        let accepted = LoginAccepted::parse(b"SESSION1  1                   ").unwrap();
        assert_eq!(accepted.session, "SESSION1");
        assert_eq!(accepted.sequence_number, "1");
    }

    #[test]
    fn login_accepted_roundtrip() {
        let accepted = LoginAccepted::new("SESSION1", "123");
        let parsed = LoginAccepted::parse(&accepted.to_wire()[3..]).unwrap();
        assert_eq!(parsed.session, "SESSION1");
        assert_eq!(parsed.sequence_number, "123");
    }

    #[test]
    fn login_accepted_parse_rejects_short_buffer() {
        assert!(matches!(
            LoginAccepted::parse(b"short"),
            Err(Error::MalformedPacket(_))
        ));
    }

    #[test]
    fn login_rejected_wire_layout() {
        let pkt = LoginRejected::new('S').to_wire().unwrap();
        assert_eq!(pkt, [0, 2, b'J', b'S']);
    }

    #[test]
    fn login_rejected_parse_reads_reason() {
        let rejected = LoginRejected::parse(b"A").unwrap();
        assert_eq!(rejected.reason, "A");
    }

    #[test]
    fn login_rejected_parse_rejects_empty_buffer() {
        assert!(matches!(
            LoginRejected::parse(b""),
            Err(Error::MalformedPacket(_))
        ));
    }

    #[test]
    fn sequenced_data_wire_framing() {
        let pkt = SequencedData::to_wire(b"hi").unwrap();
        assert_eq!(pkt, vec![0, 3, b'S', b'h', b'i']);
    }

    #[test]
    fn unsequenced_data_wire_framing() {
        let pkt = UnsequencedData::to_wire(b"hi").unwrap();
        assert_eq!(pkt, vec![0, 3, b'U', b'h', b'i']);
    }

    #[test]
    fn framed_rejects_oversized_payload() {
        let big = vec![0u8; 65535];
        assert!(matches!(
            UnsequencedData::to_wire(&big),
            Err(Error::MalformedPacket(_))
        ));
        assert!(matches!(
            SequencedData::to_wire(&big),
            Err(Error::MalformedPacket(_))
        ));
    }

    #[test]
    fn empty_packet_wire_layouts() {
        assert_eq!(ClientHeartbeat::to_wire(), [0, 1, b'R']);
        assert_eq!(ServerHeartbeat::to_wire(), [0, 1, b'H']);
        assert_eq!(LogoutRequest::to_wire(), [0, 1, b'O']);
        assert_eq!(EndOfSession::to_wire(), [0, 1, b'Z']);
    }

    #[test]
    fn decode_known_types_both_directions() {
        assert!(matches!(
            Packet::decode(b'H', b"").unwrap(),
            Packet::ServerHeartbeat
        ));
        assert!(matches!(
            Packet::decode(b'R', b"").unwrap(),
            Packet::ClientHeartbeat
        ));
        assert!(matches!(
            Packet::decode(b'O', b"").unwrap(),
            Packet::LogoutRequest
        ));
        assert!(matches!(
            Packet::decode(b'Z', b"").unwrap(),
            Packet::EndOfSession
        ));
        assert!(
            matches!(Packet::decode(b'S', b"xyz").unwrap(), Packet::SequencedData(d) if d == b"xyz")
        );
        assert!(
            matches!(Packet::decode(b'U', b"xyz").unwrap(), Packet::UnsequencedData(d) if d == b"xyz")
        );
        assert!(matches!(Packet::decode(b'+', b"note").unwrap(), Packet::Debug(t) if t == "note"));
        assert!(matches!(
            Packet::decode(b'L', &LoginRequest::new("U", "P", "", "1").to_wire()[3..]).unwrap(),
            Packet::LoginRequest(_)
        ));
    }

    #[test]
    fn decode_rejects_unknown_type() {
        assert!(matches!(
            Packet::decode(b'X', b""),
            Err(Error::UnknownPacket(_))
        ));
    }
}

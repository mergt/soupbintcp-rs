use soupbintcp_core::{Error, LoginAccepted, LoginRejected, Packet, Result};

#[derive(Debug)]
pub enum Event {
    LoginAcceptedEvent(LoginAccepted),
    LoginRejectedEvent(LoginRejected),
    SequencedDataEvent(Vec<u8>),
    UnsequencedDataEvent(Vec<u8>),
    HeartbeatEvent,
    EndOfSessionEvent,
    DebugEvent(String),
    /// The connection closed after we sent a Logout Request — a clean,
    /// client-initiated shutdown rather than a failure.
    LoggedOut,
    Disconnected(String),
}

pub(crate) fn into_event(packet_type: u8, buf: &[u8]) -> Result<Event> {
    match Packet::decode(packet_type, buf)? {
        Packet::LoginAccepted(accepted) => Ok(Event::LoginAcceptedEvent(accepted)),
        Packet::LoginRejected(rejected) => Ok(Event::LoginRejectedEvent(rejected)),
        Packet::SequencedData(data) => Ok(Event::SequencedDataEvent(data)),
        Packet::UnsequencedData(data) => Ok(Event::UnsequencedDataEvent(data)),
        Packet::ServerHeartbeat => Ok(Event::HeartbeatEvent),
        Packet::EndOfSession => Ok(Event::EndOfSessionEvent),
        Packet::Debug(text) => Ok(Event::DebugEvent(text)),
        other => Err(Error::UnknownPacket(format!(
            "unexpected packet from server: {other:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn into_event_decodes_known_types() {
        assert!(matches!(
            into_event(b'H', b"").unwrap(),
            Event::HeartbeatEvent
        ));
        assert!(matches!(
            into_event(b'Z', b"").unwrap(),
            Event::EndOfSessionEvent
        ));
        assert!(
            matches!(into_event(b'S', b"xyz").unwrap(), Event::SequencedDataEvent(d) if d == b"xyz")
        );
        assert!(
            matches!(into_event(b'U', b"xyz").unwrap(), Event::UnsequencedDataEvent(d) if d == b"xyz")
        );
        assert!(matches!(
            into_event(b'+', b"note").unwrap(),
            Event::DebugEvent(t) if t == "note"
        ));
    }

    #[test]
    fn into_event_rejects_unknown_type() {
        assert!(matches!(
            into_event(b'X', b""),
            Err(Error::UnknownPacket(_))
        ));
    }

    #[test]
    fn into_event_rejects_client_only_packets() {
        assert!(matches!(
            into_event(b'R', b""),
            Err(Error::UnknownPacket(_))
        ));
        assert!(matches!(
            into_event(b'O', b""),
            Err(Error::UnknownPacket(_))
        ));
    }
}

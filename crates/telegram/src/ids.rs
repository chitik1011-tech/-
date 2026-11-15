use grammers_client::session::types::{PeerId, PeerKind};

const CHANNEL_ID_OFFSET: i64 = 1_000_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PeerClass {
    User,
    Chat,
    Channel,
}

pub(crate) fn encode_peer(class: PeerClass, bare_id: i64) -> i64 {
    let bare_id = bare_id.saturating_abs();
    match class {
        PeerClass::User => bare_id,
        PeerClass::Chat => bare_id.saturating_neg(),
        PeerClass::Channel => CHANNEL_ID_OFFSET.saturating_add(bare_id).saturating_neg(),
    }
}

pub(crate) fn peer_id_to_core(peer_id: PeerId) -> i64 {
    let class = match peer_id.kind() {
        PeerKind::User => PeerClass::User,
        PeerKind::Chat => PeerClass::Chat,
        PeerKind::Channel => PeerClass::Channel,
    };
    encode_peer(class, peer_id.bare_id_unchecked())
}

pub(crate) fn channel_id_to_core(channel_id: i64) -> i64 {
    encode_peer(PeerClass::Channel, channel_id)
}

#[cfg(test)]
mod tests {
    use super::{PeerClass, encode_peer};

    #[test]
    fn peer_classes_do_not_collide() {
        assert_eq!(encode_peer(PeerClass::User, 42), 42);
        assert_eq!(encode_peer(PeerClass::Chat, 42), -42);
        assert_eq!(encode_peer(PeerClass::Channel, 42), -1_000_000_000_042);
    }

    #[test]
    fn negative_bare_ids_are_normalized() {
        assert_eq!(encode_peer(PeerClass::User, -42), 42);
        assert_eq!(encode_peer(PeerClass::Chat, -42), -42);
    }
}

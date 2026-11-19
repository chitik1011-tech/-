use std::collections::BTreeMap;

use grammers_client::{Client, peer::User, tl};
use tg_mobile_core::{
    Contact, ContactBook, CoreError, MAX_CONTACT_PAGE_LIMIT, normalize_contact_search_query,
    validate_contact_search,
};

use crate::{
    chats::{PeerRegistry, remember_peer},
    error::api_error,
    ids::peer_id_to_core,
};

/// Load Telegram's contact snapshot, remember usable peer references for direct
/// conversation navigation, and return one stable-ID-ordered page.
pub(crate) async fn load_contacts(
    client: &Client,
    peers: &PeerRegistry,
    offset: usize,
    limit: usize,
) -> tg_mobile_core::Result<(usize, Vec<Contact>)> {
    if !(1..=MAX_CONTACT_PAGE_LIMIT).contains(&limit) {
        return Err(CoreError::InvalidInput(
            "contact page limit is invalid".into(),
        ));
    }

    let response = client
        .invoke(&tl::functions::contacts::GetContacts { hash: 0 })
        .await
        .map_err(|error| api_error("load contacts", error))?;
    let tl::enums::contacts::Contacts::Contacts(response) = response else {
        return Err(CoreError::Backend(
            "Telegram returned an unchanged contact snapshot without a local baseline".into(),
        ));
    };

    let mutual_by_id = response
        .contacts
        .into_iter()
        .map(|contact| match contact {
            tl::enums::Contact::Contact(contact) => (contact.user_id, contact.mutual),
        })
        .collect::<BTreeMap<_, _>>();

    let mut contacts = ContactBook::default();
    for user in response.users {
        let peer = User::from_raw(client, user);
        let user_id = peer_id_to_core(peer.id());
        // Telegram's deleted-account stubs cannot resolve into a usable direct chat.
        if peer.deleted() {
            continue;
        }
        let Some(is_mutual) = mutual_by_id.get(&user_id).copied() else {
            continue;
        };

        let Some(peer_ref) = peer
            .to_ref()
            .await
            .map_err(|error| CoreError::Backend(error.to_string()))?
        else {
            // Without an authorized peer reference the row could not open a chat.
            continue;
        };
        remember_peer(peers, user_id, peer_ref).await;

        let contact = contact_summary(
            user_id,
            peer.first_name().unwrap_or_default(),
            peer.last_name().unwrap_or_default(),
            peer.username(),
            is_mutual,
            peer.is_bot(),
            peer.photo().map(|photo| photo.photo_id),
            true,
        )?;
        contacts.upsert(contact)?;
    }

    let total = contacts.len();
    Ok((total, contacts.page(offset, limit)?))
}

/// Search Telegram's public username directory. Telegram excludes the current
/// account's saved contacts from this RPC; callers can combine it with the
/// already-loaded local snapshot when they need both.
pub(crate) async fn search_contacts(
    client: &Client,
    peers: &PeerRegistry,
    query: &str,
    limit: usize,
) -> tg_mobile_core::Result<Vec<Contact>> {
    validate_contact_search(query, limit)?;
    let response = client
        .invoke(&tl::functions::contacts::Search {
            q: normalize_contact_search_query(query),
            limit: limit as i32,
            bots: false,
            broadcasts: false,
        })
        .await
        .map_err(|error| api_error("search contacts", error))?;
    let tl::enums::contacts::Found::Found(found) = response else {
        return Err(CoreError::Backend(
            "Telegram returned an unsupported contacts search result".into(),
        ));
    };

    let mut result_ids = Vec::new();
    for peer in found.my_results.into_iter().chain(found.results) {
        if let tl::enums::Peer::User(peer) = peer {
            if !result_ids.contains(&peer.user_id) {
                result_ids.push(peer.user_id);
            }
        }
    }

    let mut users = found
        .users
        .into_iter()
        .map(|user| {
            let peer = User::from_raw(client, user);
            (peer_id_to_core(peer.id()), peer)
        })
        .collect::<Vec<_>>();
    let mut contacts = Vec::new();
    for user_id in result_ids {
        let Some(index) = users
            .iter()
            .position(|(candidate_id, _)| *candidate_id == user_id)
        else {
            continue;
        };
        let (_, peer) = users.remove(index);
        if peer.deleted() {
            continue;
        }
        let Some(peer_ref) = peer
            .to_ref()
            .await
            .map_err(|error| CoreError::Backend(error.to_string()))?
        else {
            continue;
        };
        remember_peer(peers, user_id, peer_ref).await;
        contacts.push(contact_summary(
            user_id,
            peer.first_name().unwrap_or_default(),
            peer.last_name().unwrap_or_default(),
            peer.username(),
            false,
            peer.is_bot(),
            peer.photo().map(|photo| photo.photo_id),
            false,
        )?);
    }
    Ok(contacts)
}

fn contact_summary(
    user_id: i64,
    first_name: &str,
    last_name: &str,
    username: Option<&str>,
    is_mutual: bool,
    is_bot: bool,
    avatar_photo_id: Option<i64>,
    is_contact: bool,
) -> tg_mobile_core::Result<Contact> {
    let display_name = [first_name.trim(), last_name.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let display_name = if display_name.is_empty() {
        "Telegram user".to_owned()
    } else {
        display_name
    };
    Contact {
        user_id,
        display_name,
        username: username.map(str::to_owned),
        is_mutual,
        is_contact,
        is_bot,
        avatar_photo_id,
    }
    .normalized()
}

#[cfg(test)]
mod tests {
    use grammers_client::session::types::{PeerAuth, PeerId, PeerRef};

    use super::contact_summary;

    #[test]
    fn maps_contact_identity_without_phone_data() {
        let contact = contact_summary(
            42,
            "Ada",
            "Lovelace",
            Some("Ada_L"),
            true,
            false,
            Some(9001),
            true,
        )
        .unwrap();
        assert_eq!(contact.user_id, 42);
        assert_eq!(contact.display_name, "Ada Lovelace");
        assert_eq!(contact.username.as_deref(), Some("ada_l"));
        assert!(contact.is_mutual);
        assert_eq!(contact.avatar_photo_id, Some(9001));
        let json = serde_json::to_value(contact).unwrap();
        assert!(json.get("phone").is_none());
    }

    #[test]
    fn missing_server_names_use_a_non_sensitive_fallback() {
        let contact = contact_summary(8, " ", "", None, false, true, None, true).unwrap();
        assert_eq!(contact.display_name, "Telegram user");
        assert_eq!(contact.user_id, 8);
        assert!(contact.is_bot);
    }

    #[test]
    fn server_search_result_is_marked_as_not_a_saved_contact() {
        let contact = contact_summary(
            42,
            "Ada",
            "Lovelace",
            Some("Ada_L"),
            false,
            false,
            None,
            false,
        )
        .unwrap();
        assert!(!contact.is_contact);
        assert_eq!(contact.username.as_deref(), Some("ada_l"));
    }

    #[tokio::test]
    async fn contact_navigation_uses_the_stable_user_id_key() {
        let registry = crate::chats::new_peer_registry();
        let reference = PeerRef {
            id: PeerId::user(42).unwrap(),
            auth: PeerAuth::default(),
        };
        crate::chats::remember_peer(&registry, 42, reference).await;
        let found = crate::chats::find_peer(&registry, 42).await.unwrap();
        assert_eq!(found, reference);
        assert!(crate::chats::find_peer(&registry, 43).await.is_none());
    }
}

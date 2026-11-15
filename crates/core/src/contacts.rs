use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    error::{CoreError, Result},
    event::CoreEvent,
};

/// Maximum number of contacts returned by one paginated list request.
pub const MAX_CONTACT_PAGE_LIMIT: usize = 100;
const MAX_DISPLAY_NAME_CHARS: usize = 256;
const MIN_USERNAME_CHARS: usize = 5;
const MAX_USERNAME_CHARS: usize = 32;

/// A Telegram user known to the account. Phone numbers are deliberately not part
/// of this model; callers should use the stable user ID to refer to a contact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    pub user_id: i64,
    pub display_name: String,
    pub username: Option<String>,
    pub is_imported: bool,
    pub is_contact: bool,
}

impl Contact {
    /// Validate and canonicalize user supplied fields without exposing values in errors.
    pub fn normalized(mut self) -> Result<Self> {
        if self.user_id == 0 {
            return Err(CoreError::InvalidInput(
                "contact user id must be non-zero".into(),
            ));
        }

        self.display_name = self.display_name.trim().to_owned();
        if self.display_name.is_empty() {
            return Err(CoreError::InvalidInput(
                "contact display name is empty".into(),
            ));
        }
        if self.display_name.chars().count() > MAX_DISPLAY_NAME_CHARS {
            return Err(CoreError::InvalidInput(
                "contact display name is too long".into(),
            ));
        }

        self.username = self
            .username
            .take()
            .map(|username| username.trim().trim_start_matches('@').to_ascii_lowercase())
            .filter(|username| !username.is_empty());

        if let Some(username) = &self.username {
            let valid_length = (MIN_USERNAME_CHARS..=MAX_USERNAME_CHARS).contains(&username.len());
            let valid_characters = username
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
            if !valid_length || !valid_characters {
                return Err(CoreError::InvalidInput(
                    "contact username is invalid".into(),
                ));
            }
        }

        Ok(self)
    }
}

/// Contacts sorted by stable user ID. Upserts are idempotent, usernames are
/// case-insensitive, and if two IDs claim the same username the lower ID wins.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContactBook {
    #[serde(default)]
    by_user_id: BTreeMap<i64, Contact>,
}

impl ContactBook {
    pub fn len(&self) -> usize {
        self.by_user_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_user_id.is_empty()
    }

    pub fn get(&self, user_id: i64) -> Option<&Contact> {
        self.by_user_id.get(&user_id)
    }

    pub fn all(&self) -> impl ExactSizeIterator<Item = &Contact> {
        self.by_user_id.values()
    }

    /// Return a stable page sorted by user ID. Limit validation is shared with
    /// the command layer so callers cannot accidentally request an unbounded page.
    pub fn page(&self, offset: usize, limit: usize) -> Result<Vec<Contact>> {
        validate_contact_page(limit)?;
        Ok(self
            .by_user_id
            .values()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect())
    }

    /// Insert or update one contact. Returns true only when the book changes.
    pub fn upsert(&mut self, contact: Contact) -> Result<bool> {
        let contact = contact.normalized()?;

        if let Some(username) = contact.username.as_deref() {
            let conflicting_id = self
                .by_user_id
                .values()
                .find(|existing| {
                    existing.user_id != contact.user_id
                        && existing.username.as_deref() == Some(username)
                })
                .map(|existing| existing.user_id);

            if let Some(conflicting_id) = conflicting_id {
                if conflicting_id < contact.user_id {
                    return Ok(false);
                }
                self.by_user_id.remove(&conflicting_id);
            }
        }

        if self.by_user_id.get(&contact.user_id) == Some(&contact) {
            return Ok(false);
        }

        self.by_user_id.insert(contact.user_id, contact);
        Ok(true)
    }

    pub fn remove(&mut self, user_id: i64) -> bool {
        self.by_user_id.remove(&user_id).is_some()
    }

    /// Apply contact events to a snapshot. Invalid payloads are ignored at this
    /// projection boundary; command validation and backend validation report errors.
    pub fn reduce(&mut self, event: &CoreEvent) {
        match event {
            CoreEvent::ContactAdded(contact) => {
                let _ = self.upsert(contact.clone());
            }
            CoreEvent::ContactDeleted { user_id } => {
                self.remove(*user_id);
            }
            CoreEvent::ContactsLoaded { contacts, .. } => {
                for contact in contacts {
                    let _ = self.upsert(contact.clone());
                }
            }
            _ => {}
        }
    }
}

/// Validate a bounded page request for the contact list.
pub fn validate_contact_page(limit: usize) -> Result<()> {
    if (1..=MAX_CONTACT_PAGE_LIMIT).contains(&limit) {
        Ok(())
    } else {
        Err(CoreError::InvalidInput(format!(
            "contact page limit must be in 1..={MAX_CONTACT_PAGE_LIMIT}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contact(user_id: i64, username: Option<&str>) -> Contact {
        Contact {
            user_id,
            display_name: format!("Person {user_id}"),
            username: username.map(str::to_owned),
            is_imported: false,
            is_contact: true,
        }
    }

    #[test]
    fn validation_trims_names_and_canonicalizes_usernames() {
        let normalized = contact(1, Some(" @Example_User ")).normalized().unwrap();
        assert_eq!(normalized.display_name, "Person 1");
        assert_eq!(normalized.username.as_deref(), Some("example_user"));
        assert!(contact(0, None).normalized().is_err());
        assert!(Contact {
            display_name: " ".into(),
            ..contact(1, None)
        }
        .normalized()
        .is_err());
        assert!(contact(1, Some("bad name")).normalized().is_err());
    }

    #[test]
    fn duplicate_username_resolution_is_stable_and_idempotent() {
        let mut contacts = ContactBook::default();
        assert!(contacts.upsert(contact(9, Some("SameName"))).unwrap());
        assert!(!contacts.upsert(contact(9, Some("@samename"))).unwrap());
        assert!(contacts.upsert(contact(3, Some("SAMENAME"))).unwrap());
        assert_eq!(contacts.len(), 1);
        assert!(contacts.get(3).is_some());
        assert!(contacts.get(9).is_none());
        assert!(!contacts.upsert(contact(9, Some("samename"))).unwrap());
        assert_eq!(contacts.page(0, 10).unwrap()[0].user_id, 3);
    }

    #[test]
    fn pages_are_bounded_and_deterministic() {
        let mut contacts = ContactBook::default();
        contacts.upsert(contact(3, None)).unwrap();
        contacts.upsert(contact(1, None)).unwrap();
        contacts.upsert(contact(2, None)).unwrap();

        assert_eq!(
            contacts
                .page(1, 2)
                .unwrap()
                .iter()
                .map(|c| c.user_id)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert!(contacts.page(0, 0).is_err());
        assert!(contacts.page(0, MAX_CONTACT_PAGE_LIMIT + 1).is_err());
    }
}

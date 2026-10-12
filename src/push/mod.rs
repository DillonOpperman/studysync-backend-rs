//! FCM HTTP v1 delivery. PostgreSQL notifications remain the source of truth.

mod fcm;
mod worker;

pub use fcm::{FcmHttpSender, FcmSendError, FcmSender, SendOutcome};
pub use worker::{process_due_batch, spawn_worker, MAX_ATTEMPTS};

use std::sync::Arc;

use sha2::{Digest, Sha256};

#[derive(Clone)]
pub struct PushRuntime {
    /// True only when credentials loaded and PUSH_ENABLED requested.
    pub delivery_enabled: bool,
    pub sender: Arc<dyn FcmSender>,
}

impl PushRuntime {
    pub fn disabled() -> Self {
        Self {
            delivery_enabled: false,
            sender: Arc::new(fcm::DisabledSender),
        }
    }
}

pub fn token_fingerprint(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub fn category_for_type(ntype: &str) -> Option<&'static str> {
    match ntype {
        "friend_request" | "friend_request_accepted" | "friend_request_rejected" => Some("friends"),
        "join_request" | "request_approved" | "request_rejected" => Some("groups"),
        "direct_message" | "message_request" | "message_request_accepted" => Some("messages"),
        "group_announcement" => Some("announcements"),
        _ => None,
    }
}

/// Conservative OS-visible copy. Never include message bodies or contact details.
pub fn public_preview(ntype: &str) -> Option<(&'static str, &'static str)> {
    match ntype {
        "friend_request" => Some(("StudySync", "You have a new friend request")),
        "friend_request_accepted" => Some(("StudySync", "A friend request was accepted")),
        "friend_request_rejected" => Some(("StudySync", "A friend request was declined")),
        "join_request" => Some(("StudySync", "A group has a new join request")),
        "request_approved" => Some(("StudySync", "A group membership request was updated")),
        "request_rejected" => Some(("StudySync", "A group membership request was updated")),
        "direct_message" | "message_request" => Some(("StudySync", "New message on StudySync")),
        "message_request_accepted" => Some(("StudySync", "You can now message this person")),
        "group_announcement" => Some(("StudySync", "A group posted an announcement")),
        _ => None,
    }
}

pub fn preference_allows(
    category: &str,
    friends: bool,
    groups: bool,
    messages: bool,
    announcements: bool,
) -> bool {
    match category {
        "friends" => friends,
        "groups" => groups,
        "messages" => messages,
        "announcements" => announcements,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_stable_sha256_hex() {
        let a = token_fingerprint("token-one");
        let b = token_fingerprint("token-one");
        let c = token_fingerprint("token-two");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn unknown_types_are_not_pushed() {
        assert!(category_for_type("ordinary_group_chat").is_none());
        assert!(public_preview("ordinary_group_chat").is_none());
    }

    #[test]
    fn dm_preview_is_generic() {
        let (title, body) = public_preview("direct_message").unwrap();
        assert_eq!(body, "New message on StudySync");
        assert!(!title.to_lowercase().contains("secret"));
    }
}

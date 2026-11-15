use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use crate::{event::CoreEvent, model::ChatActivityKind};

pub const DEFAULT_CHAT_ACTIVITY_TTL_MS: u64 = 6_000;
pub const MAX_CHAT_ACTIVITY_TTL_MS: u64 = 30_000;
pub const CHAT_ACTIVITY_EXPIRY_CHECK_INTERVAL_MS: u64 = 250;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ActivityKey {
    chat_id: i64,
    user_id: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ActiveActivity {
    generation: u64,
    kind: ChatActivityKind,
    expires_at: Instant,
}

/// Process-local activity state. It is deliberately separate from CoreState
/// and is never sent to a StateStore.
#[derive(Debug, Default)]
pub struct ChatActivityTracker {
    active: BTreeMap<ActivityKey, ActiveActivity>,
}

impl ChatActivityTracker {
    pub fn has_active(&self) -> bool {
        !self.active.is_empty()
    }

    /// Apply a transient activity event using a monotonic clock supplied by the caller.
    pub fn apply(&mut self, event: &CoreEvent, now: Instant) {
        match event {
            CoreEvent::ChatActivityChanged {
                chat_id,
                user_id,
                generation,
                activity,
                ttl_ms,
            } => {
                if *chat_id == 0 || *user_id == 0 || *generation == 0 {
                    return;
                }
                let key = ActivityKey {
                    chat_id: *chat_id,
                    user_id: *user_id,
                };
                if self
                    .active
                    .get(&key)
                    .is_some_and(|current| *generation < current.generation)
                {
                    return;
                }

                let ttl = (*ttl_ms).clamp(1, MAX_CHAT_ACTIVITY_TTL_MS);
                self.active.insert(
                    key,
                    ActiveActivity {
                        generation: *generation,
                        kind: *activity,
                        expires_at: now + Duration::from_millis(ttl),
                    },
                );
            }
            CoreEvent::ChatActivityEnded {
                chat_id,
                user_id,
                generation,
            }
            | CoreEvent::ChatActivityExpired {
                chat_id,
                user_id,
                generation,
            } => {
                let key = ActivityKey {
                    chat_id: *chat_id,
                    user_id: *user_id,
                };
                if self
                    .active
                    .get(&key)
                    .is_some_and(|current| current.generation == *generation)
                {
                    self.active.remove(&key);
                }
            }
            _ => {}
        }
    }

    /// Remove activities whose refreshed TTL elapsed and return expiry events.
    pub fn expire(&mut self, now: Instant) -> Vec<CoreEvent> {
        let expired: Vec<_> = self
            .active
            .iter()
            .filter_map(|(key, activity)| {
                (activity.expires_at <= now).then_some((*key, activity.generation))
            })
            .collect();

        for (key, generation) in &expired {
            if self
                .active
                .get(key)
                .is_some_and(|current| current.generation == *generation)
            {
                self.active.remove(key);
            }
        }

        expired
            .into_iter()
            .map(|(key, generation)| CoreEvent::ChatActivityExpired {
                chat_id: key.chat_id,
                user_id: key.user_id,
                generation,
            })
            .collect()
    }

    pub fn activity(&self, chat_id: i64, user_id: i64) -> Option<ChatActivityKind> {
        self.active
            .get(&ActivityKey { chat_id, user_id })
            .map(|activity| activity.kind)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::{event::CoreEvent, model::ChatActivityKind, state::CoreState};

    use super::{ChatActivityTracker, DEFAULT_CHAT_ACTIVITY_TTL_MS, MAX_CHAT_ACTIVITY_TTL_MS};

    fn changed(
        chat_id: i64,
        user_id: i64,
        generation: u64,
        activity: ChatActivityKind,
        ttl_ms: u64,
    ) -> CoreEvent {
        CoreEvent::ChatActivityChanged {
            chat_id,
            user_id,
            generation,
            activity,
            ttl_ms,
        }
    }

    #[test]
    fn activity_expires_after_default_ttl_and_emits_scoped_event() {
        let now = Instant::now();
        let mut tracker = ChatActivityTracker::default();
        tracker.apply(
            &changed(
                10,
                20,
                1,
                ChatActivityKind::Typing,
                DEFAULT_CHAT_ACTIVITY_TTL_MS,
            ),
            now,
        );

        assert!(tracker
            .expire(now + Duration::from_millis(DEFAULT_CHAT_ACTIVITY_TTL_MS - 1))
            .is_empty());
        assert_eq!(
            tracker.expire(now + Duration::from_millis(DEFAULT_CHAT_ACTIVITY_TTL_MS)),
            vec![CoreEvent::ChatActivityExpired {
                chat_id: 10,
                user_id: 20,
                generation: 1,
            }]
        );
        assert!(!tracker.has_active());
    }

    #[test]
    fn activities_are_scoped_by_both_chat_and_user() {
        let now = Instant::now();
        let mut tracker = ChatActivityTracker::default();
        tracker.apply(&changed(1, 7, 1, ChatActivityKind::Typing, 50), now);
        tracker.apply(&changed(1, 8, 1, ChatActivityKind::RecordingVoice, 100), now);
        tracker.apply(&changed(2, 7, 1, ChatActivityKind::UploadingMedia, 150), now);

        tracker.apply(
            &CoreEvent::ChatActivityEnded {
                chat_id: 1,
                user_id: 7,
                generation: 1,
            },
            now,
        );
        assert_eq!(tracker.activity(1, 7), None);
        assert_eq!(
            tracker.activity(1, 8),
            Some(ChatActivityKind::RecordingVoice)
        );
        assert_eq!(
            tracker.activity(2, 7),
            Some(ChatActivityKind::UploadingMedia)
        );
    }

    #[test]
    fn new_activity_refreshes_only_its_own_expiry() {
        let now = Instant::now();
        let mut tracker = ChatActivityTracker::default();
        tracker.apply(&changed(3, 4, 1, ChatActivityKind::Typing, 20), now);
        tracker.apply(&changed(3, 5, 1, ChatActivityKind::Typing, 50), now);
        tracker.apply(
            &changed(3, 4, 2, ChatActivityKind::UploadingMedia, 100),
            now + Duration::from_millis(10),
        );

        let expired = tracker.expire(now + Duration::from_millis(51));
        assert_eq!(
            expired,
            vec![CoreEvent::ChatActivityExpired {
                chat_id: 3,
                user_id: 5,
                generation: 1,
            }]
        );
        assert_eq!(
            tracker.activity(3, 4),
            Some(ChatActivityKind::UploadingMedia)
        );
    }

    #[test]
    fn zero_ids_are_ignored_and_ttl_is_bounded() {
        let now = Instant::now();
        let mut tracker = ChatActivityTracker::default();
        tracker.apply(&changed(0, 5, 1, ChatActivityKind::Typing, 20), now);
        tracker.apply(&changed(5, 0, 1, ChatActivityKind::Typing, 20), now);
        tracker.apply(&changed(5, 6, 1, ChatActivityKind::Typing, u64::MAX), now);

        assert!(tracker
            .expire(now + Duration::from_millis(MAX_CHAT_ACTIVITY_TTL_MS - 1))
            .is_empty());
        assert!(tracker.has_active());
        assert_eq!(
            tracker.expire(now + Duration::from_millis(MAX_CHAT_ACTIVITY_TTL_MS)),
            vec![CoreEvent::ChatActivityExpired {
                chat_id: 5,
                user_id: 6,
                generation: 1,
            }]
        );
    }

    #[test]
    fn stale_end_cannot_clear_a_newer_activity_generation() {
        let now = Instant::now();
        let mut tracker = ChatActivityTracker::default();

        tracker.apply(&changed(9, 10, 1, ChatActivityKind::Typing, 100), now);
        tracker.apply(
            &changed(9, 10, 2, ChatActivityKind::RecordingVoice, 100),
            now + Duration::from_millis(1),
        );
        tracker.apply(
            &CoreEvent::ChatActivityEnded {
                chat_id: 9,
                user_id: 10,
                generation: 1,
            },
            now + Duration::from_millis(2),
        );

        assert_eq!(
            tracker.activity(9, 10),
            Some(ChatActivityKind::RecordingVoice)
        );

        tracker.apply(
            &CoreEvent::ChatActivityEnded {
                chat_id: 9,
                user_id: 10,
                generation: 2,
            },
            now + Duration::from_millis(3),
        );
        assert_eq!(tracker.activity(9, 10), None);
    }

    #[test]
    fn stale_changed_generation_cannot_replace_a_newer_activity() {
        let now = Instant::now();
        let mut tracker = ChatActivityTracker::default();

        tracker.apply(&changed(4, 5, 2, ChatActivityKind::UploadingMedia, 100), now);
        tracker.apply(
            &changed(4, 5, 1, ChatActivityKind::Typing, 100),
            now + Duration::from_millis(1),
        );

        assert_eq!(
            tracker.activity(4, 5),
            Some(ChatActivityKind::UploadingMedia)
        );
    }

    #[test]
    fn activity_events_do_not_enter_persistent_core_state() {
        let mut state = CoreState::default();
        let baseline = state.clone();
        state.reduce(&changed(1, 2, 1, ChatActivityKind::Typing, 100));
        state.reduce(&CoreEvent::ChatActivityEnded {
            chat_id: 1,
            user_id: 2,
            generation: 1,
        });

        assert_eq!(state, baseline);
        let json = serde_json::to_value(state).expect("CoreState serializes");
        assert!(json.get("activities").is_none());
    }
}

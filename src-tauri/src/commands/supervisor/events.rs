//! In-memory event ring (500). Every text is a fixed string chosen from the typed kind and state, so no
//! command output, URL, path or token can reach it.

use std::collections::VecDeque;

use serde::Serialize;

use super::machine::{ReasonCode, ServiceState};
use super::registry::ServiceId;

pub const CAP: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    StateChanged,
    Adopted,
    RestartRequested,
    RestartFailed,
    GaveUp,
    /// The app started it on launch (act on).
    Started,
    /// The app stopped it on quit (act on).
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ServiceEvent {
    /// Monotonic, starts at 1. Pass the last one you saw as `since`.
    pub seq: u64,
    pub ts_ms: u64,
    pub id: ServiceId,
    pub kind: EventKind,
    pub state: ServiceState,
    pub reason_code: ReasonCode,
    pub text: String,
}

pub(crate) fn state_text(state: ServiceState) -> &'static str {
    match state {
        ServiceState::Unknown => "Not checked yet",
        ServiceState::Starting => "Starting",
        ServiceState::Healthy => "Healthy",
        ServiceState::Degraded => "Slow or flaky",
        ServiceState::Restarting => "Restarting",
        ServiceState::NeedsYou => "Needs you",
        ServiceState::Stopped => "Stopped",
        ServiceState::Separate => "Running separately",
    }
}

pub(crate) fn kind_text(kind: EventKind, state: ServiceState) -> &'static str {
    match kind {
        EventKind::StateChanged => state_text(state),
        EventKind::Adopted => "Already running, adopted",
        EventKind::RestartRequested => "Restart requested",
        EventKind::RestartFailed => "Restart did not work",
        EventKind::GaveUp => "Gave up after 3 restarts",
        EventKind::Started => "Started by Amara OS",
        EventKind::Stopped => "Stopped by Amara OS",
    }
}

#[derive(Debug, Default)]
pub struct EventRing {
    buf: VecDeque<ServiceEvent>,
    next_seq: u64,
}

impl EventRing {
    pub fn new() -> Self {
        Self {
            buf: VecDeque::with_capacity(CAP),
            next_seq: 1,
        }
    }

    pub fn push(
        &mut self,
        ts_ms: u64,
        id: ServiceId,
        kind: EventKind,
        state: ServiceState,
        reason_code: ReasonCode,
    ) {
        if self.buf.len() == CAP {
            self.buf.pop_front();
        }
        let seq = self.next_seq.max(1);
        self.next_seq = seq + 1;
        self.buf.push_back(ServiceEvent {
            seq,
            ts_ms,
            id,
            kind,
            state,
            reason_code,
            text: kind_text(kind, state).to_string(),
        });
    }

    /// Events with `seq > since`, oldest first, optionally for one service.
    pub fn query(&self, since: Option<u64>, id: Option<ServiceId>) -> Vec<ServiceEvent> {
        let since = since.unwrap_or(0);
        self.buf
            .iter()
            .filter(|e| e.seq > since && id.is_none_or(|i| i == e.id))
            .cloned()
            .collect()
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push(r: &mut EventRing, id: ServiceId) {
        r.push(
            1,
            id,
            EventKind::StateChanged,
            ServiceState::Healthy,
            ReasonCode::Ok,
        );
    }

    #[test]
    fn keeps_the_last_500_and_numbers_them_monotonically() {
        let mut r = EventRing::new();
        for _ in 0..(CAP + 25) {
            push(&mut r, ServiceId::N8n);
        }
        assert_eq!(r.len(), CAP);
        let all = r.query(None, None);
        assert_eq!(all.first().unwrap().seq, 26);
        assert_eq!(all.last().unwrap().seq, (CAP + 25) as u64);
    }

    #[test]
    fn query_filters_by_since_and_id() {
        let mut r = EventRing::new();
        push(&mut r, ServiceId::N8n);
        push(&mut r, ServiceId::Letta);
        push(&mut r, ServiceId::N8n);
        assert_eq!(r.query(Some(1), None).len(), 2);
        let only = r.query(None, Some(ServiceId::N8n));
        assert_eq!(only.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![1, 3]);
        assert!(r.query(Some(3), None).is_empty());
    }

    #[test]
    fn texts_are_fixed_strings_per_kind_and_state() {
        let mut r = EventRing::new();
        r.push(
            5,
            ServiceId::Postgres,
            EventKind::GaveUp,
            ServiceState::NeedsYou,
            ReasonCode::RestartFailed,
        );
        r.push(
            6,
            ServiceId::Postgres,
            EventKind::StateChanged,
            ServiceState::NeedsYou,
            ReasonCode::BadBind,
        );
        let e = r.query(None, None);
        assert_eq!(e[0].text, "Gave up after 3 restarts");
        assert_eq!(e[1].text, "Needs you");
    }
}

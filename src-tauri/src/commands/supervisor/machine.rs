//! The pure state machine: one `Machine` per service, no clock, no I/O. The caller passes `now_ms`
//! and an `Observation`; the machine returns the one `Action` (if any) the caller may take.
//!
//! Rules (plan section 3):
//! - first observation Up = healthy, and adopted (the app did not start it);
//! - `Slow` counts: 3 of the last 10 observations slow = degraded;
//! - Down twice in a row AND down for the whole grace (45 s LaunchAgent, 20 s docker) = confirmed;
//! - confirmed + act on + auto-restartable: restart attempt 1, 2, 3 with backoff 5 s / 30 s / 2 min; when
//!   the backoff after the 3rd attempt ends and it is still down: `needs_you`, nothing more is done
//!   until a manual reset or the service answers again;
//! - confirmed + act off (or no verb): `needs_you` with the probe's reason, never an action;
//! - `Stopped` is never auto-restarted; a `Separate` service is never acted on;
//! - an `Optional` service that is down is `stopped`, not `needs_you`.

use std::collections::VecDeque;

use serde::Serialize;

use super::registry::{Policy, ServiceDef};

pub const MAX_ATTEMPTS: u8 = 3;
pub const BACKOFF_MS: [u64; 3] = [5_000, 30_000, 120_000];
pub const SLOW_WINDOW: usize = 10;
pub const SLOW_LIMIT: usize = 3;
pub const DOWN_CONFIRM: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Unknown,
    Starting,
    Healthy,
    Degraded,
    Restarting,
    NeedsYou,
    Stopped,
    Separate,
}

/// Fixed reason codes. These (and the fixed event texts) are the only reasons that leave the module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    NotChecked,
    Ok,
    Slow,
    Unreachable,
    Timeout,
    BadStatus,
    ContainerDown,
    ContainerStopped,
    ContainerStarting,
    Unhealthy,
    MissingContainer,
    BadBind,
    DockerUnavailable,
    NotRunning,
    NoStore,
    Restarting,
    RestartFailed,
}

/// What one probe saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observation {
    Up,
    /// Answering, but slowly.
    Slow,
    Starting(ReasonCode),
    /// Present but stopped (clean exit, never started). Never auto-restarted.
    Stopped(ReasonCode),
    Down(ReasonCode),
    /// A condition only the user can fix (bad bind, missing container, no Docker).
    NeedsYou(ReasonCode),
}

impl Observation {
    /// Worst-of ordering for rows made of several checks (Weaviate + Verba).
    pub fn rank(&self) -> u8 {
        match self {
            Observation::Up => 0,
            Observation::Slow => 1,
            Observation::Starting(_) => 2,
            Observation::Stopped(_) => 3,
            Observation::Down(_) => 4,
            Observation::NeedsYou(_) => 5,
        }
    }

    pub fn worst(self, other: Observation) -> Observation {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    /// Restart attempt number n (1..=3).
    Restart(u8),
}

#[derive(Debug, Clone)]
pub struct Machine {
    grace_ms: u64,
    restartable: bool,
    policy: Policy,
    state: ServiceState,
    reason: ReasonCode,
    since_ms: u64,
    checked_ms: u64,
    attempt: u8,
    adopted: bool,
    seen_up: bool,
    window: VecDeque<bool>,
    down_streak: u32,
    down_since: Option<u64>,
    next_attempt_at: u64,
    gave_up: bool,
}

impl Machine {
    pub fn new(def: &ServiceDef, now_ms: u64) -> Self {
        Self {
            grace_ms: def.grace_ms,
            restartable: def.auto_restart && def.policy == Policy::Supervised,
            policy: def.policy,
            state: ServiceState::Unknown,
            reason: ReasonCode::NotChecked,
            since_ms: now_ms,
            checked_ms: 0,
            attempt: 0,
            adopted: false,
            seen_up: false,
            window: VecDeque::with_capacity(SLOW_WINDOW),
            down_streak: 0,
            down_since: None,
            next_attempt_at: 0,
            gave_up: false,
        }
    }

    pub fn state(&self) -> ServiceState {
        self.state
    }
    pub fn reason(&self) -> ReasonCode {
        self.reason
    }
    pub fn since_ms(&self) -> u64 {
        self.since_ms
    }
    pub fn checked_ms(&self) -> u64 {
        self.checked_ms
    }
    pub fn attempt(&self) -> u8 {
        self.attempt
    }
    pub fn adopted(&self) -> bool {
        self.adopted
    }
    pub fn gave_up(&self) -> bool {
        self.gave_up
    }

    /// A manual restart by the user clears the budget (the existing `service_restart` keeps its own count).
    pub fn manual_reset(&mut self) {
        self.attempt = 0;
        self.gave_up = false;
        self.next_attempt_at = 0;
        self.down_streak = 0;
        self.down_since = None;
    }

    fn set(&mut self, now: u64, state: ServiceState, reason: ReasonCode) {
        if self.state != state {
            self.state = state;
            self.since_ms = now;
        }
        self.reason = reason;
    }

    fn clear_down(&mut self) {
        self.down_streak = 0;
        self.down_since = None;
    }

    /// Feed one observation. `act` says whether the caller may take the returned action.
    pub fn observe(&mut self, now_ms: u64, obs: Observation, act: bool) -> Action {
        self.checked_ms = now_ms;
        let separate = self.policy == Policy::Separate;
        match obs {
            Observation::Up | Observation::Slow => {
                if !self.seen_up {
                    self.seen_up = true;
                    self.adopted = true;
                }
                if self.window.len() == SLOW_WINDOW {
                    self.window.pop_front();
                }
                self.window.push_back(obs == Observation::Slow);
                self.clear_down();
                self.attempt = 0;
                self.gave_up = false;
                self.next_attempt_at = 0;
                let slow = self.window.iter().filter(|s| **s).count();
                let (state, reason) = if separate {
                    (ServiceState::Separate, ReasonCode::Ok)
                } else if slow >= SLOW_LIMIT {
                    (ServiceState::Degraded, ReasonCode::Slow)
                } else {
                    (ServiceState::Healthy, ReasonCode::Ok)
                };
                self.set(now_ms, state, reason);
                Action::None
            }
            Observation::Starting(r) => {
                self.clear_down();
                let state = if separate {
                    ServiceState::Separate
                } else {
                    ServiceState::Starting
                };
                self.set(now_ms, state, r);
                Action::None
            }
            Observation::Stopped(r) => {
                self.clear_down();
                let state = if separate {
                    ServiceState::Separate
                } else {
                    ServiceState::Stopped
                };
                self.set(now_ms, state, r);
                Action::None
            }
            Observation::NeedsYou(r) => {
                self.clear_down();
                let state = if separate {
                    ServiceState::Separate
                } else {
                    ServiceState::NeedsYou
                };
                self.set(now_ms, state, r);
                Action::None
            }
            Observation::Down(r) => self.on_down(now_ms, r, act, separate),
        }
    }

    fn on_down(&mut self, now: u64, r: ReasonCode, act: bool, separate: bool) -> Action {
        self.down_streak += 1;
        let since = *self.down_since.get_or_insert(now);
        if separate {
            self.set(now, ServiceState::Separate, r);
            return Action::None;
        }
        let confirmed =
            self.down_streak >= DOWN_CONFIRM && now.saturating_sub(since) >= self.grace_ms;
        if !confirmed {
            let state = if self.seen_up {
                ServiceState::Degraded
            } else {
                ServiceState::Starting
            };
            self.set(now, state, r);
            return Action::None;
        }
        if self.policy == Policy::Optional {
            self.set(now, ServiceState::Stopped, ReasonCode::NotRunning);
            return Action::None;
        }
        if self.gave_up {
            self.set(now, ServiceState::NeedsYou, ReasonCode::RestartFailed);
            return Action::None;
        }
        if !(act && self.restartable) {
            self.set(now, ServiceState::NeedsYou, r);
            return Action::None;
        }
        if self.attempt < MAX_ATTEMPTS {
            self.set(now, ServiceState::Restarting, ReasonCode::Restarting);
            if now >= self.next_attempt_at {
                self.attempt += 1;
                self.next_attempt_at = now + BACKOFF_MS[usize::from(self.attempt) - 1];
                return Action::Restart(self.attempt);
            }
            return Action::None;
        }
        if now >= self.next_attempt_at {
            self.gave_up = true;
            self.set(now, ServiceState::NeedsYou, ReasonCode::RestartFailed);
        } else {
            self.set(now, ServiceState::Restarting, ReasonCode::Restarting);
        }
        Action::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::supervisor::registry::{Registry, ServiceId};

    fn machine(id: ServiceId) -> Machine {
        let r = Registry::builtin();
        Machine::new(r.get(id).unwrap(), 0)
    }

    const DOWN: Observation = Observation::Down(ReasonCode::Unreachable);

    #[test]
    fn first_up_is_healthy_and_adopted() {
        let mut m = machine(ServiceId::N8n);
        assert_eq!(m.state(), ServiceState::Unknown);
        assert_eq!(m.observe(1_000, Observation::Up, true), Action::None);
        assert_eq!(m.state(), ServiceState::Healthy);
        assert!(m.adopted());
        assert_eq!(m.since_ms(), 1_000);
        assert_eq!(m.checked_ms(), 1_000);
    }

    #[test]
    fn three_slow_of_the_last_ten_is_degraded_and_recovers_when_they_age_out() {
        let mut m = machine(ServiceId::N8n);
        let mut t = 0;
        let mut feed = |m: &mut Machine, o| {
            t += 10_000;
            m.observe(t, o, false)
        };
        feed(&mut m, Observation::Up);
        feed(&mut m, Observation::Slow);
        feed(&mut m, Observation::Slow);
        assert_eq!(m.state(), ServiceState::Healthy);
        feed(&mut m, Observation::Slow);
        assert_eq!(m.state(), ServiceState::Degraded);
        assert_eq!(m.reason(), ReasonCode::Slow);
        for _ in 0..9 {
            feed(&mut m, Observation::Up);
        }
        // the oldest slow one just left the window of 10
        assert_eq!(m.state(), ServiceState::Healthy);
    }

    #[test]
    fn one_down_is_degraded_not_an_action() {
        let mut m = machine(ServiceId::N8n);
        m.observe(0, Observation::Up, true);
        assert_eq!(m.observe(10_000, DOWN, true), Action::None);
        assert_eq!(m.state(), ServiceState::Degraded);
        assert_eq!(m.reason(), ReasonCode::Unreachable);
    }

    #[test]
    fn down_before_ever_seen_is_starting_until_confirmed() {
        let mut m = machine(ServiceId::N8n);
        m.observe(0, DOWN, true);
        assert_eq!(m.state(), ServiceState::Starting);
    }

    #[test]
    fn docker_waits_20s_launchd_waits_45s_before_the_first_restart() {
        for (id, grace) in [
            (ServiceId::N8n, 20_000u64),
            (ServiceId::AmaraRouter, 45_000),
        ] {
            let mut m = machine(id);
            m.observe(0, Observation::Up, true);
            assert_eq!(m.observe(1_000, DOWN, true), Action::None);
            assert_eq!(
                m.observe(1_000 + grace - 1, DOWN, true),
                Action::None,
                "{id:?}"
            );
            assert_eq!(
                m.observe(1_000 + grace, DOWN, true),
                Action::Restart(1),
                "{id:?}"
            );
        }
    }

    #[test]
    fn three_attempts_with_backoff_then_needs_you_and_no_more_actions() {
        let mut m = machine(ServiceId::N8n);
        m.observe(0, Observation::Up, true);
        m.observe(1_000, DOWN, true);
        let t1 = 21_000;
        assert_eq!(m.observe(t1, DOWN, true), Action::Restart(1));
        assert_eq!(m.state(), ServiceState::Restarting);
        assert_eq!(m.attempt(), 1);
        // inside the 5 s backoff: waiting, no action
        assert_eq!(m.observe(t1 + 4_999, DOWN, true), Action::None);
        assert_eq!(m.state(), ServiceState::Restarting);
        let t2 = t1 + 5_000;
        assert_eq!(m.observe(t2, DOWN, true), Action::Restart(2));
        assert_eq!(m.observe(t2 + 29_999, DOWN, true), Action::None);
        let t3 = t2 + 30_000;
        assert_eq!(m.observe(t3, DOWN, true), Action::Restart(3));
        assert_eq!(m.observe(t3 + 119_999, DOWN, true), Action::None);
        assert_eq!(m.state(), ServiceState::Restarting);
        // the 4th failure: backoff after attempt 3 is over, still down
        let t4 = t3 + 120_000;
        assert_eq!(m.observe(t4, DOWN, true), Action::None);
        assert_eq!(m.state(), ServiceState::NeedsYou);
        assert_eq!(m.reason(), ReasonCode::RestartFailed);
        assert!(m.gave_up());
        for k in 1..20 {
            assert_eq!(m.observe(t4 + k * 5_000, DOWN, true), Action::None);
            assert_eq!(m.state(), ServiceState::NeedsYou);
        }
    }

    #[test]
    fn manual_reset_gives_a_fresh_budget_and_an_answer_clears_needs_you() {
        let mut m = machine(ServiceId::N8n);
        m.observe(0, Observation::Up, true);
        m.observe(1_000, DOWN, true);
        let mut t = 21_000;
        let mut restarts = 0;
        for _ in 0..40 {
            if m.observe(t, DOWN, true) != Action::None {
                restarts += 1;
            }
            t += 5_000;
        }
        assert_eq!(restarts, 3);
        assert_eq!(m.state(), ServiceState::NeedsYou);
        m.manual_reset();
        // reset also restarts the down streak and grace
        assert_eq!(m.observe(t, DOWN, true), Action::None);
        assert_eq!(m.observe(t + 20_000, DOWN, true), Action::Restart(1));
        // the service answers on its own: healthy, budget back to full
        m.observe(t + 40_000, Observation::Up, true);
        assert_eq!(m.state(), ServiceState::Healthy);
        assert_eq!(m.attempt(), 0);
    }

    #[test]
    fn act_off_confirms_down_as_needs_you_with_the_probe_reason_and_never_acts() {
        let mut m = machine(ServiceId::N8n);
        m.observe(0, Observation::Up, false);
        m.observe(1_000, DOWN, false);
        for k in 0..50u64 {
            assert_eq!(m.observe(21_000 + k * 5_000, DOWN, false), Action::None);
        }
        assert_eq!(m.state(), ServiceState::NeedsYou);
        assert_eq!(m.reason(), ReasonCode::Unreachable);
        assert_eq!(m.attempt(), 0);
    }

    #[test]
    fn a_service_without_a_verb_is_never_acted_on_even_with_act_on() {
        for id in [
            ServiceId::ModelRuntime,
            ServiceId::Docker,
            ServiceId::HealthReader,
        ] {
            let mut m = machine(id);
            m.observe(0, Observation::Up, true);
            m.observe(1_000, DOWN, true);
            assert_eq!(m.observe(100_000, DOWN, true), Action::None, "{id:?}");
            assert_eq!(m.state(), ServiceState::NeedsYou, "{id:?}");
        }
    }

    #[test]
    fn stopped_is_never_auto_restarted() {
        let mut m = machine(ServiceId::N8n);
        m.observe(0, Observation::Up, true);
        for k in 1..30u64 {
            let a = m.observe(
                k * 10_000,
                Observation::Stopped(ReasonCode::ContainerStopped),
                true,
            );
            assert_eq!(a, Action::None);
            assert_eq!(m.state(), ServiceState::Stopped);
        }
    }

    #[test]
    fn separate_is_never_acted_on_whatever_it_does() {
        let mut m = machine(ServiceId::OpenWebui);
        m.observe(0, Observation::Up, true);
        assert_eq!(m.state(), ServiceState::Separate);
        for k in 1..40u64 {
            assert_eq!(m.observe(k * 10_000, DOWN, true), Action::None);
            assert_eq!(m.state(), ServiceState::Separate);
        }
    }

    #[test]
    fn optional_voice_down_is_stopped_not_needs_you() {
        let mut m = machine(ServiceId::AmaraVoice);
        m.observe(0, DOWN, true);
        assert_eq!(m.observe(60_000, DOWN, true), Action::None);
        assert_eq!(m.state(), ServiceState::Stopped);
        assert_eq!(m.reason(), ReasonCode::NotRunning);
    }

    #[test]
    fn needs_you_observations_win_and_never_act() {
        let mut m = machine(ServiceId::CrewService);
        m.observe(0, Observation::NeedsYou(ReasonCode::BadBind), true);
        assert_eq!(m.state(), ServiceState::NeedsYou);
        assert_eq!(m.reason(), ReasonCode::BadBind);
        assert_eq!(
            m.observe(60_000, Observation::NeedsYou(ReasonCode::BadBind), true),
            Action::None
        );
    }

    #[test]
    fn worst_of_orders_the_severities() {
        use Observation::*;
        assert_eq!(Up.worst(Slow), Slow);
        assert_eq!(
            Slow.worst(Starting(ReasonCode::ContainerStarting)),
            Starting(ReasonCode::ContainerStarting)
        );
        assert_eq!(Stopped(ReasonCode::ContainerStopped).worst(DOWN), DOWN);
        assert_eq!(
            DOWN.worst(NeedsYou(ReasonCode::BadBind)),
            NeedsYou(ReasonCode::BadBind)
        );
        assert_eq!(
            NeedsYou(ReasonCode::BadBind).worst(Up),
            NeedsYou(ReasonCode::BadBind)
        );
    }
}

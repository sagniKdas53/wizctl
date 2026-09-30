//! Background bulb command pump shared by the popover widget and the studio.
//!
//! One thread owns every UDP round trip so the UI thread never blocks. Rapid
//! slider traffic is coalesced (only the newest value of a given kind is sent)
//! and the bulb is re-polled periodically with failure backoff.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::bulb::{
    apply_update_limited, brightness_pilot_params, get_pilot_limited, BulbError, PacketRateLimiter,
    PilotResult,
};

/// Idle re-poll cadence while the bulb answers.
const PING_INTERVAL: Duration = Duration::from_secs(10);
/// Escalating re-poll delays after consecutive failures.
const PING_BACKOFF: [Duration; 4] = [
    Duration::from_secs(6),
    Duration::from_secs(12),
    Duration::from_secs(20),
    Duration::from_secs(30),
];
/// Settle delay before confirming a just-issued command against the bulb.
const CONFIRM_DELAY: Duration = Duration::from_millis(1200);

#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    Power(bool),
    Brightness(u8),
    Kelvin(u16),
    Rgb(u8, u8, u8),
    Scene(u32),
    /// An explicitly constructed, validated one-packet delta (used for saved
    /// preset restore so its fields cannot conflict with one another).
    Update(serde_json::Value),
    /// Point the worker at a different bulb and re-poll immediately.
    SetIp(String),
    /// Poll the bulb now.
    Ping,
    /// Sent after the UI consumes a Conflict event; only then may queued input
    /// resume against the freshly displayed state.
    AcknowledgeConflict,
}

impl Cmd {
    /// Commands of the same kind supersede each other while draining the queue.
    fn same_kind(&self, other: &Cmd) -> bool {
        matches!(
            (self, other),
            (Cmd::Brightness(_), Cmd::Brightness(_))
                | (Cmd::Kelvin(_), Cmd::Kelvin(_))
                | (Cmd::Rgb(..), Cmd::Rgb(..))
                | (Cmd::Power(_), Cmd::Power(_))
                | (Cmd::Scene(_), Cmd::Scene(_))
                | (Cmd::Update(_), Cmd::Update(_))
                | (Cmd::Ping, Cmd::Ping)
        )
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    Online {
        pilot: Box<PilotResult>,
        latency_ms: u32,
    },
    Offline(String),
    /// A command was refused because the UI's observed state became stale.
    Conflict {
        pilot: Box<PilotResult>,
        message: String,
    },
    /// The bulb answered but rejected/failed a command; it is not an offline
    /// indication and the UI must preserve that distinction.
    CommandFailed(String),
}

#[derive(Debug, Clone)]
struct QueuedCmd {
    cmd: Cmd,
    expected_state_token: Option<String>,
    generation: u64,
}

#[derive(Debug, Clone)]
struct QueuedEvent {
    event: Event,
    generation: u64,
}

/// Maps only this worker's confirmed state transitions. Queued UI gestures may
/// rebase through it, but every write still performs a fresh prewrite read.
#[derive(Default)]
struct TokenLineage(HashMap<String, String>);

impl TokenLineage {
    fn resolve(&self, expected: Option<&str>) -> Option<String> {
        let mut token = expected?.to_string();
        while let Some(next) = self.0.get(&token) {
            if next == &token {
                break;
            }
            token = next.clone();
        }
        Some(token)
    }

    fn record(&mut self, expected: Option<String>, confirmed: String) {
        if let Some(expected) = expected {
            // Collapse every known self-transition to the latest endpoint.
            // Returning to an earlier color/brightness must not create a cycle
            // such as A -> B -> A and hang the next queued gesture.
            for endpoint in self.0.values_mut() {
                endpoint.clone_from(&confirmed);
            }
            self.0.insert(expected, confirmed.clone());
            self.0.remove(&confirmed);
        }
    }

    fn clear(&mut self) {
        self.0.clear();
    }
}

enum ApplyOutcome {
    Applied(String),
    Conflict,
    Failed,
}

pub struct BulbWorker {
    tx: Sender<QueuedCmd>,
    rx: Receiver<QueuedEvent>,
    observed_token: Arc<Mutex<Option<String>>>,
    generation: Arc<AtomicU64>,
}

impl BulbWorker {
    #[cfg(test)]
    pub(crate) fn inert() -> Self {
        let (tx, _cmd_rx) = channel();
        let (_event_tx, rx) = channel();
        Self {
            tx,
            rx,
            observed_token: Arc::new(Mutex::new(None)),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn new(ip: String, ctx: egui::Context) -> Self {
        let (cmd_tx, cmd_rx) = channel::<QueuedCmd>();
        let (event_tx, event_rx) = channel::<QueuedEvent>();
        let observed_token = Arc::new(Mutex::new(None));
        let generation = Arc::new(AtomicU64::new(0));
        let worker_generation = Arc::clone(&generation);

        thread::spawn(move || {
            let mut ip = ip;
            let mut active_generation = 0;
            let mut failures: usize = 0;
            let mut next_poll = Instant::now();
            let mut limiter = PacketRateLimiter::new();
            let mut conflict_blocked = false;
            let mut lineage = TokenLineage::default();
            let mut last_confirmed_token: Option<String> = None;

            loop {
                let now = Instant::now();
                let wait = next_poll.saturating_duration_since(now);

                match cmd_rx.recv_timeout(wait) {
                    Ok(first) => {
                        // Coalesce a burst of same-kind commands into the newest.
                        let mut pending = first;
                        while let Ok(next) = cmd_rx.try_recv() {
                            if pending.cmd.same_kind(&next.cmd) {
                                pending = next;
                            } else {
                                if pending.generation != worker_generation.load(Ordering::Acquire) {
                                    // A retarget superseded this queued intent.
                                } else if let Cmd::SetIp(new_ip) = &pending.cmd {
                                    ip = new_ip.clone();
                                    active_generation = pending.generation;
                                    conflict_blocked = false;
                                    lineage.clear();
                                    last_confirmed_token = None;
                                } else if matches!(pending.cmd, Cmd::AcknowledgeConflict) {
                                    conflict_blocked = false;
                                } else if conflict_blocked {
                                    let _ = event_tx.send(QueuedEvent {
                                        generation: pending.generation,
                                        event: Event::CommandFailed(
                                            "pending command discarded after a state conflict"
                                                .to_string(),
                                        ),
                                    });
                                } else {
                                    match apply_and_report(
                                        &ip,
                                        &pending,
                                        &mut limiter,
                                        &event_tx,
                                        &mut failures,
                                        pending.generation,
                                        &mut lineage,
                                    ) {
                                        ApplyOutcome::Applied(token) => {
                                            last_confirmed_token = Some(token)
                                        }
                                        ApplyOutcome::Conflict => conflict_blocked = true,
                                        ApplyOutcome::Failed => {}
                                    }
                                }
                                pending = next;
                            }
                        }
                        if pending.generation != worker_generation.load(Ordering::Acquire) {
                            // A retarget superseded this queued intent.
                        } else if let Cmd::SetIp(new_ip) = &pending.cmd {
                            ip = new_ip.clone();
                            active_generation = pending.generation;
                            conflict_blocked = false;
                            lineage.clear();
                            last_confirmed_token = None;
                        } else if matches!(pending.cmd, Cmd::AcknowledgeConflict) {
                            conflict_blocked = false;
                        } else if conflict_blocked {
                            let _ = event_tx.send(QueuedEvent {
                                generation: pending.generation,
                                event: Event::CommandFailed(
                                    "pending command discarded after a state conflict".to_string(),
                                ),
                            });
                        } else {
                            match apply_and_report(
                                &ip,
                                &pending,
                                &mut limiter,
                                &event_tx,
                                &mut failures,
                                pending.generation,
                                &mut lineage,
                            ) {
                                ApplyOutcome::Applied(token) => last_confirmed_token = Some(token),
                                ApplyOutcome::Conflict => conflict_blocked = true,
                                ApplyOutcome::Failed => {}
                            }
                        }

                        let poll_now = matches!(pending.cmd, Cmd::Ping | Cmd::SetIp(_));
                        next_poll = Instant::now()
                            + if poll_now {
                                Duration::ZERO
                            } else {
                                CONFIRM_DELAY
                            };
                        ctx.request_repaint();
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        let started = Instant::now();
                        let poll_generation = active_generation;
                        match get_pilot_limited(&ip, Some(&mut limiter)) {
                            Ok(pilot) => {
                                failures = 0;
                                let token = pilot.state_token();
                                if last_confirmed_token
                                    .as_deref()
                                    .is_some_and(|previous| previous != token)
                                {
                                    lineage.clear();
                                }
                                last_confirmed_token = Some(token);
                                let latency_ms = started.elapsed().as_millis().min(9999) as u32;
                                if event_tx
                                    .send(QueuedEvent {
                                        generation: poll_generation,
                                        event: Event::Online {
                                            pilot: Box::new(pilot),
                                            latency_ms,
                                        },
                                    })
                                    .is_err()
                                {
                                    return;
                                }
                                next_poll = Instant::now() + PING_INTERVAL;
                            }
                            Err(e) => {
                                failures = failures.saturating_add(1);
                                if event_tx
                                    .send(QueuedEvent {
                                        generation: poll_generation,
                                        event: Event::Offline(e.to_string()),
                                    })
                                    .is_err()
                                {
                                    return;
                                }
                                let idx = (failures - 1).min(PING_BACKOFF.len() - 1);
                                next_poll = Instant::now() + PING_BACKOFF[idx];
                            }
                        }
                        ctx.request_repaint();
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        });

        Self {
            tx: cmd_tx,
            rx: event_rx,
            observed_token,
            generation,
        }
    }

    pub fn send(&self, cmd: Cmd) {
        if matches!(cmd, Cmd::SetIp(_)) {
            *self
                .observed_token
                .lock()
                .expect("worker token lock poisoned") = None;
            self.generation.fetch_add(1, Ordering::AcqRel);
        }
        let expected_state_token = self
            .observed_token
            .lock()
            .ok()
            .and_then(|token| token.clone());
        let _ = self.tx.send(QueuedCmd {
            cmd,
            expected_state_token,
            generation: self.generation.load(Ordering::Acquire),
        });
    }

    pub fn try_recv(&self) -> Option<Event> {
        let event = loop {
            let queued = self.rx.try_recv().ok()?;
            if queued.generation == self.generation.load(Ordering::Acquire) {
                break queued.event;
            }
        };
        if let Event::Online { pilot, .. } | Event::Conflict { pilot, .. } = &event {
            *self
                .observed_token
                .lock()
                .expect("worker token lock poisoned") = Some(pilot.state_token());
        }
        if matches!(event, Event::Conflict { .. }) {
            let _ = self.tx.send(QueuedCmd {
                cmd: Cmd::AcknowledgeConflict,
                expected_state_token: None,
                generation: self.generation.load(Ordering::Acquire),
            });
        }
        Some(event)
    }
}

/// Execute one command using the latest observed token. Command failures and
/// optimistic conflicts are reported to the UI rather than being discarded.
fn apply_and_report(
    ip: &str,
    queued: &QueuedCmd,
    limiter: &mut PacketRateLimiter,
    event_tx: &Sender<QueuedEvent>,
    failures: &mut usize,
    generation: u64,
    lineage: &mut TokenLineage,
) -> ApplyOutcome {
    use serde_json::json;
    let params = match &queued.cmd {
        Cmd::Power(on) => json!({ "state": on }),
        Cmd::Brightness(b) => match brightness_pilot_params(*b) {
            Ok(params) => params,
            Err(error) => {
                let _ = event_tx.send(QueuedEvent {
                    generation,
                    event: Event::CommandFailed(error.to_string()),
                });
                return ApplyOutcome::Failed;
            }
        },
        Cmd::Kelvin(k) => json!({ "state": true, "temp": k }),
        Cmd::Rgb(r, g, b) => crate::bulb::rgb_pilot_params(*r, *g, *b),
        Cmd::Scene(sid) => json!({ "state": true, "sceneId": sid }),
        Cmd::Update(params) => params.clone(),
        Cmd::SetIp(_) | Cmd::Ping | Cmd::AcknowledgeConflict => return ApplyOutcome::Failed,
    };
    let expected = lineage.resolve(queued.expected_state_token.as_deref());
    match apply_update_limited(ip, expected.as_deref(), params, Some(limiter)) {
        Ok(pilot) => {
            *failures = 0;
            let confirmed = pilot.state_token();
            lineage.record(expected, confirmed.clone());
            let _ = event_tx.send(QueuedEvent {
                generation,
                event: Event::Online {
                    pilot: Box::new(pilot),
                    latency_ms: 0,
                },
            });
            ApplyOutcome::Applied(confirmed)
        }
        Err(BulbError::StateConflict { current, .. }) => {
            lineage.clear();
            let message = "bulb state changed; refresh before applying the change".to_string();
            let _ = event_tx.send(QueuedEvent {
                generation,
                event: Event::Conflict {
                    pilot: current,
                    message,
                },
            });
            ApplyOutcome::Conflict
        }
        Err(error) => {
            *failures = failures.saturating_add(1);
            let _ = event_tx.send(QueuedEvent {
                generation,
                event: Event::CommandFailed(error.to_string()),
            });
            ApplyOutcome::Failed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn returning_to_an_earlier_state_does_not_cycle_token_lineage() {
        let mut lineage = TokenLineage::default();
        lineage.record(Some("a".into()), "b".into());
        lineage.record(Some("b".into()), "a".into());
        assert_eq!(lineage.resolve(Some("a")).as_deref(), Some("a"));
        assert_eq!(lineage.resolve(Some("b")).as_deref(), Some("a"));
        lineage.record(Some("a".into()), "c".into());
        assert_eq!(lineage.resolve(Some("b")).as_deref(), Some("c"));
    }
    use std::sync::{Mutex, OnceLock};

    fn udp_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn wait_for_event(worker: &BulbWorker) -> Event {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(event) = worker.try_recv() {
                return event;
            }
            assert!(Instant::now() < deadline, "worker event timed out");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn retarget_after_an_unconsumed_conflict_allows_a_fresh_command() {
        use serde_json::json;
        use std::net::UdpSocket;

        let _serial = udp_lock();
        let server = UdpSocket::bind(("127.0.0.1", crate::bulb::WIZ_PORT)).unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let responder = thread::spawn(move || {
            // Initial observation, conflicting pre-read, new pre-read, write,
            // and authoritative post-write readback.
            for index in 0..5 {
                let mut bytes = [0; 2048];
                let (length, sender) = server.recv_from(&mut bytes).unwrap();
                let request: serde_json::Value = serde_json::from_slice(&bytes[..length]).unwrap();
                let result = if index == 3 {
                    assert_eq!(request["method"], "setPilot");
                    assert_eq!(request["params"], json!({"state": true}));
                    json!({"success": true})
                } else {
                    assert_eq!(request["method"], "getPilot");
                    json!({"state": index == 0 || index == 4})
                };
                server
                    .send_to(
                        &serde_json::to_vec(&json!({"result": result})).unwrap(),
                        sender,
                    )
                    .unwrap();
            }
        });
        let worker = BulbWorker::new("127.0.0.1".into(), egui::Context::default());
        let initial = worker.rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let Event::Online { pilot, .. } = initial.event else {
            panic!("initial status")
        };
        *worker.observed_token.lock().unwrap() = Some(pilot.state_token());
        worker.send(Cmd::Power(false));
        let conflict = worker.rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(conflict.event, Event::Conflict { .. }));
        // Do not consume/acknowledge the conflict through try_recv. Changing
        // the target must reset the old target's conflict barrier itself.
        worker.send(Cmd::SetIp("127.0.0.1".into()));
        worker.send(Cmd::Power(true));
        let updated = worker.rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(updated.generation, 1);
        assert!(matches!(updated.event, Event::Online { ref pilot, .. } if pilot.is_on()));
        responder.join().unwrap();
    }

    #[test]
    fn same_kind_only_matches_identical_variants() {
        assert!(Cmd::Brightness(10).same_kind(&Cmd::Brightness(200)));
        assert!(Cmd::Rgb(1, 2, 3).same_kind(&Cmd::Rgb(9, 9, 9)));
        assert!(!Cmd::Brightness(10).same_kind(&Cmd::Kelvin(2700)));
        assert!(!Cmd::Scene(6).same_kind(&Cmd::Power(true)));
    }

    #[test]
    fn send_captures_only_ui_consumed_token_and_retarget_clears_it() {
        let (tx, rx) = channel();
        let (_event_tx, event_rx) = channel::<QueuedEvent>();
        let worker = BulbWorker {
            tx,
            rx: event_rx,
            observed_token: Arc::new(Mutex::new(Some("shown-token".to_string()))),
            generation: Arc::new(AtomicU64::new(0)),
        };
        worker.send(Cmd::Brightness(128));
        assert_eq!(
            rx.recv().unwrap().expected_state_token.as_deref(),
            Some("shown-token")
        );
        worker.send(Cmd::SetIp("127.0.0.2".to_string()));
        assert!(rx.recv().unwrap().expected_state_token.is_none());
    }

    #[test]
    fn stale_target_events_are_discarded_before_they_update_the_token() {
        let (tx, _cmd_rx) = channel();
        let (event_tx, event_rx) = channel();
        let generation = Arc::new(AtomicU64::new(1));
        let worker = BulbWorker {
            tx,
            rx: event_rx,
            observed_token: Arc::new(Mutex::new(None)),
            generation: Arc::clone(&generation),
        };
        event_tx
            .send(QueuedEvent {
                generation: 0,
                event: Event::Online {
                    pilot: Box::new(PilotResult {
                        state: Some(true),
                        ..PilotResult::default()
                    }),
                    latency_ms: 1,
                },
            })
            .unwrap();
        assert!(worker.try_recv().is_none());
        assert!(worker.observed_token.lock().unwrap().is_none());
    }

    #[test]
    fn queued_slider_value_rebases_after_our_confirmed_write() {
        use serde_json::json;
        use std::net::UdpSocket;

        let _serial = udp_lock();
        let server = UdpSocket::bind(("127.0.0.1", crate::bulb::WIZ_PORT)).unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let (first_write_tx, first_write_rx) = channel();
        let responder = thread::spawn(move || {
            for index in 0..7 {
                let mut bytes = [0; 2048];
                let (length, sender) = server.recv_from(&mut bytes).unwrap();
                let request: serde_json::Value = serde_json::from_slice(&bytes[..length]).unwrap();
                let result = match index {
                    0 | 1 => {
                        assert_eq!(request["method"], "getPilot");
                        json!({"state":true,"dimming":50})
                    }
                    2 => {
                        assert_eq!(
                            request,
                            json!({"method":"setPilot","params":{"state":true,"dimming":39}})
                        );
                        first_write_tx.send(()).unwrap();
                        json!({"success":true})
                    }
                    3 | 4 => {
                        assert_eq!(request["method"], "getPilot");
                        json!({"state":true,"dimming":39})
                    }
                    5 => {
                        assert_eq!(
                            request,
                            json!({"method":"setPilot","params":{"state":true,"dimming":78}})
                        );
                        json!({"success":true})
                    }
                    6 => json!({"state":true,"dimming":78}),
                    _ => unreachable!(),
                };
                server
                    .send_to(
                        &serde_json::to_vec(&json!({"result":result})).unwrap(),
                        sender,
                    )
                    .unwrap();
            }
        });
        let worker = BulbWorker::new("127.0.0.1".into(), egui::Context::default());
        let _ = wait_for_event(&worker); // initial displayed state establishes the UI token
        worker.send(Cmd::Brightness(100));
        first_write_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        worker.send(Cmd::Brightness(200));
        let mut final_dimming = None;
        for _ in 0..2 {
            if let Event::Online { pilot, .. } = wait_for_event(&worker) {
                final_dimming = Some(pilot.dimming);
            }
        }
        assert_eq!(final_dimming, Some(Some(78)));
        responder.join().unwrap();
    }

    #[test]
    fn external_change_between_queued_writes_conflicts_without_second_write() {
        use serde_json::json;
        use std::net::UdpSocket;

        let _serial = udp_lock();
        let server = UdpSocket::bind(("127.0.0.1", crate::bulb::WIZ_PORT)).unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let (first_write_tx, first_write_rx) = channel();
        let responder = thread::spawn(move || {
            for index in 0..5 {
                let mut bytes = [0; 2048];
                let (length, sender) = server.recv_from(&mut bytes).unwrap();
                let request: serde_json::Value = serde_json::from_slice(&bytes[..length]).unwrap();
                let result = match index {
                    0 | 1 => {
                        assert_eq!(request["method"], "getPilot");
                        json!({"state":true,"dimming":50})
                    }
                    2 => {
                        assert_eq!(request["method"], "setPilot");
                        first_write_tx.send(()).unwrap();
                        json!({"success":true})
                    }
                    3 => {
                        assert_eq!(request["method"], "getPilot");
                        json!({"state":true,"dimming":39})
                    }
                    4 => {
                        assert_eq!(request["method"], "getPilot");
                        json!({"state":true,"dimming":60})
                    }
                    _ => unreachable!(),
                };
                server
                    .send_to(
                        &serde_json::to_vec(&json!({"result":result})).unwrap(),
                        sender,
                    )
                    .unwrap();
            }
        });
        let worker = BulbWorker::new("127.0.0.1".into(), egui::Context::default());
        let _ = wait_for_event(&worker);
        worker.send(Cmd::Brightness(100));
        first_write_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        worker.send(Cmd::Brightness(200));
        let mut saw_conflict = false;
        for _ in 0..2 {
            if matches!(wait_for_event(&worker), Event::Conflict { .. }) {
                saw_conflict = true;
            }
        }
        assert!(saw_conflict);
        responder.join().unwrap();
    }
}

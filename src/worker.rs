//! Background bulb command pump shared by the popover widget and the studio.
//!
//! One thread owns every UDP round trip so the UI thread never blocks. Rapid
//! slider traffic is coalesced (only the newest value of a given kind is sent)
//! and the bulb is re-polled periodically with failure backoff.

use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::bulb::{get_pilot, send_pilot, PilotResult};

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
    /// Point the worker at a different bulb and re-poll immediately.
    SetIp(String),
    /// Poll the bulb now.
    Ping,
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
}

pub struct BulbWorker {
    tx: Sender<Cmd>,
    rx: Receiver<Event>,
}

impl BulbWorker {
    pub fn new(ip: String, ctx: egui::Context) -> Self {
        let (cmd_tx, cmd_rx) = channel::<Cmd>();
        let (event_tx, event_rx) = channel::<Event>();

        thread::spawn(move || {
            let mut ip = ip;
            let mut failures: usize = 0;
            let mut next_poll = Instant::now();

            loop {
                let now = Instant::now();
                let wait = next_poll.saturating_duration_since(now);

                match cmd_rx.recv_timeout(wait) {
                    Ok(first) => {
                        // Coalesce a burst of same-kind commands into the newest.
                        let mut pending = first;
                        while let Ok(next) = cmd_rx.try_recv() {
                            if pending.same_kind(&next) {
                                pending = next;
                            } else {
                                if let Some(new_ip) = apply(&ip, &pending) {
                                    ip = new_ip;
                                }
                                pending = next;
                            }
                        }
                        if let Some(new_ip) = apply(&ip, &pending) {
                            ip = new_ip;
                        }

                        let poll_now = matches!(pending, Cmd::Ping | Cmd::SetIp(_));
                        next_poll = Instant::now()
                            + if poll_now {
                                Duration::ZERO
                            } else {
                                CONFIRM_DELAY
                            };
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        let started = Instant::now();
                        match get_pilot(&ip) {
                            Ok(pilot) => {
                                failures = 0;
                                let latency_ms = started.elapsed().as_millis().min(9999) as u32;
                                if event_tx
                                    .send(Event::Online {
                                        pilot: Box::new(pilot),
                                        latency_ms,
                                    })
                                    .is_err()
                                {
                                    return;
                                }
                                next_poll = Instant::now() + PING_INTERVAL;
                            }
                            Err(e) => {
                                failures = failures.saturating_add(1);
                                if event_tx.send(Event::Offline(e.to_string())).is_err() {
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
        }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn try_recv(&self) -> Option<Event> {
        self.rx.try_recv().ok()
    }
}

/// Execute one command. Returns a new target IP when the command retargets.
fn apply(ip: &str, cmd: &Cmd) -> Option<String> {
    use serde_json::json;
    match cmd {
        Cmd::Power(on) => {
            let _ = send_pilot(ip, json!({ "state": on }));
            None
        }
        Cmd::Brightness(b) => {
            let dim = ((*b as f64 * 100.0 / 255.0).round() as u8).clamp(10, 100);
            let _ = send_pilot(ip, json!({ "state": true, "dimming": dim }));
            None
        }
        Cmd::Kelvin(k) => {
            let _ = send_pilot(ip, json!({ "state": true, "temp": (*k).clamp(2200, 6500) }));
            None
        }
        Cmd::Rgb(r, g, b) => {
            let _ = send_pilot(ip, crate::bulb::rgb_pilot_params(*r, *g, *b));
            None
        }
        Cmd::Scene(sid) => {
            let _ = send_pilot(ip, json!({ "state": true, "sceneId": sid }));
            None
        }
        Cmd::SetIp(new_ip) => Some(new_ip.clone()),
        Cmd::Ping => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_kind_only_matches_identical_variants() {
        assert!(Cmd::Brightness(10).same_kind(&Cmd::Brightness(200)));
        assert!(Cmd::Rgb(1, 2, 3).same_kind(&Cmd::Rgb(9, 9, 9)));
        assert!(!Cmd::Brightness(10).same_kind(&Cmd::Kelvin(2700)));
        assert!(!Cmd::Scene(6).same_kind(&Cmd::Power(true)));
    }
}

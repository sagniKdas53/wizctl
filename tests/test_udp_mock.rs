use std::net::UdpSocket;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use wizctl::bulb::{
    apply_update, command_brightness, command_wizclick, send_pilot, set_brightness,
    set_temperature, PacketRateLimiter, PilotResult, WIZ_PORT,
};

fn pilot(state: bool, dimming: u8) -> Value {
    let mut fixture: Value =
        serde_json::from_str(include_str!("fixtures/wiz/get-pilot.json")).unwrap();
    fixture["result"]["state"] = json!(state);
    fixture["result"]["dimming"] = json!(dimming);
    fixture["result"].clone()
}

fn protocol_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

/// Bind the real WiZ port so this exercises the production UDP transport and
/// response validation, rather than merely proving two raw sockets can talk.
fn mock_server(
    count: usize,
    handler: impl Fn(usize, Value) -> Value + Send + 'static,
) -> thread::JoinHandle<Vec<Value>> {
    let server = UdpSocket::bind(("127.0.0.1", WIZ_PORT)).expect("bind WiZ mock port");
    server
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    thread::spawn(move || {
        let mut seen = Vec::new();
        for index in 0..count {
            let mut buf = [0; 2048];
            let (amount, sender) = server
                .recv_from(&mut buf)
                .expect("receive production request");
            let request: Value = serde_json::from_slice(&buf[..amount]).expect("JSON request");
            let response = handler(index, request.clone());
            seen.push(request);
            server
                .send_to(&serde_json::to_vec(&response).unwrap(), sender)
                .expect("reply");
        }
        seen
    })
}

#[test]
fn production_apply_update_reads_checks_writes_delta_and_reads_back() {
    let _serial = protocol_lock();
    let initial = pilot(true, 50);
    let initial_pilot: PilotResult = serde_json::from_value(initial.clone()).unwrap();
    let server = mock_server(3, move |index, request| match index {
        0 => {
            assert_eq!(request["method"], "getPilot");
            json!({"result": initial})
        }
        1 => {
            assert_eq!(
                request,
                serde_json::from_str::<Value>(include_str!("fixtures/wiz/set-pilot.json")).unwrap()
            );
            json!({"result": {"success": true}})
        }
        2 => json!({"result": pilot(true, 75)}),
        _ => unreachable!(),
    });
    let updated = apply_update(
        "127.0.0.1",
        Some(&initial_pilot.state_token()),
        json!({"dimming": 75}),
    )
    .unwrap();
    assert_eq!(updated.dimming, Some(75));
    server.join().unwrap();
}

#[test]
fn stale_token_rejects_before_the_write_and_attaches_fresh_pilot() {
    let _serial = protocol_lock();
    let fresh = pilot(true, 25);
    let server = mock_server(1, move |_, request| {
        assert_eq!(request["method"], "getPilot");
        json!({"result": fresh})
    });
    let error = apply_update("127.0.0.1", Some("stale"), json!({"dimming": 75})).unwrap_err();
    assert!(error.to_string().contains("refresh before applying"));
    server.join().unwrap();
}

#[test]
fn state_token_excludes_telemetry_and_source_but_includes_rhythm() {
    let mut first: PilotResult = serde_json::from_value(
        json!({"state":true,"dimming":50,"rssi":-40,"src":7,"schdPsetId":2}),
    )
    .unwrap();
    let token = first.state_token();
    first.rssi = Some(-70);
    assert_eq!(token, first.state_token());
    first.rhythm_id = Some(3);
    assert_ne!(token, first.state_token());
    first.rhythm_id = Some(2);
    first.src = Some(json!("app"));
    assert_eq!(token, first.state_token());
}

#[test]
fn duplicate_on_does_not_write_and_off_rejects_other_fields() {
    let _serial = protocol_lock();
    let initial = pilot(true, 50);
    let server = mock_server(1, move |_, _| json!({"result": initial}));
    let result = apply_update("127.0.0.1", None, json!({"state": true})).unwrap();
    assert!(result.is_on());
    server.join().unwrap();
    assert!(apply_update("127.0.0.1", None, json!({"state": false, "dimming": 50})).is_err());
}

#[test]
fn off_update_is_a_state_only_payload_even_when_already_off() {
    let _serial = protocol_lock();
    let server = mock_server(3, move |index, request| match index {
        0 => {
            assert_eq!(request["method"], "getPilot");
            json!({"result": pilot(false, 50)})
        }
        1 => {
            assert_eq!(
                request,
                json!({"method":"setPilot","params":{"state":false}})
            );
            json!({"result":{"success":true}})
        }
        2 => json!({"result": pilot(false, 50)}),
        _ => unreachable!(),
    });
    assert!(!apply_update("127.0.0.1", None, json!({"state": false}))
        .unwrap()
        .is_on());
    server.join().unwrap();
}

#[test]
fn cli_kelvin_1000_is_sent_exactly_and_read_back() {
    let _serial = protocol_lock();
    let server = mock_server(3, move |index, request| match index {
        0 => json!({"result": pilot(true, 50)}),
        1 => {
            assert_eq!(
                request,
                json!({"method":"setPilot","params":{"state":true,"temp":1000}})
            );
            json!({"result":{"success":true}})
        }
        2 => {
            let mut readback = pilot(true, 50);
            readback["temp"] = json!(1000);
            json!({"result": readback})
        }
        _ => unreachable!(),
    });
    set_temperature("127.0.0.1", 1000).unwrap();
    server.join().unwrap();
}

#[test]
fn brightness_zero_writes_an_explicit_off_delta_and_reads_back() {
    let _serial = protocol_lock();
    let server = mock_server(3, move |index, request| match index {
        0 => json!({"result": pilot(true, 50)}),
        1 => {
            assert_eq!(
                request,
                json!({"method":"setPilot","params":{"state":false}})
            );
            json!({"result":{"success":true}})
        }
        2 => json!({"result": pilot(false, 50)}),
        _ => unreachable!(),
    });
    set_brightness("127.0.0.1", 0).unwrap();
    server.join().unwrap();
}

#[test]
fn below_minimum_cli_brightness_fails_before_network_io() {
    let error = command_brightness("127.0.0.1", "1").unwrap_err();
    assert!(error.to_string().contains("below the WiZ 10% minimum"));
}

#[test]
fn wizclick_error_response_propagates_without_mutation() {
    let _serial = protocol_lock();
    let server = mock_server(1, move |_, request| {
        assert_eq!(request["method"], "getFavs");
        json!({"error":{"code":-1}})
    });
    assert!(command_wizclick("127.0.0.1", Some(1)).is_err());
    assert_eq!(server.join().unwrap().len(), 1);
}

#[test]
fn wizclick_malformed_favorites_propagate_without_mutation() {
    let _serial = protocol_lock();
    let server = mock_server(1, move |_, request| {
        assert_eq!(request["method"], "getFavs");
        json!({"result":{"favs":{"not":"an array"}}})
    });
    assert!(command_wizclick("127.0.0.1", Some(1)).is_err());
    assert_eq!(server.join().unwrap().len(), 1);
}

#[test]
fn wizclick_timeout_propagates_without_mutation() {
    let _serial = protocol_lock();
    let server = UdpSocket::bind(("127.0.0.1", WIZ_PORT)).unwrap();
    let server_thread = thread::spawn(move || {
        let mut bytes = [0; 2048];
        let _ = server.recv_from(&mut bytes).unwrap();
        server
            .set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        assert!(
            server.recv_from(&mut bytes).is_err(),
            "failed getFavs must not send setPilot"
        );
    });
    assert!(command_wizclick("127.0.0.1", Some(1)).is_err());
    server_thread.join().unwrap();
}

#[test]
fn core_rejects_invalid_hardware_values_before_network_io() {
    for params in [
        json!({"state": 1}),
        json!({"temp": 999}),
        json!({"temp": 10001}),
        json!({"dimming": 101}),
        json!({"r": 256}),
        json!({"sceneId": 999_999}),
        json!({"speed": 9}),
        json!({"speed": 201}),
    ] {
        assert!(apply_update("127.0.0.1", None, params).is_err());
    }
}

#[test]
fn set_pilot_requires_a_successful_response() {
    let _serial = protocol_lock();
    let server = mock_server(1, move |_, request| {
        assert_eq!(request["method"], "setPilot");
        json!({"error": {"code": -1}})
    });
    assert!(send_pilot("127.0.0.1", json!({"state": true})).is_err());
    server.join().unwrap();
}

#[test]
fn set_pilot_rejects_a_false_success_acknowledgement() {
    let _serial = protocol_lock();
    let server = mock_server(
        1,
        move |_, _| json!({"method":"setPilot", "result": {"success": false}}),
    );
    assert!(send_pilot("127.0.0.1", json!({"state": true})).is_err());
    server.join().unwrap();
}

#[test]
fn set_pilot_timeout_is_an_error_not_a_silent_success() {
    let _serial = protocol_lock();
    let server = UdpSocket::bind(("127.0.0.1", WIZ_PORT)).unwrap();
    let server_thread = thread::spawn(move || {
        let mut buf = [0; 2048];
        let _ = server.recv_from(&mut buf).unwrap();
    });
    assert!(send_pilot("127.0.0.1", json!({"state": true})).is_err());
    server_thread.join().unwrap();
}

#[test]
fn limiter_spaces_all_worker_packets_at_twelve_per_second() {
    let mut limiter = PacketRateLimiter::new();
    let started = std::time::Instant::now();
    limiter.wait();
    limiter.wait();
    limiter.wait();
    assert!(started.elapsed() >= Duration::from_millis(150));
}

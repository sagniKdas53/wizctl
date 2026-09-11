use std::net::UdpSocket;
use std::thread;
use std::time::Duration;
use serde_json::json;


#[test]
fn test_mock_udp_pilot() {
    let server = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind mock server");
    let server_addr = server.local_addr().expect("Failed to get local addr");

    let server_thread = thread::spawn(move || {
        let mut buf = [0u8; 1024];
        if let Ok((amt, src)) = server.recv_from(&mut buf) {
            let req: serde_json::Value = serde_json::from_slice(&buf[..amt]).unwrap();
            let method = req["method"].as_str().unwrap();

            let resp = if method == "getPilot" {
                json!({
                    "method": "getPilot",
                    "result": {
                        "mac": "cc4085e299f4",
                        "rssi": -42,
                        "state": true,
                        "dimming": 75,
                        "temp": 3500,
                        "sceneId": 6
                    }
                })
            } else {
                json!({
                    "method": method,
                    "result": { "success": true }
                })
            };

            let resp_bytes = serde_json::to_vec(&resp).unwrap();
            let _ = server.send_to(&resp_bytes, src);
        }
    });

    let client = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind client");
    client.set_read_timeout(Some(Duration::from_millis(500))).unwrap();

    let payload = json!({ "method": "getPilot" });
    let req_bytes = serde_json::to_vec(&payload).unwrap();
    client.send_to(&req_bytes, server_addr).unwrap();

    let mut buf = [0u8; 1024];
    let (amt, _) = client.recv_from(&mut buf).unwrap();
    let resp: serde_json::Value = serde_json::from_slice(&buf[..amt]).unwrap();

    assert_eq!(resp["result"]["mac"], "cc4085e299f4");
    assert_eq!(resp["result"]["state"], true);
    assert_eq!(resp["result"]["dimming"], 75);
    assert_eq!(resp["result"]["sceneId"], 6);

    server_thread.join().unwrap();
}

use wizctl::bulb::PilotResult;
use wizctl::genmon::{generate_genmon_xml, render_genmon_xml};
use wizctl::state::State;

#[test]
fn test_genmon_xml_structure() {
    let xml = generate_genmon_xml(Some("192.168.0.102"));
    assert!(xml.contains("<img>"), "Missing <img> tag");
    assert!(xml.contains("</img>"), "Missing </img> tag");
    assert!(xml.contains("<txt>"), "Missing <txt> tag");
    assert!(xml.contains("</txt>"), "Missing </txt> tag");
    assert!(xml.contains("<tool>"), "Missing <tool> tag");
    assert!(xml.contains("</tool>"), "Missing </tool> tag");
    assert!(xml.contains("<click>"), "Missing <click> tag");
    assert!(xml.contains("</click>"), "Missing </click> tag");
    assert!(xml.contains("WiZ Smart Light (192.168.0.102)"));
    assert!(xml.contains("wizctl widget --click"));
}

#[test]
fn live_pilot_overrides_cached_panel_status() {
    let state = State {
        power: false,
        brightness: 10,
        mode: "color".into(),
        ..State::default()
    };
    let pilot = PilotResult {
        state: Some(true),
        dimming: Some(80),
        temp: Some(3500),
        scene_id: Some(0),
        ..PilotResult::default()
    };
    let xml = render_genmon_xml("192.168.0.2", &state, Some(&pilot));
    assert!(xml.contains("80%"));
    assert!(xml.contains("White Temp: 3500K"));
    assert!(xml.contains("panel_bulb_on.png"));

    let offline = render_genmon_xml("192.168.0.2", &state, None);
    assert!(offline.contains("panel_bulb_offline.png"));
    assert!(offline.contains("Bulb unreachable"));
}

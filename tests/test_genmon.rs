use wizctl::genmon::generate_genmon_xml;

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

"""Unit tests for color, brightness, and scene parsers."""

import pytest

from wizctl.colors import COLORS
from wizctl.parsers import parse_brightness, parse_color, parse_scene


class TestColorParser:
    """Test parse_color function."""

    @pytest.mark.parametrize("name,expected", list(COLORS.items()))
    def test_named_colors(self, name, expected):
        assert parse_color(name) == expected

    @pytest.mark.parametrize(
        "name,expected",
        [
            ("WARM-WHITE", COLORS["warmwhite"]),
            ("warm_white", COLORS["warmwhite"]),
            ("warm white", COLORS["warmwhite"]),
            ("cool-white", COLORS["coolwhite"]),
            ("Cool White", COLORS["coolwhite"]),
            ("Day-Light", COLORS["daylight"]),
        ],
    )
    def test_normalized_named_colors(self, name, expected):
        assert parse_color(name) == expected

    @pytest.mark.parametrize(
        "hex_str,expected",
        [
            ("#ff0000", (255, 0, 0)),
            ("ff0000", (255, 0, 0)),
            ("#00ff00", (0, 255, 0)),
            ("#0000ff", (0, 0, 255)),
            ("#ffffff", (255, 255, 255)),
            ("#000000", (0, 0, 0)),
            ("#ff5500", (255, 85, 0)),
            ("#AABBCC", (170, 187, 204)),
        ],
    )
    def test_6_digit_hex(self, hex_str, expected):
        assert parse_color(hex_str) == expected

    @pytest.mark.parametrize(
        "hex_str,expected",
        [
            ("#f00", (255, 0, 0)),
            ("f00", (255, 0, 0)),
            ("#0f0", (0, 255, 0)),
            ("#00f", (0, 0, 255)),
            ("#fff", (255, 255, 255)),
            ("#abc", (170, 187, 204)),
        ],
    )
    def test_3_digit_hex(self, hex_str, expected):
        assert parse_color(hex_str) == expected

    @pytest.mark.parametrize(
        "rgb_str,expected",
        [
            ("255,0,0", (255, 0, 0)),
            ("0,255,0", (0, 255, 0)),
            ("255, 128, 0", (255, 128, 0)),
            ("  0 , 255 , 100  ", (0, 255, 100)),
            ("0,0,0", (0, 0, 0)),
            ("255,255,255", (255, 255, 255)),
        ],
    )
    def test_rgb_triples(self, rgb_str, expected):
        assert parse_color(rgb_str) == expected

    @pytest.mark.parametrize(
        "invalid_val",
        [
            "",
            "   ",
            "notacolor",
            "#12345",
            "#1234567",
            "#gggggg",
            "256,0,0",
            "0,-1,0",
            "255,255",
            "255,255,255,255",
            "abc,def,ghi",
        ],
    )
    def test_invalid_colors(self, invalid_val):
        with pytest.raises(ValueError):
            parse_color(invalid_val)


class TestBrightnessParser:
    """Test parse_brightness function."""

    @pytest.mark.parametrize(
        "val,expected",
        [
            ("0%", 0),
            ("50%", 128),
            ("100%", 255),
            ("20%", 51),
            ("80%", 204),
            ("  50%  ", 128),
        ],
    )
    def test_percentage(self, val, expected):
        assert parse_brightness(val) == expected

    @pytest.mark.parametrize(
        "val,expected",
        [
            ("0", 0),
            ("128", 128),
            ("255", 255),
            ("50", 50),
            ("  200  ", 200),
        ],
    )
    def test_raw_value(self, val, expected):
        assert parse_brightness(val) == expected

    @pytest.mark.parametrize(
        "invalid_val",
        [
            "",
            "   ",
            "-1",
            "256",
            "1000",
            "-5%",
            "101%",
            "200%",
            "fifty%",
            "abc",
        ],
    )
    def test_invalid_brightness(self, invalid_val):
        with pytest.raises(ValueError):
            parse_brightness(invalid_val)


class TestSceneParser:
    """Test parse_scene function."""

    @pytest.mark.parametrize(
        "scene_id,expected_name",
        [
            ("1", "Ocean"),
            ("3", "Sunset"),
            ("6", "Cozy"),
            ("29", "Candlelight"),
            ("36", "Snowy sky"),
            ("40", "Dim-to-warm"),
        ],
    )
    def test_numeric_scenes(self, scene_id, expected_name):
        sid, sname = parse_scene(scene_id)
        assert sid == int(scene_id)
        assert sname == expected_name

    @pytest.mark.parametrize(
        "scene_input,expected_id,expected_name",
        [
            ("ocean", 1, "Ocean"),
            ("Ocean", 1, "Ocean"),
            ("sunset", 3, "Sunset"),
            ("cozy", 6, "Cozy"),
            ("Cozy", 6, "Cozy"),
            ("deep dive", 23, "Deep dive"),
            ("deep-dive", 23, "Deep dive"),
            ("deep_dive", 23, "Deep dive"),
            ("candlelight", 29, "Candlelight"),
            ("warm white", 11, "Warm white"),
            ("warmwhite", 11, "Warm white"),
            ("tv time", 18, "TV time"),
            ("dim-to-warm", 40, "Dim-to-warm"),
        ],
    )
    def test_named_scenes(self, scene_input, expected_id, expected_name):
        sid, sname = parse_scene(scene_input)
        assert sid == expected_id
        assert sname == expected_name

    @pytest.mark.parametrize(
        "invalid_scene",
        [
            "",
            "   ",
            "9999",
            "-1",
            "not_a_real_scene",
            "galaxy",
        ],
    )
    def test_invalid_scenes(self, invalid_scene):
        with pytest.raises(ValueError):
            parse_scene(invalid_scene)


class TestValidateIP:
    """Test validate_ip function."""

    @pytest.mark.parametrize(
        "valid_ip,expected",
        [
            ("192.168.0.102", "192.168.0.102"),
            ("10.0.0.1", "10.0.0.1"),
            ("172.16.0.50", "172.16.0.50"),
            ("  192.168.1.1  ", "192.168.1.1"),
        ],
    )
    def test_valid_ips(self, valid_ip, expected):
        from wizctl.parsers import validate_ip
        assert validate_ip(valid_ip) == expected

    @pytest.mark.parametrize(
        "invalid_ip",
        [
            "",
            "   ",
            "not_an_ip",
            "999.999.999.999",
            "192.168.1.256",
            "255.255.255.255",  # Global broadcast
            "224.0.0.1",        # Multicast
            "::1",              # IPv6
            12345,
            None,
        ],
    )
    def test_invalid_ips(self, invalid_ip):
        from wizctl.parsers import validate_ip
        with pytest.raises(ValueError):
            validate_ip(invalid_ip)


class TestKelvinParser:
    """Test parse_kelvin function."""

    @pytest.mark.parametrize(
        "kelvin_input,expected",
        [
            ("2700", 2700),
            ("2700K", 2700),
            ("2700k", 2700),
            (" 4000 K ", 4000),
            ("6500", 6500),
            (2700, 2700),
            (2200.0, 2200),
            (1000, 1000),
            (10000, 10000),
        ],
    )
    def test_valid_kelvin(self, kelvin_input, expected):
        from wizctl.parsers import parse_kelvin
        assert parse_kelvin(kelvin_input) == expected

    @pytest.mark.parametrize(
        "invalid_kelvin",
        [
            "",
            "   ",
            "abc",
            "999",     # Below 1000K
            "10001",   # Above 10000K
            "-500",
            0,
            float("nan"),
            float("inf"),
            None,
            [],
        ],
    )
    def test_invalid_kelvin(self, invalid_kelvin):
        from wizctl.parsers import parse_kelvin
        with pytest.raises(ValueError):
            parse_kelvin(invalid_kelvin)


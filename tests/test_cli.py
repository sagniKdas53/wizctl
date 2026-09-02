"""Unit tests for wizctl command-line parsing."""

import pytest

from wizctl.cli import create_parser, main


class TestCLIParser:
    """Test CLI argument parsing."""

    def test_parser_default_ip(self):
        parser = create_parser()
        args = parser.parse_args(["status"])
        assert args.command == "status"
        assert args.ip == "192.168.0.102"

    def test_parser_custom_ip(self):
        parser = create_parser()
        args = parser.parse_args(["--ip", "192.168.1.50", "on"])
        assert args.command == "on"
        assert args.ip == "192.168.1.50"

    def test_parser_color_command(self):
        parser = create_parser()
        args = parser.parse_args(["color", "red"])
        assert args.command == "color"
        assert args.value == "red"

    def test_parser_palette_command(self):
        parser = create_parser()
        args = parser.parse_args(
            ["palette", "photo.jpg", "--colors", "8", "--apply", "2", "--plain"]
        )
        assert args.command == "palette"
        assert args.image == "photo.jpg"
        assert args.colors == 8
        assert args.apply == 2
        assert args.plain is True

    def test_parser_brightness_command(self):
        parser = create_parser()
        args = parser.parse_args(["brightness", "50%"])
        assert args.command == "brightness"
        assert args.value == "50%"

    def test_parser_kelvin_command(self):
        parser = create_parser()
        args = parser.parse_args(["kelvin", "4000"])
        assert args.command == "kelvin"
        assert args.value == 4000

    def test_parser_scene_command(self):
        parser = create_parser()
        args = parser.parse_args(["scene", "cozy"])
        assert args.command == "scene"
        assert args.value == "cozy"

    def test_parser_scenes_command(self):
        parser = create_parser()
        args = parser.parse_args(["scenes"])
        assert args.command == "scenes"

    def test_parser_no_command_defaults_to_none(self):
        parser = create_parser()
        args = parser.parse_args([])
        assert args.command is None
        assert args.ip == "192.168.0.102"

    def test_parser_gui_command(self):
        parser = create_parser()
        args = parser.parse_args(["gui"])
        assert args.command == "gui"
        assert args.ip == "192.168.0.102"

    def test_parser_invalid_command_raises(self):
        parser = create_parser()
        with pytest.raises(SystemExit):
            parser.parse_args(["dance"])


class TestCLIMain:
    """Test main function with mock execution."""

    def test_main_scenes(self, capsys):
        exit_code = main(["scenes"])
        assert exit_code == 0
        captured = capsys.readouterr()
        assert "Available WiZ Scenes:" in captured.out
        assert "Cozy" in captured.out
        assert "Sunset" in captured.out

    def test_main_gui_default(self, monkeypatch):
        called_with_ip = []
        monkeypatch.setattr("wizctl.gui.run_gui", lambda target_ip=None: called_with_ip.append(target_ip) or 0)
        exit_code = main([])
        assert exit_code == 0
        assert called_with_ip == ["192.168.0.102"]

    def test_main_gui_subcommand(self, monkeypatch):
        called_with_ip = []
        monkeypatch.setattr("wizctl.gui.run_gui", lambda target_ip=None: called_with_ip.append(target_ip) or 0)
        exit_code = main(["--ip", "192.168.1.99", "gui"])
        assert exit_code == 0
        assert called_with_ip == ["192.168.1.99"]


.PHONY: help venv install install-dev test test-live build binary palette clean

PYTHON ?= python3
VENV_DIR ?= .venv
VENV_BIN = $(VENV_DIR)/bin

help:
	@echo "wizctl build & development targets:"
	@echo "  make venv         - Create Python virtual environment (.venv)"
	@echo "  make install      - Install package in virtual environment"
	@echo "  make install-dev  - Install package with development dependencies"
	@echo "  make test         - Run unit test suite (100+ tests)"
	@echo "  make test-live    - Run live device integration tests"
	@echo "  make build        - Build Python wheel and source distribution"
	@echo "  make binary       - Compile standalone binary executable (dist/wizctl)"
	@echo "  make palette IMAGE=path - Extract image colors and show wizctl commands"
	@echo "  make clean        - Remove build artifacts, pycache, dist files"

venv:
	$(PYTHON) -m venv $(VENV_DIR)
	$(VENV_BIN)/pip install --upgrade pip setuptools wheel

install: venv
	$(VENV_BIN)/pip install -e .

install-dev: venv
	$(VENV_BIN)/pip install -e ".[dev]"

test:
	$(VENV_BIN)/pytest -v

test-live:
	$(VENV_BIN)/pytest -v --live tests/test_integration.py

build:
	$(VENV_BIN)/python -m build

binary:
	$(VENV_BIN)/pyinstaller --onefile --clean --name wizctl --collect-all tkinterdnd2 --paths src src/wizctl/__main__.py

palette:
	@test -n "$(IMAGE)" || (echo "Set IMAGE to an image path, for example: make palette IMAGE=photo.jpg"; exit 2)
	$(VENV_BIN)/python scripts/extract_palette.py "$(IMAGE)"

clean:
	rm -rf build/ dist/ *.egg-info/ .pytest_cache/ *.spec
	find . -type d -name "__pycache__" -exec rm -rf {} +

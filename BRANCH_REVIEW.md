# Branch review (2026-09-22)

This review compares every branch currently published on the GitHub repository
against `main` at `5ff0c53`.

## Executive decision

| Branch | Head | Decision | Reason |
| --- | --- | --- | --- |
| `main` | `5ff0c53` | **Keep** | Small, stable Python CLI/library baseline; 126 tests pass and one live-device test is skipped. |
| `worktree-ci-setup` | `5ff0c53` | **Delete** | It is byte-for-byte identical to `main`; there is nothing to merge. |
| `feat/conflict-safe-state` | `383a2cf` | **Do not merge as-is** | The optimistic-concurrency design is useful, but its own suite has one failing assertion (131 pass, 1 fail, 1 live skip). Port the behavior with corrected tests into whichever implementation becomes primary, then delete the branch. |
| `feat/gui-and-palette` | `9b35f1c` | **Close without merging; retain temporarily only as a reference** | It is a 5,072-line Python/Tk desktop expansion, includes roughly 34 MB of sample photos plus two generated panel archives, and 19 GUI tests cannot run in a headless environment. Its desktop direction overlaps the Rust rewrite. Extract only wanted product ideas, then delete it. |
| `feat/rust-rewrite` | `155e3d8` | **Keep, but require a focused replacement PR before merging** | It is the more coherent desktop direction and all 39 Rust tests pass. However, it is a wholesale replacement that removes the Python package/API, so it should not be merged as an ordinary feature branch. Decide explicitly whether `wizctl` is a Python library or a Rust desktop application first. |

## What the project does today

`wizctl` is a Python 3.9+ command-line tool and importable library for direct,
cloud-free control of a WiZ light on the local network. It uses `pywizlight` and
supports status, on/off/toggle, RGB color, brightness, color temperature, and
WiZ scenes. The target is selected with `--ip`, `WIZ_IP`, or `BULB_IP`, with a
hard-coded fallback of `192.168.0.102`.

The current architecture is deliberately small:

- `src/wizctl/cli.py` parses commands and translates network and validation
  failures into CLI exit codes.
- `src/wizctl/bulb.py` owns asynchronous bulb communication and closes the UDP
  transport after each operation.
- `src/wizctl/parsers.py` validates color, brightness, and scene input.
- `tests/` contains mocked unit tests plus opt-in live hardware tests.

## How the branches relate

```text
8d42dcf  initial Python CLI
  |
5ff0c53  main + CI
  |\
  | 6644512--383a2cf  feat/conflict-safe-state
  |
  2fc13bc  first image-palette experiment
    |\
    | 58df3b0--155e3d8  feat/rust-rewrite
    |
    787f286--...--9b35f1c  feat/gui-and-palette
```

The two desktop branches share only the first palette experiment and then
diverge. They should not both be merged. The conflict-safe branch starts from
`main` independently and overlaps state-reconciliation work later added to the
Python GUI branch.

## Required cleanup before a Rust replacement PR

1. State in the PR title and migration notes that this removes the Python API,
   packaging, and Python tests; this is a breaking 0.2 product decision.
2. Split reusable LAN protocol code from desktop UI code so the Android client
   can share protocol fixtures even if it cannot reuse the UI.
3. Remove or justify desktop-specific XFCE setup from the core delivery path.
4. Add formatter and linter checks (`cargo fmt --check` and `cargo clippy`) to
   CI, in addition to `cargo test --locked`.
5. Test release builds on the actual supported desktop targets and one physical
   WiZ device before merging.
6. Preserve a tag for the final Python release so library users have a clear
   supported endpoint.

If maintaining an importable Python library matters, do **not** merge the Rust
rewrite into `main`. Publish the Rust desktop application from a separate
package/repository and continue the Python line instead.

## Safe branch-removal order

1. Delete `worktree-ci-setup` now.
2. Close `feat/gui-and-palette` after recording any UI requirements that are
   still wanted; do not import its sample photos or generated archives.
3. Fix or reimplement the conflict-safe delta-write behavior, verify it against
   a current `pywizlight`, and then delete `feat/conflict-safe-state`.
4. Keep `feat/rust-rewrite` until the Python-versus-Rust product decision is
   made. Replace it with a clean, reviewable PR rather than merging its current
   history directly.


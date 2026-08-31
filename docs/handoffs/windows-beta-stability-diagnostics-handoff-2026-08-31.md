# Windows Beta Stability and Diagnostics Handoff

Date: 2026-08-31 America/Los_Angeles
Status: complete
Project: `lgraak/resonance-signal`
Repository: GitHub `lgraak/resonance-signal`
Branch: `main`
Starting implementation HEAD: `a97b01f2700dfd6d31e5e4ec19cf4d5b7e02b101`
Final implementation revision: the containing handoff commit; use the final
Executor response and authoritative remote readback for its exact SHA
Standards revision: `46278c6b5d5f1ea687c16fce473967e402fa3c52`
Executor: OpenAI Codex
Model: GPT-5 (Codex)
Effort: not exposed
Previous handoff: `docs/handoffs/portable-standards-adoption-handoff-2026-08-31.md`
Containing handoff commit: not self-recordable; use final Executor response and authoritative remote readback.

## Objective and Outcome

Completed the bounded Windows beta stability and diagnostics milestone. Normal
`resonance-agent.exe` execution is now a native GUI-subsystem tray runtime with
no console or shell dependency. Durable per-user Info/Debug diagnostics,
bounded rotation, panic and shutdown evidence, native diagnostics-folder
opening, and focused failure classifications are implemented and accepted.

The original one-executable requirement reached its explicit stop condition:
Windows selects GUI versus console behavior from the PE header before argument
dispatch. After explicit Observer authorization, the package was split into a
GUI-subsystem `resonance-agent.exe` tray runtime and console-subsystem
`resonance-agent-cli.exe` for the retained diagnostic CLI.

No release, tag, GitHub Release, installer, Linux/PipeWire, consumer, protocol,
remote-network, recovery, watchdog, or automatic-restart work was performed.

## Governing References

- `.project-standards.toml` and the exact adopted Project Bootstrap, Prompt,
  and Handoff Standard v1 documents at
  `46278c6b5d5f1ea687c16fce473967e402fa3c52`.
- `AGENTS.md`.
- `README.md`, `docs/architecture.md`, `docs/api.md`,
  `docs/consumer-protocol.md`, `docs/windows-beta.md`, `docs/roadmap.md`, and
  the applicable accepted ADRs under `docs/decisions/`.
- Previous handoff:
  `docs/handoffs/portable-standards-adoption-handoff-2026-08-31.md`.

## Current Verified State

- Preflight began from clean `main` at
  `a97b01f2700dfd6d31e5e4ec19cf4d5b7e02b101`, matching authoritative
  `origin/main` with ahead/behind `0/0` after fetch.
- The project-standards checkout and authoritative Gitea `origin/main` both
  resolved to `46278c6b5d5f1ea687c16fce473967e402fa3c52` with ahead/behind `0/0`.
- All three adopted standard paths resolved at that exact revision before the
  first project write.
- The accepted package contains exactly `LICENSE.txt`, `README.txt`,
  `resonance-agent.exe`, and `resonance-agent-cli.exe`.
- The accepted archive is
  `resonance-signal-0.1.0-beta.1-windows-x64.zip`, SHA-256
  `F5B3AC3F5928AC6DAB4C4A259DBB060E839CE5681A30864FF95260E2C616C249`.
- Final local runtime state is stopped: no Resonance Signal process or listener
  remains, Start with Windows is disabled, and the persisted log level is Info.

## Work Completed

- Changed `resonance-agent.exe` to a Windows GUI-subsystem tray-only entry
  point. Normal launch and the direct `--tray` startup command create no
  console and do not launch PowerShell, `pwsh`, `cmd`, Windows Terminal, or a
  script.
- Added `resonance-agent-cli.exe` as the Windows console-subsystem entry point
  for `--help`, `--version`, `serve`, and `capture`, preserving synchronous
  console behavior after the authorized split.
- Reconciled the prior capped tray logger into one project-owned diagnostics
  module under `%LOCALAPPDATA%\Resonance Signal`.
- Added Info and Debug selection to the native tray menu. The preference is
  stored in `settings.json`; missing or malformed data safely falls back to
  Info.
- Added current plus two rotated log files under
  `%LOCALAPPDATA%\Resonance Signal\logs`, capped at 1 MiB each, with bounded
  single-line entries and a total content bound of approximately 3 MiB.
- Added startup, version, PID, runtime mode, protocol, tray, listener,
  discovery, session, capture, startup-registration, log-level,
  diagnostics-folder, shutdown, and final process-exit classifications.
- Added a top-level Rust panic hook with version, PID, runtime mode, lifecycle
  state, location, message, and backtrace capture. Tray panic reporting does
  not depend on a console handle.
- Added native `ShellExecuteW` diagnostics-folder opening and retained the
  direct per-user HKCU Run registration contract.
- Updated package construction to build, verify, and ship both executables and
  fail unless their PE subsystem values are GUI `2` and console `3`.
- Updated directly affected Windows beta, architecture, CLI, protocol-command,
  roadmap, package, and top-level documentation.

## Decisions and Constraints

- The permanent two-executable package shape was explicitly authorized only
  after the one-executable stop condition was demonstrated.
- The tray executable owns normal runtime activation; the CLI executable owns
  explicit console diagnostics. This is a packaging/entry-point separation,
  not a provider/consumer or service split.
- Start with Windows remains the exact direct form
  `"<absolute package path>\resonance-agent.exe" --tray` with no shell,
  script, elevation, or indirection.
- Diagnostics use the standard library and existing Serde dependencies; no
  production dependency, framework, external service, telemetry, crash cloud,
  recovery execution, or watchdog was introduced.
- Logs exclude waveform samples, native endpoint identifiers, private identity
  registry contents, credentials, secrets, and per-frame activity.
- Automatic recovery remains disabled and all transport remains numeric
  loopback-only.

## Validation and Evidence

- `cargo fmt --all --check`: passed.
- `cargo check --workspace --all-targets --locked`: passed.
- `cargo test --workspace --locked`: passed. Agent library: 132 passed and 2
  expected host tests ignored; tray binary: 1 passed; API: 6 passed; core: 25
  passed; CLI binary and doc-test targets: no failures.
- `cargo doc --workspace --no-deps --locked`: passed.
- Focused diagnostics tests: 6 passed, covering Info/Debug parsing, missing and
  malformed fallback, persistence, rotation, panic formatting, paths, and
  orderly versus unexpected exit classification.
- Focused startup tests passed, and the ignored real-current-user registration
  round trip was run explicitly and passed; the owned value was absent after
  cleanup.
- Focused tray tests cover checked Info/Debug state. The controlled packaged
  startup failure remained available through the tray, and the bounded source
  path maps its startup error to `Status: Startup failed (see diagnostics
  log)`, never Running.
- The package script passed against the exact accepted archive. Direct PE
  inspection reported subsystem `2` for `resonance-agent.exe` and `3` for
  `resonance-agent-cli.exe`; packaged `--version` returned
  `resonance-agent 0.1.0-beta.1`, and packaged help returned synchronously.
- Native packaged tray launch produced PID 9504 with no child process. It owned
  `127.0.0.1:48480`; `/v1/status` returned ready with protocol 1 and
  `/v1/sources` returned three sources. No PowerShell, `pwsh`, `cmd`, Windows
  Terminal, script, or persistent console was created by Resonance Signal.
- The existing external `Auraline.Host.exe` connected to the packaged tray. The
  repository external consumer separately received live 48 kHz two-channel
  waveform frames and completed with `consumer_cancelled`.
- Info mode recorded application/tray/service startup, version, PID, protocol,
  discovery, the Auraline capture/session, and orderly shutdown evidence.
- The actual tray menu showed Running with Info checked. Switching to Debug
  persisted `{"log_level":"debug"}`; a packaged restart began with
  `log_level=debug` and immediately emitted additional Debug lifecycle records.
  The final tray interaction restored Info and persisted it.
- The actual **Open Diagnostics Folder** item was selected twice. Durable logs
  recorded `diagnostics_folder_opened mechanism=windows_shell`, and the user
  supplied the resulting diagnostics log. No shell process was required.
- Actual Start with Windows enablement produced the real-user HKCU Run value
  `"%LOCALAPPDATA%\Temp\resonance-signal-acceptance-20260831-final3\resonance-agent.exe" --tray`.
  It contained no shell or script and was disabled through the tray; readback
  then confirmed the owned value was absent.
- Controlled isolated rotation retained
  `resonance-signal.log`, `.1.log`, and `.2.log` at 259, 1,048,500, and
  1,048,500 bytes, totaling 2,097,259 bytes. Both rotated seed generations
  remained readable and no file exceeded 1 MiB.
- With packaged CLI service temporarily occupying port 48480, packaged tray
  PID 45588 remained available for the failure menu and durably recorded the
  loopback bind and service startup failures instead of claiming a successful
  service. Tray Exit then ended it orderly; the occupier stopped on its normal
  console signal path.
- Two healthy tray Exit runs terminated their process, removed the listener,
  cleaned up active sessions, and ended with
  `process_exit orderly=true reason=tray_exit`.
- The exact final rebuilt archive was extracted independently and rechecked:
  PE subsystems remained `2` and `3`, packaged CLI version remained correct,
  tray PID 42792 reached ready with no child process, Auraline connected, and
  tray Exit removed the process/listener with an orderly final log record.
- Log inspection after live waveform acceptance found Debug lifecycle records
  but no waveform sample marker or sample payload.
- `git diff --check`: final result is recorded with the containing commit
  validation; checkout line-ending notices are non-errors.

## Unresolved Items

- The previously observed unexplained tray disappearance was not reproduced or
  attributed. This milestone removes shell-lifetime ambiguity and leaves
  durable evidence for a future occurrence; it does not claim the underlying
  observation was a Rust panic.
- Rust panic/unwind handling cannot capture every platform-level termination,
  forced process kill, power loss, or fault that prevents user-mode log I/O.
- The package split changes the documented CLI executable name. It does not
  change protocol or provider behavior, but old commands naming
  `resonance-agent.exe` for CLI modes must use `resonance-agent-cli.exe`.

## Files Changed

- `crates/resonance-agent/src/main.rs`: GUI-subsystem tray entry point.
- `crates/resonance-agent/src/bin/resonance-agent-cli.rs`: console entry point.
- `crates/resonance-agent/src/cli.rs`: retained CLI dispatch and diagnostics.
- `crates/resonance-agent/src/diagnostics.rs`: durable diagnostics, preference,
  rotation, panic, shutdown, and focused tests.
- `crates/resonance-agent/src/tray.rs`: native menu controls, folder opening,
  lifecycle evidence, and failure-state tests.
- `crates/resonance-agent/src/transport.rs`: bounded service, discovery,
  session, capture, backpressure, and shutdown classifications.
- `crates/resonance-agent/src/lib.rs` and
  `crates/resonance-agent/Cargo.toml`: module exposure and removal of the unused
  console API feature.
- `scripts/package-windows-beta.ps1` and
  `packaging/windows-beta/README.txt`: two-executable package and PE checks.
- `README.md`, `docs/architecture.md`, `docs/api.md`,
  `docs/consumer-protocol.md`, `docs/windows-beta.md`, and `docs/roadmap.md`:
  reconciled runtime, CLI, diagnostics, package, and milestone documentation.
- `docs/handoffs/windows-beta-stability-diagnostics-handoff-2026-08-31.md`:
  this milestone handoff.

## Publication and Runtime State

- Worktree implementation and local validation: complete.
- Local commit and authoritative GitHub `origin/main` readback: use the final
  Executor response because this handoff cannot self-record its containing
  commit SHA.
- Deployment, activation, startup registration, Git tag, GitHub Release, and
  public package publication: not performed or left active.
- Local acceptance runtime: stopped; no Resonance Signal process or listener
  remains.
- Per-user diagnostic preference: Info.
- Start with Windows: disabled and owned HKCU Run value absent.

## Safety and Privacy Considerations

- Durable diagnostic messages are single-line, length-bounded, and flushed per
  write; rotation occurs before an incoming entry would exceed the file cap.
- Raw unexpected error messages are omitted from the final process-exit record;
  durable component logs prefer typed or bounded failure context.
- Backend-native endpoint identifiers and private identity registry data remain
  inside `resonance-agent`; the registry was not deleted or reset.
- The native diagnostics-folder action passes only the project-owned local
  diagnostics directory to Windows ShellExecute.

## Do Not Redo

- Do not restore console hiding, PowerShell launchers, script indirection, or a
  GUI-subsystem CLI mode in `resonance-agent.exe`; direct shell capture proved
  those shapes cannot retain reliable synchronous CLI semantics.
- Do not merge the two entry points without new Windows launcher evidence and
  explicit architecture authorization.
- Do not add per-frame/sample logging, telemetry, a crash cloud, retry
  execution, automatic restart, or a watchdog under this milestone.
- Do not delete the private identity registry or current diagnostic history as
  routine cleanup.

## Milestone Learning Candidates

### Windows PE subsystem choice is an entry-point contract

- Evidence: a GUI-subsystem prototype removed normal console allocation, but
  direct PowerShell CLI capture returned before completion and closed stdout
  with Windows error 232. The final separate GUI and console PEs passed native
  package and runtime acceptance.
- Lesson: Windows chooses shell wait and console attachment from the PE header
  before application argument dispatch. One ordinary Rust entry point cannot
  reliably provide both Explorer-native console-free tray launch and unchanged
  synchronous console CLI behavior.
- Project relevance: retain separate tray and CLI entry points unless a future
  Windows launcher design supplies equally strong native and console evidence.

## Next Recommended Action

**Run an extended Windows/Auraline soak test using the new diagnostics to
determine whether Resonance Signal still terminates unexpectedly before
authorizing the public beta release.**

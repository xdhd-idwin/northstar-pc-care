# Northstar PC Care

A Windows 10/11 desktop utility built with Electron, React, TypeScript, and a constrained PowerShell backend. Its catalog has 143 distinct entries across the requested categories: 107 are actionable through reversible registry, service-startup, power-plan, power-setting, or scheduled-task changes and 36 remain review-only with an explanation of why they need hardware-specific, user-selected, or security-sensitive handling. The network page includes DNS provider selection, restore, DNS cache flush, and backed-up per-adapter Nagle controls. It also includes a system dashboard, cleanup preview, and startup Run-key management.

## Safety and scope

System tuning is hardware-, driver-, and Windows-build-dependent. This project deliberately exposes the catalog as a reviewable set of proposed policy/value changes; it does not bundle opaque “debloat” commands, force power plans, disable security, or apply service changes in bulk. Before enabling a tweak, the app asks whether to attempt a Windows restore point, continue with per-setting backups only, or cancel. Windows restore points are best-effort and may be disabled by policy; the app's own exact-value backups are saved either way. Restart-required changes are marked. Review each description before changing a production machine.

Some requested controls (CPU C-states, timer resolution, vendor GPU controls, per-adapter TCP/DNS, global fullscreen behavior) require firmware, driver, adapter, or per-game integration. Their catalog entries are informational and deliberately non-applicable until a device-specific implementation is added. The UI shows this as “Review only”. Do not advertise these as performance guarantees.

## Requirements

- Windows 10 22H2 or Windows 11
- Node.js 22.12 or newer and npm
- PowerShell 5.1 or newer

## Run locally

1. Extract this folder to a writable location.
2. In PowerShell, change to this project folder and run `npm install`.
3. Run `npm run dev` to launch Vite and Electron.
4. The packaged app requests administrator access when it opens. Accept one Windows UAC prompt per app launch; system changes then run in the already elevated app, without another prompt for each tweak. If you cancel, Windows closes the launch; start the app again and accept UAC to use it.
5. For local development builds, run `npm run build`. A distributable installer requires the trusted code-signing certificate described below; `npm run dist` fails instead of creating an unsigned release installer.

`npm run dev` uses Electron's isolated preload bridge and a strict Content Security Policy. For development, launch the terminal as Administrator before starting Electron if you need to apply system changes. Renderer code cannot invoke arbitrary PowerShell or shell commands. The main process validates tweak IDs against the catalog before calling the bundled script.

## Plans and updates

Free permits up to 15 active catalog tweaks and includes the dashboard, DNS tools, startup manager, and app-managed revert controls. Cleaner is marked Pro and hidden behind the plan chooser. The plan chooser displays the requested €2/month and €20 one-time Pro prices, but checkout and paid-license activation are not configured; its purchase buttons clearly report this and do not take payment or unlock Pro. Real sales require checkout links plus a trusted server-side payment/license verification flow.

The packaged app checks for updates before showing its main window using the public `xdhd-idwin/northstar-pc-care` GitHub Releases source in `electron/update-source.json`. The release workflow builds the installer and `latest.yml`, then publishes them as release assets when a version tag is pushed. A downloaded update is installed and the app relaunches; if checking fails or no newer version exists, Northstar opens normally. Release `v0.1.1` is published but unsigned. Install it manually as the first updater-enabled version; subsequent releases require the signing secrets below.

## Windows code signing for releases

Windows SmartScreen may warn that a new app is unrecognized. Northstar's current `v0.1.1` installer is unsigned. To publish signed releases, obtain a trusted Windows Authenticode signing identity from a certificate authority or a supported cloud signing service. A new signature identifies the publisher but may still need download reputation before SmartScreen stops warning.

For a PFX/P12 certificate, add these repository Actions secrets under **Settings → Secrets and variables → Actions**:

- `WIN_CSC_LINK`: the base64-encoded PFX/P12 certificate.
- `WIN_CSC_KEY_PASSWORD`: its private-key password.

Never commit a certificate, private key, or password to the repository. The release workflow requires both secrets and electron-builder verifies that the installer is signed before publishing. Use the same publisher identity for future releases. A self-signed certificate does not establish public SmartScreen trust.

## Project layout

```text
northstar-pc-care/
  package.json
  tsconfig.json
  vite.config.mts
  electron/
    main.ts              # Window, IPC allowlist, PowerShell bridge
    preload.ts           # Narrow contextBridge API
    powershell/
      SystemOps.ps1      # Snapshot/apply/revert/monitor/cleanup operations
  src/
    main.tsx
    App.tsx
    styles.css
    shared/catalog.ts   # 143 categorized entries (107 actionable, 36 review-only)
    shared/types.ts
  README.md
```

## Reversion and data

Per-tweak snapshots and an operation log are stored in `%LOCALAPPDATA%\NorthstarPcCare\`. A registry snapshot records whether each value existed and its original registry kind/data. A service snapshot records its original startup mode and delayed-start value. Startup Run entries disabled through the app are also backed up there. “Restore all” restores these app-managed entries and values; it does not reset Windows to factory defaults. Export `%LOCALAPPDATA%\NorthstarPcCare` before reinstalling Windows or removing the app.

The Startup Apps page manages the current-user and all-users Windows `Run` registry keys and non-Microsoft scheduled tasks with boot or logon triggers. Microsoft Windows tasks are shown as read-only. Startup-folder shortcuts are not enumerated.

Cleanup is preview-first. It proposes files older than seven days under temp folders, DirectX and vendor shader caches, and Chromium/Firefox profile caches while the browsers are closed. It does not touch Prefetch or Windows Update servicing state.

## Extending the engine

Add a catalog item with an explicit registry path, value name, registry kind, and desired data. Mark it `applicable: true` only after confirming the policy on supported Windows versions and testing both apply and revert on a disposable VM. Never accept registry paths, commands, or filesystem paths from renderer input. Add service, scheduled-task, or cleaner operations as separate typed allowlisted handlers with their own snapshot/revert logic; do not turn these into generic command execution.

## Limitations

- Hardware metrics use Windows performance counters and WMI and may be unavailable or slow on some systems.
- GPU utilization requires a Windows GPU Engine counter and can be absent on older drivers.
- Network “ping” is a one-shot latency probe to Cloudflare DNS, not a promise about game-server latency.
- Windows Restore Point creation can fail if System Protection is off, the machine is managed, or a restore point was created too recently.
- Review-only catalog items are documented ideas, not executable tweaks.
- This scaffold is not digitally signed and has not been audited for production deployment.

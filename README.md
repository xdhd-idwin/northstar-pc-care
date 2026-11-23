# Northstar PC Care

Northstar is a Windows 10/11 desktop utility for reviewing and applying reversible system tweaks. The desktop host and command validation layer use Rust and Tauri 2; the interface uses React and TypeScript. The current Windows operations adapter still invokes the audited PowerShell engine in `electron/powershell/SystemOps.ps1`; a complete Rust port of every Windows operation is not finished.

The catalog contains 143 entries. 107 are marked actionable through supported registry, service-startup, power, or task operations; 36 are review-only because they need device-specific work or a safer implementation. “Actionable” is not a performance guarantee, and Windows build, drivers, or device policy may still prevent an operation.

## Requirements and local development

- Windows 10 22H2 or Windows 11
- Node.js 22.12 or newer and pnpm 11.25
- Rust stable, Visual Studio C++ Build Tools, and the Windows 10/11 SDK
- PowerShell 5.1 or newer

Run `pnpm install`, then `pnpm dev` to launch the Tauri app. Run `pnpm build` to create the NSIS installer and signed updater archive. A local updater-signing key is needed for updater artifacts; routine frontend checks can run with `pnpm run build:web`.

The packaged Windows app requests administrator access so system operations can run. The Tauri command layer validates operations against the embedded catalog before calling the Windows adapter; renderer input cannot provide arbitrary commands or registry paths. Changes are backed up per setting. Restore point creation is best-effort and can be declined in favor of app-managed backups.

## Tweaks, profiles, and plans

Free permits up to 15 active tweaks and includes the dashboard, DNS tools, startup manager, and revert controls. Pro unlocks unlimited tweaks, Cleaner, and the rest of the Pro features for €2/month or €20 one-time. Checkout is handled by the Cloudflare Worker in `billing-service/`; Stripe secrets remain in Cloudflare. The Stripe IDs currently in the Worker are sandbox/test values. Configure and verify separate Live products, prices, webhook, and API secret before accepting real customer payments.

Profiles save the selected tweak IDs in the current Windows user's app data. Shared profiles are validated JSON and can be sent to friends. Applying a profile checks plan limits and reports progress; Pro state is not stored in a profile.

Cleaner previews eligible temporary/cache files older than seven days before removal. It rechecks paths, age, and reparse-point status when deleting. DNS settings use family-specific Windows DNS client objects and save original values per adapter so the user can restore them. The network view includes cache flush and backed-up per-adapter Nagle controls.

Pro entitlement is server-side and associated with a stable installation ID under `%LOCALAPPDATA%\NorthstarPcCare`. Updating the app keeps this ID and the profile/browser storage, so a current Pro entitlement remains active after an update. Entitlement is installation-bound; it does not automatically transfer to another Windows account or PC.

## Automatic updates and releases

On launch, Northstar checks the GitHub Releases update feed. When a newer signed version is available, it downloads and installs the update, then relaunches. If offline or the feed is unavailable, the existing app continues to open. Tauri verifies every update with the public key in `src-tauri/tauri.conf.json`; unsigned update artifacts are rejected.

Pushing a `v*` tag builds and publishes the Windows installer, updater signature, and `latest.json` feed. The tag must match `package.json` and `src-tauri/tauri.conf.json`. GitHub Actions needs the `TAURI_SIGNING_PRIVATE_KEY` secret. Its matching private key is kept outside the repository at `%LOCALAPPDATA%\NorthstarPcCare\tauri-updater.key`; keep a secure backup because losing it prevents future in-app updates. Never commit or paste that private key. The public key is safe to include in source.

Updater signatures validate the update package but are separate from Windows Authenticode signing. Without a trusted Authenticode certificate, Windows may show a SmartScreen warning. Avoid publishing a version tag until the build has been reviewed and the release notes reflect the tested changes.

## Windows operations limitations

- Hardware metrics use Windows counters/WMI and may be absent or slow on some systems.
- GPU utilization requires a supported Windows GPU Engine counter.
- Ping is a one-shot latency sample to Cloudflare DNS, not game-server latency.
- Restore point creation can fail if System Protection is off, policy blocks it, or a restore point was created recently.
- Review-only catalog entries are descriptions, not executable tweaks.
- System operations still run through the existing PowerShell adapter. The app has not yet been audited or tested across Windows versions and hardware.

## Project layout

```text
northstar-pc-care/
  src/                    React UI, Tauri bridge, catalog, profiles, update check
  src-tauri/              Rust Tauri shell, command validation, updater configuration
  electron/powershell/    Existing Windows system-operation adapter
  billing-service/        Cloudflare Worker source for checkout and entitlement
  .github/workflows/      Signed Windows release and updater-feed workflow
```

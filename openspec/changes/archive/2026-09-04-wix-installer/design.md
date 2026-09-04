# Design: wix-installer

## Context

The application is a single statically-linked `filecommand.exe` (workspace version `0.1.0`, built by `cargo build --release`) with no registry or service footprint; runtime files (`config.toml`, `usermenu.toml`, `history.json`) are created beside the working directory at run time, not install time. The repo has no packaging assets. The user supplied WiX v4/v5 reference material: Burn bundles wrap an MSI in a `Setup.exe` (`MsiPackage ... Compressed="yes"` embeds it), WixStdBA supplies the bootstrapper UI, and both engine and bundle should be signed for production. Winget natively supports `installerType: burn` with per-scope installer switches.

## Goals / Non-Goals

**Goals:**

- One artifact (`FileCommandSetup.exe`) a user can double-click and winget can drive silently.
- Per-user install with **no elevation prompt at all**; per-machine install elevating exactly once.
- `filecommand` resolvable from a fresh shell after either install; clean PATH removal on uninstall.
- Deterministic identity (UpgradeCode, names, paths) so upgrades and winget manifests stay stable.

**Non-Goals:**

- No code-signing execution (documented, not performed); no winget-pkgs PR; no auto-updater; no per-user ngen/firstrun work; no change to where the app writes its runtime configuration.

## Decisions

### D1: Two single-scope MSIs behind one bundle, per-user default (revised)

Originally this change specified one dual-scope MSI (`ALLUSERS=2` + `MSIINSTALLPERUSER` semantics) to keep a single identity surface. Real-machine testing falsified that approach: under WiX v5, Burn statically treats a `Scope="perUserOrMachine"` MSI as per-machine (build warning WIX1133), launches an elevated engine process on **every** run regardless of the requested scope, and detects the package in the wrong scope — which made the bundle's own uninstall a silent no-op. Dynamic per-user/per-machine bundle scope for a single dual-scope package is an upstream WiX v7 feature proposal, and WiX v7 additionally requires accepting FireGiant's paid Open Source Maintenance Fee EULA, so it is not an option here. The revised decision: **two single-scope MSIs** — `PackagePerUser.wxs` (`Scope="perUser"`) and `PackagePerMachine.wxs` (`Scope="perMachine"`) — chained in one bundle with exactly one planned per run via the `InstallScope` bundle variable. The doubled identity surface (two MSI UpgradeCodes) is the accepted cost of the non-negotiable no-elevation per-user default; the winget-facing identity remains the single bundle. Per-user stays the default because it is the no-elevation path.

### D2: Install paths anchored at `BigHatGroup\FileCommand`

Per-user: `%LOCALAPPDATA%\Programs\BigHatGroup\FileCommand` — the Windows convention for unelevated per-user program installs and always user-writable. Per-machine: `%ProgramFiles%\BigHatGroup\FileCommand`. Both satisfy the required `BigHatGroup\FileCommand` suffix; the manufacturer directory keeps future BigHatGroup tools co-locatable.

### D3: PATH via the MSI Environment table, scope-matched

The MSI's environment authoring appends the install directory to the **user** PATH for per-user installs and the **system** PATH for per-machine installs (each single-scope MSI carries exactly the one Environment component matching its scope — no runtime conditions needed), and removes the entry on uninstall. The Environment table path is chosen over custom actions because Windows Installer handles append/remove/rollback and the `WM_SETTINGCHANGE` broadcast natively — no custom code to get wrong. Already-running shells not seeing the new PATH is inherent Windows behavior, documented in the README rather than fought.

### D4: Burn bundle with WixStdBA, embedded MSI, scope switch for silent installs

`Bundle.wxs` chains both MSIs `Compressed="yes"` (one self-contained `FileCommandSetup.exe`, per the user's reference notes) behind WixStdBA, each gated by an `InstallCondition` on the persisted `InstallScope` bundle variable so exactly one is planned per run. Interactive runs default per-user; the documented command-line control (`FileCommandSetup.exe /quiet InstallScope=perMachine`) selects the per-machine package for silent/automated installs — this is the hook winget's `scope` switches map onto. The bundle embeds both MSIs, roughly doubling the payload for one statically-linked executable — accepted as trivially small against the correctness win. Burn's standard `/quiet`, `/passive`, `/norestart`, `/uninstall` come free with the engine. A custom BA was rejected: nothing here needs UI beyond license-and-install.

### D5: Version and identity flow from the Cargo workspace

The build script reads the workspace `version` (`0.1.0`) and stamps it into MSI ProductVersion and Bundle Version, so releasing remains "bump one number." UpgradeCodes (bundle, per-user MSI, per-machine MSI) are fixed GUIDs generated once and committed; each MSI's `MajorUpgrade` gives in-place same-scope upgrades, and the differing per-scope UpgradeCodes are what make cross-scope installs a documented uninstall-then-reinstall rather than an accidental upgrade. Winget identity: `BigHatGroup.FileCommand`.

### D6: Build is a checked-in script, not CI

`installer/build.ps1`: `cargo build --release` → `wix build Package.wxs` → `wix build Bundle.wxs`, with prerequisites (WiX v4/v5 toolset via `dotnet tool`, .NET SDK) checked up front and documented in `installer/README.md` alongside the production signing steps (sign the Burn engine and the final bundle, per the WiX signing guide the user referenced). CI packaging was rejected for now — the repo has no CI pipeline, and inventing one is a separate concern.

## Risks / Trade-offs

- [App writes runtime config to its working directory, not `%APPDATA%`] → per-user installs are unaffected (install dir is user-writable, and the app writes to CWD anyway); noted as a candidate follow-up proposal for true multi-user per-machine hygiene. The installer itself never creates or owns those files, so uninstall leaves user data alone.
- [Per-scope UpgradeCodes mean a per-user and a per-machine install can coexist if a user installs both scopes] → same-scope upgrades only; changing scope means uninstall + reinstall, stated in the README and the spec, and the coexistence case is called out in the README.
- [Unsigned dev builds trip SmartScreen] → accepted for development; signing is the documented production gate, out of execution scope.
- [Winget manifest can drift from bundle reality] → the template lives in-repo next to the bundle source and the build README's release checklist pairs them.

## Open Questions

- None. Scope model, paths, PATH behavior, and the bootstrapper-for-winget requirement were all specified directly by the user; WiX mechanics follow their supplied reference notes.

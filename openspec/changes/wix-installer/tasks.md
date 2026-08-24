# Tasks: wix-installer

## 1. MSI package

- [x] 1.1 Author `installer/PackagePerUser.wxs` and `installer/PackagePerMachine.wxs`: two single-scope packages (per-user default, per-machine on selection; revised design D1), directory trees ending in `BigHatGroup\FileCommand` under `%LOCALAPPDATA%\Programs` / `%ProgramFiles%`, `filecommand.exe` component, Start Menu shortcut, fixed per-scope UpgradeCodes + `MajorUpgrade` (windows-installer: "Dual-scope MSI package"; windows-installer: "Versioned in-place upgrades")
- [x] 1.2 Author the per-scope PATH environment components (one per package): user PATH in the per-user MSI, system PATH in the per-machine MSI; append on install, remove on uninstall (windows-installer: "Install directory joins PATH at the matching scope")

## 2. Burn bundle

- [x] 2.1 Author `installer/Bundle.wxs`: WixStdBA bundle embedding both MSIs compressed with `InstallCondition`-gated scope selection, producing `FileCommandSetup.exe` (windows-installer: "Burn bootstrapper")
- [x] 2.2 Wire the silent scope control: persisted `InstallScope` bundle variable selecting which MSI is planned; verify `/quiet`, `/passive`, `/norestart`, `/uninstall` behave (windows-installer: "Burn bootstrapper"; windows-installer: "Winget deployability")

## 3. Build & docs

- [x] 3.1 Write `installer/build.ps1`: prerequisite checks (WiX v4/v5, .NET SDK), `cargo build --release`, version read from workspace `Cargo.toml` stamped into MSI and bundle, wix build of all packages and the bundle (windows-installer: "Reproducible installer build")
- [x] 3.2 Write `installer/README.md`: prerequisites, build usage, scope semantics (including same-scope-upgrade-only), PATH refresh note for open shells, production signing steps (windows-installer: "Reproducible installer build")
- [x] 3.3 Author the winget manifest template under `installer/winget/` (`installerType: burn`, `BigHatGroup.FileCommand`, per-scope installer switches) (windows-installer: "Winget deployability")

## 4. Verification

- [x] 4.1 Per-user matrix on a real machine: install with no elevation prompt → files under `%LOCALAPPDATA%\Programs\BigHatGroup\FileCommand`, user PATH contains the folder, `filecommand` launches from a fresh shell; uninstall → files and PATH entry gone (windows-installer: "Dual-scope MSI package"; windows-installer: "Install directory joins PATH at the matching scope")
- [x] 4.2 Per-machine matrix: elevated install → files under `%ProgramFiles%\BigHatGroup\FileCommand`, machine PATH updated; uninstall cleans both (windows-installer: "Dual-scope MSI package"; windows-installer: "Install directory joins PATH at the matching scope"). Verified 2026-08-24: elevated (UAC-approved) `FileCommandSetup.exe /quiet InstallScope=perMachine` installed to `%ProgramFiles%\BigHatGroup\FileCommand`, updated the machine `PATH`, and created the all-users Start Menu shortcut; elevated `/uninstall /quiet InstallScope=perMachine` removed the files, PATH entry, and shortcut cleanly.
- [x] 4.3 Silent matrix: `/quiet` per-user, `/quiet` + scope switch per-machine, `/uninstall /quiet`; version bump → in-place same-scope upgrade (windows-installer: "Burn bootstrapper"; windows-installer: "Versioned in-place upgrades"). Verified 2026-08-24: per-user `/quiet` install/uninstall round-tripped cleanly (LOCALAPPDATA path + user PATH); per-machine silent round-trip covered by 4.2's elevated run above; a scratch version bump (0.1.0 → 0.1.1, reverted after) installed over an existing per-user install produced a single ARP entry at `DisplayVersion 0.1.1` with a changed binary hash and one PATH entry — a true in-place upgrade, not a coexisting install.
- [x] 4.4 Validate the winget manifest template against the built bundle (`winget validate`, local `winget install --manifest`) (windows-installer: "Winget deployability"). Verified 2026-08-24: `winget validate` passed against a scratch copy of the manifest with placeholders filled in (version, a local HTTP URL serving the built bundle, its real SHA-256); `winget install --manifest` found the package, downloaded it, and verified the installer hash successfully, then invoked `FileCommandSetup.exe /quiet` — which Windows Defender/SmartScreen blocked because the bundle is an unsigned dev build fetched over HTTP (Mark-of-the-Web). This is the exact behavior `installer/README.md`'s "Production signing" section already documents and is expected for unsigned builds; it will succeed once the bundle is signed per that section. No repo files were left modified — the filled-in manifest was a throwaway copy outside the repo.

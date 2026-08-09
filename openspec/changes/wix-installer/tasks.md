# Tasks: wix-installer

## 1. MSI package

- [ ] 1.1 Author `installer/Package.wxs`: dual-scope package (per-user default, per-machine on selection), directory trees ending in `BigHatGroup\FileCommand` under `%LOCALAPPDATA%\Programs` / `%ProgramFiles%`, `filecommand.exe` component, Start Menu shortcut, fixed UpgradeCode + `MajorUpgrade` (windows-installer: "Dual-scope MSI package"; windows-installer: "Versioned in-place upgrades")
- [ ] 1.2 Author the scope-conditioned PATH environment components: user PATH for per-user, system PATH for per-machine; append on install, remove on uninstall (windows-installer: "Install directory joins PATH at the matching scope")

## 2. Burn bundle

- [ ] 2.1 Author `installer/Bundle.wxs`: WixStdBA bundle embedding the MSI compressed, producing `FileCommandSetup.exe` (windows-installer: "Burn bootstrapper")
- [ ] 2.2 Wire the silent scope control: bundle variable forwarded to the MSI scope properties; verify `/quiet`, `/passive`, `/norestart`, `/uninstall` behave (windows-installer: "Burn bootstrapper"; windows-installer: "Winget deployability")

## 3. Build & docs

- [ ] 3.1 Write `installer/build.ps1`: prerequisite checks (WiX v4/v5, .NET SDK), `cargo build --release`, version read from workspace `Cargo.toml` stamped into MSI and bundle, wix build of both (windows-installer: "Reproducible installer build")
- [ ] 3.2 Write `installer/README.md`: prerequisites, build usage, scope semantics (including same-scope-upgrade-only), PATH refresh note for open shells, production signing steps (windows-installer: "Reproducible installer build")
- [ ] 3.3 Author the winget manifest template under `installer/winget/` (`installerType: burn`, `BigHatGroup.FileCommand`, per-scope installer switches) (windows-installer: "Winget deployability")

## 4. Verification

- [ ] 4.1 Per-user matrix on a real machine: install with no elevation prompt → files under `%LOCALAPPDATA%\Programs\BigHatGroup\FileCommand`, user PATH contains the folder, `filecommand` launches from a fresh shell; uninstall → files and PATH entry gone (windows-installer: "Dual-scope MSI package"; windows-installer: "Install directory joins PATH at the matching scope")
- [ ] 4.2 Per-machine matrix: elevated install → files under `%ProgramFiles%\BigHatGroup\FileCommand`, machine PATH updated; uninstall cleans both (windows-installer: "Dual-scope MSI package"; windows-installer: "Install directory joins PATH at the matching scope")
- [ ] 4.3 Silent matrix: `/quiet` per-user, `/quiet` + scope switch per-machine, `/uninstall /quiet`; version bump → in-place same-scope upgrade (windows-installer: "Burn bootstrapper"; windows-installer: "Versioned in-place upgrades")
- [ ] 4.4 Validate the winget manifest template against the built bundle (`winget validate`, local `winget install --manifest`) (windows-installer: "Winget deployability")

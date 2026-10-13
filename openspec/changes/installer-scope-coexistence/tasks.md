# Tasks: installer-scope-coexistence

## 1. Detection in the bundle (`installer/Bundle.wxs`)

- [ ] 1.1 Add the `util` namespace and two `util:ProductSearch` elements by UpgradeCode: per-machine `4E088CA3-DA52-42AB-8620-DF5E41B57C7A` to `FileCommandPerMachineState`, and per-user `E4FA877A-C5DE-4BB5-A56E-530939D32718` to `FileCommandPerUserState`, both with `Result="state"` (windows-installer: "Bundle warns when the other installation scope is present"; design D1)
- [ ] 1.2 Add a `SetVariable` (`After="SearchPerUserProduct"`) that sets numeric `FileCommandOtherScopePresent` to `1` when `(InstallScope <> "perMachine" AND FileCommandPerMachineState = 5) OR (InstallScope = "perMachine" AND FileCommandPerUserState = 5)`. Comment it in `Bundle.wxs` with the PATH-precedence reason (design D1, D2)
- [ ] 1.3 Leave the chain, `InstallCondition`s, `InstallScope`, and the bundle `UpgradeCode` unchanged, and do not add a `bal:Condition` (design D2)

## 2. Warning UI (custom WixStdBA theme)

- [ ] 2.1 Copy the built-in `rtfLicense` theme (`RtfTheme.xml`, `RtfTheme.wxl`) from `WixToolset.BootstrapperApplications.wixext` 5.0.2 into `installer/theme/`, with a header comment naming the source version (design D2; Risks)
- [ ] 2.2 Add one `Label` at the top of the `Install` page with `VisibleCondition="FileCommandOtherScopePresent"` and two `<Text Condition="…">` children on `InstallScope`, moving the license `Richedit` down to make room. Add the two scope-specific messages to the `.wxl`, naming the installed scope and the copy that will start from PATH (windows-installer: "Per-user setup over an existing per-machine install", "Per-machine setup over an existing per-user install")
- [ ] 2.3 Point `bal:WixStandardBootstrapperApplication` at the custom `ThemeFile` and `LocalizationFile`, keeping `LicenseFile`, `SuppressOptionsUI="yes"`, and `ShowVersion="yes"`
- [ ] 2.4 On a test VM, verify that `VisibleCondition` shows and hides the control on the install page. If it does not, switch to the D2 fallback (a text control bound to `[FileCommandScopeWarning]`, set by conditioned `SetVariable` elements) (design D2)

## 3. Build script and prerequisites

- [ ] 3.1 `installer/build.ps1`: add `-ext WixToolset.Util.wixext` to the `wix build Bundle.wxs` call, and update the extension-cache comment to name both extensions (proposal: Impact)
- [ ] 3.2 `installer/README.md` § Prerequisites: add `wix extension add WixToolset.Util.wixext/5.0.2` with the same pin rationale as the bootstrapper extension, plus a note that the custom theme must be re-derived when the extension version changes (design: Risks)

## 4. Documentation (`installer/README.md`)

- [ ] 4.1 § Scope semantics, "Same-scope upgrades only": state that the machine PATH precedes the user PATH, so the per-machine copy always wins. Say the bundle now warns when the other scope is present, silent runs only log it, and mixing Chocolatey (per-machine default) with winget (per-user default) is the common way to end up with both (windows-installer: "Bundle warns when the other installation scope is present"; design D3)
- [ ] 4.2 New section "Uninstalling and removing settings" (windows-installer: "Settings removal is documented"; design D4). It should cover:
  - what uninstall removes and keeps;
  - `%APPDATA%\FileCommand` and its three files, with one copy per account for per-machine installs;
  - `Remove-Item -Recurse -Force "$env:APPDATA\FileCommand"` and `rmdir /s /q "%APPDATA%\FileCommand"`;
  - portable mode in the install directory (files plus the marker remain, so the folder remains);
  - `--config-dir`.
- [ ] 4.3 Make sure the profile-directory wording matches `config-profile-directory`'s resolution order (`--config-dir`, then portable marker, then `%APPDATA%\FileCommand`) as it stands when this change lands (proposal: Coordination)

## 5. Verification (Windows test VM)

- [ ] 5.1 `installer/build.ps1` builds all three artifacts cleanly with the Util extension installed. Without it, the bundle step fails with WiX's extension-not-found error (`WIX0144`), which the README prerequisites already explain for the bootstrapper extension (windows-installer "Reproducible installer build")
- [ ] 5.2 Install per-machine, then run the bundle interactively (per-user): the warning appears with the all-users wording, Install completes, and both PATH entries exist. A new shell's `where filecommand` lists the Program Files copy first (windows-installer: "Per-user setup over an existing per-machine install")
- [ ] 5.3 Install per-user, then run `FileCommandSetup.exe InstallScope=perMachine` from an elevated prompt as the same user: the warning appears with the per-account wording, and the install completes (windows-installer: "Per-machine setup over an existing per-user install")
- [ ] 5.4 Clean machine, interactive per-user install: no warning. Same-scope upgrade from version N to N+1 with only that scope present: no warning (windows-installer: "No warning without the other scope")
- [ ] 5.5 Per-machine present, then `FileCommandSetup.exe /quiet /log out.log`: the install completes with no UI, and `out.log` shows `FileCommandPerMachineState` set to 5 (windows-installer: "Silent installs are unaffected")
- [ ] 5.6 Uninstall each scope after running FileCommand once: `%APPDATA%\FileCommand` and its files remain, and a reinstall picks them up (windows-installer: "Uninstall still keeps settings")
- [ ] 5.7 `openspec validate installer-scope-coexistence --strict` clean

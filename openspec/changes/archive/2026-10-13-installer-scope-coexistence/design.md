# Design: installer-scope-coexistence

## Context

Verified on `build/open-proposals-2026-10` (`1bada6f`).

**Packages.**
- `installer/PackagePerUser.wxs`:
  - `Scope="perUser"`, `UpgradeCode="E4FA877A-C5DE-4BB5-A56E-530939D32718"`;
  - installs to `%LocalAppData%\Programs\BigHatGroup\FileCommand`;
  - appends that directory to the **user** PATH (`<Environment … Part="last" Permanent="no" System="no">`);
  - writes HKCU keypath values.
- `installer/PackagePerMachine.wxs`:
  - `Scope="perMachine"`, `UpgradeCode="4E088CA3-DA52-42AB-8620-DF5E41B57C7A"`;
  - installs to `%ProgramFiles%\BigHatGroup\FileCommand`;
  - appends that directory to the **machine** PATH;
  - writes HKLM keypath values.
- Both packages have `MajorUpgrade` for same-scope upgrades, and `RemoveFolder` for the application and Start Menu folders.

**Bundle.** `installer/Bundle.wxs`:
- `UpgradeCode="83A047C2-CBA3-4DA9-96EF-1481CD2D0A6A"`;
- `<Variable Name="InstallScope" Value="perUser" bal:Overridable="yes" Persisted="yes"/>`;
- `bal:WixStandardBootstrapperApplication` with `Theme="rtfLicense"`, `LicenseFile="License.rtf"`, `SuppressOptionsUI="yes"`, `ShowVersion="yes"`;
- a two-package chain gated by `InstallCondition` on `InstallScope`.

The bundle has no searches today. `installer/build.ps1` builds the bundle with `-ext WixToolset.BootstrapperApplications.wixext` only, and the README pins that extension with `wix extension add WixToolset.BootstrapperApplications.wixext/5.0.2`.

**Coexistence today.** `installer/README.md` § "Same-scope upgrades only" says that installing at the other scope "will **coexist** with (not replace) the first install". Nothing detects it at install time.

Windows composes a new process's `PATH` from the machine `Path` value followed by the user `Path` value. So when both directories are present, `filecommand` always resolves to `%ProgramFiles%\BigHatGroup\FileCommand\filecommand.exe`, whichever scope was installed last.

**Settings.** On this branch, `config-profile-directory` is implemented (still in flight on `Spec`). FileCommand resolves its profile directory once at startup:
1. `--config-dir <path>`;
2. otherwise, if a file named `portable` sits beside the executable, the executable's directory;
3. otherwise `%APPDATA%\FileCommand` on Windows.

`installer/README.md` § "Scope semantics" already says that settings (`config.toml`, `history.json`, `usermenu.toml`) live under `%APPDATA%\FileCommand` "regardless of install scope". The windows-installer spec already requires that uninstall leaves those files untouched ("Uninstall leaves user data alone"). Nothing says how to remove them.

**Spec.** `windows-installer` has "Dual-scope MSI package", "Install directory joins PATH at the matching scope", "Burn bootstrapper", "Winget deployability", "Versioned in-place upgrades", and "Reproducible installer build". No in-flight change has a delta on this capability.

## Goals / Non-Goals

**Goals:**

- A user starting an interactive install learns, before clicking Install, that the other scope is already installed, and which copy will run afterwards.
- Unattended installs (winget, Chocolatey, `/quiet`, `/passive`) behave exactly as today, with the detection visible in the log.
- A user who wants to remove FileCommand completely can find out where its settings live and how to delete them.

**Non-Goals:**

- Blocking, or automatically migrating, an install at the other scope. Switching scope stays uninstall-then-reinstall, as documented.
- Changing either MSI, PATH handling, install locations, or upgrade codes.
- A "remove settings" option in the bundle UI or the MSIs (D4).
- Detecting per-user installs that belong to *other* Windows accounts (D5).
- A scope picker in the bundle UI (`SuppressOptionsUI` stays).

## Decisions

### D1: Detect the other scope with `util:ProductSearch` by UpgradeCode

`Bundle.wxs` declares two searches, using the WiX Util extension (`xmlns:util="http://wixtoolset.org/schemas/v4/wxs/util"`):

```xml
<util:ProductSearch Id="SearchPerMachineProduct" UpgradeCode="4E088CA3-DA52-42AB-8620-DF5E41B57C7A"
                    Result="state" Variable="FileCommandPerMachineState" />
<util:ProductSearch Id="SearchPerUserProduct" UpgradeCode="E4FA877A-C5DE-4BB5-A56E-530939D32718"
                    Result="state" Variable="FileCommandPerUserState" />
```

A `state` of `5` (`INSTALLSTATE_DEFAULT`) means installed; a product that is not installed reports `2` (`INSTALLSTATE_ABSENT`).

A `SetVariable` element (`After="SearchPerUserProduct"`) sets a numeric `FileCommandOtherScopePresent` to `1` when "other scope present" holds, and the theme tests that one variable:
- `InstallScope <> "perMachine" AND FileCommandPerMachineState = 5`, or
- `InstallScope = "perMachine" AND FileCommandPerUserState = 5`.

Checked with the WiX 5.0.2 CLI and `WixToolset.Util.wixext` 5.0.2 while reviewing this change: a test bundle with exactly these two searches, the `SetVariable`, and a custom `ThemeFile`/`LocalizationFile` compiles, and its Burn log shows `Setting numeric variable 'FileCommandPerMachineState' to value 2` and `… 'FileCommandPerUserState' to value 5` on a machine with only the per-user MSI installed, followed by the condition's evaluation.

Burn runs searches during Detect, in the bundle's own (non-elevated) process, before any plan or elevation. The per-user search therefore sees the per-user product of the account that launched setup.

Searching by UpgradeCode, not ProductCode, is what makes this version-independent. Each MSI's ProductCode changes with every build, but its UpgradeCode is fixed (windows-installer "Winget deployability" — identity is stable across versions).

*Alternative considered: `util:RegistrySearch` on the packages' keypath values (`HKCU`/`HKLM\Software\BigHatGroup\FileCommand`).* Those values are implementation details of the component layout, and could change without anyone thinking of this search. The UpgradeCode is the packages' declared, stable identity. **Rejected.**

*Alternative considered: searching the PATH for `filecommand.exe`.* That would also flag unrelated copies, such as a `cargo install` binary or a portable copy. Scope coexistence is specifically about the two MSIs. **Rejected.**

### D2: Warn on the install page through a custom WixStdBA theme; never block

The warning is a `Label` control on WixStdBA's `Install` page (the page with the license text, the accept checkbox, and the Install button), visible only when "other scope present" is true.

To get it, the bundle supplies its own theme: `ThemeFile` and `LocalizationFile` on `bal:WixStandardBootstrapperApplication`, both stored in `installer/theme/` (`Theme="rtfLicense"` can stay; the test bundle compiled with both). The theme is copied from the built-in `rtfLicense` theme (`RtfTheme.xml` and `RtfTheme.wxl`, which `WixToolset.BootstrapperApplications.wixext` **5.0.2**, the pinned version, embeds in its wixlib) and extended with that one control. In the built-in theme the license `Richedit` fills the page from `Y="80"` down to the button row, so the label takes the top of that area and the `Richedit` starts below it. The text is in the `.wxl` so it can be translated like the rest of the UI.

The wording depends on which scope is running:
- **Per-user run, per-machine present:** "FileCommand is already installed for all users of this computer. If you also install it for your account, both copies are on PATH and typing `filecommand` starts the all-users copy, not this one. To switch to a per-user install, uninstall the all-users copy first."
- **Per-machine run, per-user present:** "FileCommand is already installed for your account. If you also install it for all users, both copies are on PATH and typing `filecommand` starts this all-users copy; your per-user copy will no longer be used until this one is removed."

The Install button and the rest of the flow are unchanged. Clicking Install proceeds exactly as today.

The label's `VisibleCondition="FileCommandOtherScopePresent"` drives visibility, and two `<Text Condition="…">` children on `InstallScope` pick the wording. Both are patterns the built-in 5.0.2 `rtfLicense` theme already uses: its `Install` page hides the Options button with `VisibleCondition="NOT WixStdBASuppressOptionsUI"`, and its Success and Failure labels choose their text with `<Text Condition="WixBundleAction = …">`. Task 2.4 still checks the result on a test VM; if the label does not show or hide as expected, the fallback is a label whose content is a bundle variable (`[FileCommandScopeWarning]`), set to the warning text by `SetVariable` elements conditioned on the same expressions and empty otherwise.

*Alternative considered: `bal:Condition` with a message.* WixStdBA treats a false `bal:Condition` as a **blocking** launch condition: the message is shown and the install cannot proceed. The requirement is to warn, not refuse. **Rejected.**

*Alternative considered: a custom bootstrapper application (a managed or native BA).* That is a large new component, with its own build, signing, and runtime prerequisites, for one line of text. **Rejected.**

### D3: Silent runs are never interrupted; the log carries the result

With `/quiet` or `/passive` (winget and Chocolatey use `/quiet`), WixStdBA shows no install page, so there is no warning to show. Nothing new can block or prompt. Burn already writes every search result to its log (for example `Setting numeric variable 'FileCommandPerMachineState' to value 5`), and that line is the record for unattended installs.

The Chocolatey package installs per-machine unless `/Scope:user` is passed (`chocolateyInstall.ps1`), while the winget manifest lists the per-user installer first and maps `--scope machine` to `InstallScope=perMachine`. Mixing the two package managers is therefore the most likely way to end up with both scopes. The README's scope section says so.

### D4: Settings removal is documentation, not an installer option

`installer/README.md` gains a section "Uninstalling and removing settings". It says:
- Uninstalling (from Apps & features, or with `FileCommandSetup.exe /uninstall`) removes the program, its shortcut, and its PATH entry, but **keeps** the settings in the profile directory, so a reinstall picks them up.
- The profile directory is `%APPDATA%\FileCommand` (`config.toml`, `history.json`, `usermenu.toml`). For a per-machine install, every Windows account that ran FileCommand has its own copy.
- Removal commands, run as each affected user:
  - PowerShell: `Remove-Item -Recurse -Force "$env:APPDATA\FileCommand"`
  - cmd: `rmdir /s /q "%APPDATA%\FileCommand"`
- **Portable mode.** If a `portable` file was placed beside `filecommand.exe` in the install directory, the settings files live there instead. Uninstall leaves those files, the `portable` marker, and therefore the folder itself, because the MSI removes only what it installed. Delete the folder by hand.
- **`--config-dir`.** Settings live wherever the user pointed it, and the installer never touches that location.

*Alternative considered: a "Remove my settings" checkbox on the uninstall page, or an MSI custom action.*
- For a per-machine install, the uninstalling account cannot (and should not) reach other users' `%APPDATA%`.
- Silent uninstalls through winget or Chocolatey would need a new switch.
- A destructive, user-data-deleting custom action in an installer that has none today is a poor trade for a rare need.
- The existing spec rule "Uninstall … SHALL NOT delete user-created runtime files" would also have to be weakened.

**Rejected** in favor of documentation.

### D5: Scope of detection

The per-user search sees only the per-user product of the account running setup. A per-machine install launched by an administrator in a different account will not see a standard user's per-user install. This is a Windows Installer property: per-user products of other users are not enumerable without their profile. It is accepted and documented, because the realistic coexistence case is one person installing both scopes from their own account.

## Risks / Trade-offs

- [The custom theme copy drifts from the built-in theme when the WiX extension is upgraded.] → The theme is copied from the pinned `5.0.2` extension, and its header names that version. Any extension bump already requires re-testing (the README documents the 7.0.0 incompatibility). The README notes that the theme must be re-derived on a bump.
- [`VisibleCondition` behaves differently than expected on the install page.] → Unlikely, since the built-in theme uses it on the same page; still verified on a test VM before merge (task 2.4), with the variable-text fallback in D2.
- [Running the same bundle version again shows WixStdBA's maintenance page (Repair or Uninstall) instead of the install page, so no warning is shown.] → No install happens in that case, so there is nothing to warn about. The warning targets the install page, where a second-scope install actually begins.
- [The Util extension adds a build prerequisite.] → It is pinned and documented like the bootstrapper extension, and `build.ps1` passes it explicitly.

## Migration Plan

Developers building the installer run `wix extension add WixToolset.Util.wixext/5.0.2` once, as the README prerequisites will say. There is no impact on existing installs. Upgrades behave exactly as before, and an upgrade in a coexistence situation now shows the warning.

## Open Questions

None.

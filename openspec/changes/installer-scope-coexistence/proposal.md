# Change: installer-scope-coexistence

## Why

Audit finding **B-10** (Info). The installer design is sound, with two gaps:

- **The two install scopes coexist silently.**
  - The per-user and per-machine MSIs have different UpgradeCodes on purpose (per-user `E4FA877A-…`, per-machine `4E088CA3-…`), because switching scope is uninstall-then-reinstall (windows-installer "Versioned in-place upgrades").
  - The bundle never checks whether the *other* scope is already installed. A user who installed for all users, and later runs the default per-user setup (or the reverse), ends up with two copies and two PATH entries.
  - Windows builds a process's `PATH` as the machine `PATH` followed by the user `PATH`, so the per-machine copy always runs when the user types `filecommand`. A per-user install or upgrade made after a per-machine one is silently shadowed: the user upgrades, and the old version keeps starting.
  - `installer/README.md` mentions coexistence, but nothing tells the user at install time.
- **Settings survive uninstall, and nothing says how to remove them.** Uninstall deliberately keeps the profile directory (`%APPDATA%\FileCommand`: `config.toml`, `history.json`, `usermenu.toml`; windows-installer "Uninstall leaves user data alone"). Nothing in the installer documentation says where those files are or how to remove them, so a user who wants a clean removal has to guess.

## What Changes

- **Detection.** The Burn bundle checks, before planning, whether the other scope's MSI is installed, by its UpgradeCode. A per-user run looks for the per-machine product, and a per-machine run looks for the per-user product of the current user.
- **Warning, not refusal.** On an interactive run, when the other scope is present, the bundle's install page shows a warning. It names the scope already installed, says that both copies would be on PATH, and says which one will run (always the per-machine copy). The install stays allowed: there is no extra prompt and the Install button is unchanged.
- **Silent runs are unchanged.** `/quiet`, `/passive`, winget, and Chocolatey runs are never blocked or prompted. The detection result is recorded in the Burn log.
- **Documented removal of settings.** `installer/README.md` gains an "Uninstalling and removing settings" section. It names the profile directory, states that uninstall keeps it (for a per-machine install, one per user who ran FileCommand), and gives the commands that remove it. It also covers portable mode (settings beside the executable in the install directory) and `--config-dir`. The section also explains the shadowing rule and how to switch scope cleanly.
- **Build changes.** `installer/build.ps1` passes the WiX Util extension (needed for the product search) to the bundle build. The README prerequisites pin it to the same `5.0.2` as the bootstrapper extension.
- **No change** to the MSIs, the install paths, PATH handling, upgrade behavior, or what uninstall removes. No "remove settings" checkbox is added (design D4).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `windows-installer`: adds the requirement "Bundle warns when the other installation scope is present" (detection by UpgradeCode, interactive warning naming the shadowing copy, install still allowed, silent runs unaffected and logged). It also adds "Settings removal is documented" (profile location, kept on uninstall, per-user and portable cases, removal commands).

## Impact

- **`installer/Bundle.wxs`:**
  - adds the `util` namespace;
  - adds two `util:ProductSearch` elements by UpgradeCode (per-machine `4E088CA3-DA52-42AB-8620-DF5E41B57C7A` and per-user `E4FA877A-C5DE-4BB5-A56E-530939D32718`), each writing a bundle variable;
  - adds a derived "other scope present" condition;
  - points the WixStdBA at a custom theme file and localization.
- **New `installer/theme/` files:** a copy of the built-in `rtfLicense` theme (`RtfTheme.xml` and `RtfTheme.wxl`, embedded in `WixToolset.BootstrapperApplications.wixext` 5.0.2), extended with one warning `Label` on the install page.
- **`installer/build.ps1`:** `-ext WixToolset.Util.wixext` on the bundle build, and a comment noting the pinned version.
- **`installer/README.md`:**
  - prerequisites: `wix extension add WixToolset.Util.wixext/5.0.2`;
  - "Scope semantics": the warning and the PATH-precedence rule;
  - a new "Uninstalling and removing settings" section.
- **Not touched:** `installer/PackagePerUser.wxs`, `installer/PackagePerMachine.wxs`, the winget and Chocolatey manifests, and Rust code.

**Audit findings:** B-10

**Coordination:**
- `config-profile-directory` (in flight on `Spec`; already implemented on `build/open-proposals-2026-10`) fixes the profile directory:
  - `--config-dir <path>` first;
  - then portable mode (a file named `portable` beside the executable means the executable's directory);
  - otherwise `%APPDATA%\FileCommand` on Windows.

  That change also adds the sentence in `installer/README.md` saying settings live under `%APPDATA%\FileCommand`. The new removal section builds on that sentence and uses the same resolution order. If `config-profile-directory` changes its default location before archiving, this section follows it.
- No other audit proposal touches `installer/`.

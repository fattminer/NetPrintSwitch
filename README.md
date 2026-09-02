# NetPrintSwitch alpha

Rust-based Windows tray utility. Associates Wi-Fi SSIDs or Ethernet network names with installed Windows printers, then asks before changing the default printer.
# BUILT USING AI
## Run

Requirements:

- Windows 10 or Windows 11
- Rust stable with the MSVC toolchain
- PowerShell available on `PATH`

```powershell
cargo run
```

The WinUI 3 management window opens on a normal launch. Clicking X asks whether to hide the window in the tray or quit NetPrintSwitch completely. The hidden WinUI process stays alive so reopening from the tray is immediate. Right-click the tray icon to reopen, refresh, or exit.

The Behavior section controls close handling: Ask once per startup remembers the first close choice until restart, while Always minimize to tray and Always quit NetPrintSwitch skip the confirmation.

On launch, the app registers the per-user scheduled task `NetPrintSwitch\\NetworkConnected`. Windows NetworkProfile event 10000 starts the executable with `--check-network`. If the tray app is already running, the short-lived process forwards the command to it through the app's single-instance window message and exits. If the app is not running, it starts hidden in the tray, checks the network, and prompts when appropriate. Multiple saved associations for one detected network trigger a warning and block automatic switching until the duplicate is removed.

The task is queried before registration and is left unchanged when it already points to the current executable. Network, printer, task, and default-printer commands have a 15-second timeout. Temporary check failures show one notification and retry with exponential backoff up to four minutes; printer enumeration is cached briefly between duplicate requests.

Remove the task manually with:

```powershell
schtasks.exe /Delete /TN "NetPrintSwitch\\NetworkConnected" /F
```

## Alpha limitations

- Network and printer discovery uses PowerShell and Windows printer commands.
- Network profiles are filtered to connected interfaces and classified from adapter media type.
- Printer names are used as association identifiers.
- Notifications use the Windows tray balloon plus a native confirmation dialog.
- The supplied `PrintSwitch.svg` is the icon source; its color-preserving `PrintSwitch.ico` derivative is embedded as the executable icon and staged for the WinUI window and tray icon.
- The management window uses Microsoft's Rust `windows-reactor` layer over WinUI 3 controls and Fluent-style defaults. The tray host remains a lightweight native Rust window.
- `windows-reactor` currently resolves Microsoft's Windows App SDK runtime at startup; package a self-contained Windows App SDK runtime before distributing the alpha.
- The alpha relies on Windows NetworkProfile event 10000; a later version can replace the command bridge with direct Network List Manager callbacks.

## License

NetPrintSwitch is licensed under the **GNU Affero General Public License v3.0** with the **Commons Clause License Condition v1.0**; see [LICENSE](./LICENSE).

You may use, modify, and redistribute NetPrintSwitch under those terms. The Commons Clause condition prohibits selling NetPrintSwitch, or a product or service whose value derives substantially from its functionality, including modified, rebranded, or re-skinned versions.

# PRD: NetPrintSwitch

## 1. Introduction / Overview

NetPrintSwitch is a Rust-based Windows tray application built around WinUI 3 that automatically helps users select the correct default printer for the network they are connected to. Users create associations between a network and an installed Windows printer. When the computer connects to a known network, NetPrintSwitch detects the network, checks for an association, and notifies the user that an associated printer was found. The user can then switch the Windows default printer with one action.

The MVP focuses on Wi-Fi SSID and Ethernet network-name detection, installed Windows printers, configurable notification behavior, and tray-based management. Rust provides application logic and Windows API integration; user-facing management screens use WinUI 3 and Windows App SDK conventions through the selected Rust UI approach.

## 2. Goals

- Let users associate one installed Windows printer with each network.
- Detect Wi-Fi SSID and Ethernet network-name changes while the app runs in the tray.
- Notify users when a known network has an associated printer.
- Switch the Windows default printer only after explicit user approval.
- Make association management available without opening a full application window.
- Preserve associations and user settings across restarts.
- Avoid repeated or disruptive prompts through configurable behavior.

## 3. User Stories

### US-001: Run in Windows system tray

**Description:** As a Windows user, I want NetPrintSwitch to run from the system tray so that printer switching happens without keeping a window open.

**Acceptance Criteria:**

- [ ] Application starts and displays a system-tray icon.
- [ ] Closing the management window leaves the application running in the tray.
- [ ] Tray menu includes current network status, association management, settings, and Exit.
- [ ] Exit fully stops monitoring and removes the tray icon.
- [ ] Native Windows UI is manually verified on a supported Windows environment.
- [ ] Tray integration works with the WinUI application lifecycle without leaving orphaned processes or icons.

### US-002: View installed printers

**Description:** As a user, I want to see installed Windows printers so that I can choose a printer for a network association.

**Acceptance Criteria:**

- [ ] Application lists printers installed and available through Windows.
- [ ] Each printer displays a distinguishable name.
- [ ] Current Windows default printer is identified.
- [ ] Empty or unavailable-printer state shows an actionable message.
- [ ] Printer list refreshes after the user requests refresh.

### US-003: Create network-printer association

**Description:** As a user, I want to associate a detected network with an installed printer so that NetPrintSwitch knows which printer to suggest there.

**Acceptance Criteria:**

- [ ] User can view the currently detected Wi-Fi SSID or Ethernet network name.
- [ ] User can select one installed printer and save an association.
- [ ] Association requires a non-empty network identifier and printer selection.
- [ ] Existing association for the same network can be replaced after confirmation.
- [ ] Saved association persists after application restart.
- [ ] Association list shows network identifier and associated printer.
- [ ] Native management UI is manually verified on a supported Windows environment.

### US-004: Edit and delete associations

**Description:** As a user, I want to change or remove associations so that network-printer mappings stay accurate.

**Acceptance Criteria:**

- [ ] User can change printer assigned to an existing network.
- [ ] User can delete an association.
- [ ] Delete action asks for confirmation before removal.
- [ ] Deleted association no longer triggers notifications.
- [ ] Changes persist after application restart.
- [ ] Native management UI is manually verified on a supported Windows environment.

### US-005: Detect network connection changes

**Description:** As a user, I want the app to detect when I connect to a network so that it can check the relevant printer association automatically.

**Acceptance Criteria:**

- [ ] App detects Wi-Fi connection changes using SSID.
- [ ] App detects Ethernet connection changes using the configured Windows network name.
- [ ] App checks associations after a network connection becomes available.
- [ ] App receives a check request from the per-user Windows scheduled task when a network-connected event occurs.
- [ ] App does not trigger a switch prompt for an unknown network.
- [ ] Temporary disconnected or unavailable network states do not crash the app.
- [ ] Duplicate connection events do not produce duplicate prompts.

### US-006: Prompt before switching default printer

**Description:** As a user, I want to approve a suggested printer change so that NetPrintSwitch never changes my default printer unexpectedly.

**Acceptance Criteria:**

- [ ] When a known network has an associated installed printer, a Windows notification appears.
- [ ] Notification identifies the detected network and associated printer.
- [ ] Notification asks whether the user wants to switch the default printer.
- [ ] User can accept the switch.
- [ ] User can dismiss or decline the switch without changing the default printer.
- [ ] Accepted switch sets the associated printer as Windows default printer.
- [ ] Notification reports failure if printer is unavailable or Windows rejects the change.
- [ ] User choice follows configured repeat-prompt behavior.
- [ ] Native notification and tray flow are manually verified on a supported Windows environment.

### US-007: Configure prompt behavior

**Description:** As a user, I want to configure when prompts repeat so that notifications match my workflow.

**Acceptance Criteria:**

- [ ] Settings expose prompt-frequency choices, including prompt once per network connection and prompt every connection.
- [ ] Selected setting persists after restart.
- [ ] Prompt-once behavior allows a new prompt after a later reconnect, according to documented MVP semantics.
- [ ] Settings changes apply to future network events without requiring app restart.
- [ ] Native settings UI is manually verified on a supported Windows environment.

### US-008: Show current network and printer status

**Description:** As a user, I want to see current network and default-printer status from the tray so that I can understand what NetPrintSwitch is doing.

**Acceptance Criteria:**

- [ ] Tray status identifies current Wi-Fi SSID or Ethernet network name when available.
- [ ] Tray status identifies current Windows default printer.
- [ ] Status indicates whether current network has an association.
- [ ] Status updates after network or default-printer changes.

## 4. Functional Requirements

- **FR-1:** The system must run as a Windows background application with a system-tray icon.
- **FR-2:** The system must provide tray actions for status, association management, settings, and Exit.
- **FR-3:** The system must enumerate installed Windows printers through supported Windows printer APIs.
- **FR-4:** The system must identify the active Wi-Fi network by SSID.
- **FR-5:** The system must identify the active Ethernet network by Windows network name.
- **FR-6:** The system must persist network-printer associations locally.
- **FR-7:** Each association must contain one normalized network identifier, network type, and one installed-printer identifier or name.
- **FR-8:** The system must allow users to create, edit, replace, and delete associations.
- **FR-9:** The system must monitor network connectivity changes while running.
- **FR-10:** After a new network connection is detected, the system must look up a matching association.
- **FR-11:** The system must verify that an associated printer is still installed or available before offering a switch.
- **FR-12:** For a matching association, the system must display a notification naming the network and printer.
- **FR-13:** The notification must provide an explicit action to switch the default printer and an action to dismiss or decline.
- **FR-14:** The system must change the Windows default printer only after the user accepts the notification.
- **FR-15:** The system must not prompt when the associated printer is already the Windows default printer.
- **FR-16:** The system must support configurable prompt frequency, at minimum once per network connection and every connection.
- **FR-17:** The system must prevent duplicate prompts caused by repeated operating-system network events.
- **FR-18:** The system must handle unknown networks, disconnected states, missing printers, permission errors, and API failures without crashing.
- **FR-19:** The system must provide clear error feedback when it cannot enumerate printers, identify a network, or change the default printer.
- **FR-20:** The system must allow users to exit the app and stop background monitoring.
- **FR-21:** The system must register a per-user Windows Task Scheduler event task for the NetworkProfile connected event.
- **FR-22:** The scheduled task must launch the executable with a check command when the network-connected event occurs.
- **FR-23:** A check command must forward to an existing tray instance through authenticated single-instance coordination or a same-user Windows IPC mechanism, then exit without creating a second UI instance.
- **FR-24:** If no tray instance exists, a scheduled check must start the app unobtrusively, perform the association check, and keep the app in the tray.
- **FR-25:** The scheduled task must run interactively under the current user so any required prompt appears in the user's desktop session.

## 5. Non-Goals (Out of Scope)

- Automatic printer installation or driver installation.
- Adding printers that are not already installed in Windows.
- Printer discovery or scanning across the network.
- Switching printers without user approval.
- Cloud synchronization of associations.
- Multi-user account synchronization.
- Mobile, macOS, or Linux support.
- Rules based on IP address, gateway, subnet, VPN, location, time, or application.
- Managing printer queues, print jobs, ink levels, or printer maintenance.
- Enterprise policy management or centralized administration.
- Advanced import/export in MVP.

## 6. Design Considerations

- Tray menu should make current state visible without requiring the management window.
- Build management screens with WinUI 3 controls, Windows App SDK patterns, and responsive layouts suitable for compact utility windows.
- Keep tray-process concerns separate from WinUI window concerns so the app can continue monitoring when its window is closed.
- Use WinUI data binding and observable view models for association, printer, network, and settings state.
- Provide light and dark theme support through WinUI theme resources and follow the user's Windows app theme where practical.
- Use WinUI accessibility properties, keyboard navigation, focus management, and high-contrast-compatible colors.
- Notification wording should be concise: “Associated printer network found: [Printer] for [Network]. Switch default printer?”
- Accept and dismiss actions must be visually and semantically distinct.
- Association management should show unknown or unavailable printers clearly.
- Network identifiers may expose sensitive workplace or home-network names; keep data local and avoid logging SSIDs by default.
- Use accessible labels, keyboard navigation, readable contrast, and Windows-native notification conventions.
- Define behavior for network identifier changes, such as an SSID rename, as separate associations unless the user edits the mapping.

## 7. Technical Considerations

- Use WinUI 3 with the Windows App SDK as the desktop UI framework. Confirm selected Windows App SDK version and supported Windows versions before implementation.
- Build application logic in Rust using stable Rust and the MSVC Windows toolchain.
- Use the `windows-rs` crate for generated Rust bindings to Windows and WinRT APIs.
- Use Microsoft's `windows-reactor` as the primary pure-Rust WinUI 3 UI layer for the alpha. Pin the dependency to a known `windows-rs` revision and validate required controls, window lifecycle, tray integration, toast activation, accessibility, and packaging.
- If `windows-reactor` cannot support a required capability, isolate that capability behind a Rust service boundary and document the smallest WinUI/C++ or C# interop layer required.
- Keep Rust domain logic independent of UI bindings. Association matching, persistence, prompt policy, event debouncing, and error handling must compile and test without a UI runtime.
- Use Cargo workspaces or clearly separated modules for domain logic, Windows integrations, UI, and application entry point.
- Use a WinUI 3 `Window` for association and settings management, with a lightweight native Rust tray host that remains active while the WinUI window is closed.
- Keep networking, printer discovery, association persistence, and prompt policy in testable services separate from WinUI views and view models.
- Use Windows App SDK deployment and packaging guidance for the selected MSIX or unpackaged deployment model.
- Register the app identity and notification activation path required for Windows toast actions when using packaged deployment.
- Use Windows networking APIs through `windows-rs` or another Rust-compatible Windows binding to report Wi-Fi SSID and active Ethernet network name.
- Use Windows printer APIs through `windows-rs` or another Rust-compatible Windows binding to enumerate printers and set the default printer; confirm API compatibility during technical design.
- Register a per-user Task Scheduler ONEVENT task for `Microsoft-Windows-NetworkProfile/Operational` network-connected events. Configure it for interactive execution and launch `--check-network`.
- Use a named mutex to enforce one tray instance. A `--check-network` process must forward a Windows message to the existing instance and exit.
- Use Windows toast notifications for user prompts, with WinUI/tray fallback if toast activation is unavailable.
- Store configuration in a per-user local application-data directory using a versioned schema.
- Use a stable printer identifier where Windows provides one; retain display name for user-facing text.
- Network events can arrive rapidly or out of order. Debounce events and re-read active network state before lookup.
- Treat Task Scheduler events as hints, not complete network state. Re-read current network and printer state before matching an association.
- Task installation must be idempotent and update the executable path when the app is moved or upgraded.
- Default-printer changes may be restricted by Windows policy or permissions; surface the exact actionable failure state.
- App should remain idle when no network event occurs and use minimal CPU and memory.
- Include automated tests for association persistence, matching, prompt policy, event debouncing, and failure handling. Use Windows integration tests for printer and network APIs where practical.
- Include Rust-side tests plus WinUI UI tests or focused manual test cases for window lifecycle, theme behavior, accessibility, tray actions, and toast activation.
- Ensure UI-thread-bound WinUI operations are marshaled correctly from network and printer event handlers.

## 8. Success Metrics

- User can create a network-printer association in under 60 seconds.
- On a known network connection, matching association is detected within 10 seconds after stable network availability.
- At least 95% of valid known-network events produce a notification without duplicate prompts in acceptance testing.
- Accepted prompt changes default printer successfully in at least 99% of cases where the associated printer is installed and Windows permits the change.
- Unknown networks never generate an association prompt.
- App crash rate is zero across defined MVP test scenarios.
- App idle resource usage meets an implementation target established during technical design.

## 9. Open Questions

- Which Windows versions are officially supported?
- Should “prompt once per network connection” mean once per physical connection, once per app session, or once until the user changes networks?
- Should a declined switch be remembered for the current connection, or may the app ask again after another network event?
- What should happen when both Wi-Fi and Ethernet are connected simultaneously?
- Should Ethernet use Windows network profile name exactly, or require a user-confirmed name during association setup?
- Should the app launch automatically at Windows sign-in?
- Should associations be encrypted or merely stored with normal per-user filesystem permissions?
- What fallback behavior is required when Windows toast notifications are disabled?
- Which installer and update mechanism will be used?
- Will deployment use MSIX, unpackaged Windows App SDK deployment, or another installer strategy?
- Which Rust toolchain, Windows SDK, Windows App SDK, and `windows-rs` versions will the MVP target?
- Does `windows-reactor` meet all MVP UI, tray, notification, accessibility, and packaging requirements at project start?
- Which tray-icon integration library or Windows API approach will be used with WinUI 3?

# Third-Party Notices

unifi-studio's own source code is licensed under AGPL-3.0 with the
Commons Clause License Condition — see [LICENSE](./LICENSE).

This project depends on the following third-party component, which is
licensed separately and is NOT covered by unifi-studio's license terms:

## Slint

- **Project:** https://slint.dev / https://github.com/slint-ui/slint
- **License used by this project:** Slint Royalty-free License
  (LicenseRef-Slint-Royalty-free-1.1)
  https://github.com/slint-ui/slint/blob/master/LICENSES/LicenseRef-Slint-Royalty-free-1.1.md
- **Why this license and not GPLv3:** Slint's GPLv3 option would require
  the combined work (unifi-studio + Slint) to permit unrestricted resale,
  which conflicts with unifi-studio's Commons Clause condition. The
  Royalty-free license carries no such conflict.

### Obligations fulfilled under the Slint Royalty-free License

1. The `AboutSlint` widget is displayed in this application's About
   screen/dialog, reachable from the top-level menu.
2. The Slint attribution badge is displayed on the public page where
   this application's binaries are made available for download.
3. Copyright notices, warranty disclaimers, and liability limitations
   within Slint's own source code have not been removed or altered.

### Limitations carried over from Slint's license

- Slint may not be redistributed standalone, separate from integration
  into an application.
- Slint may not be used within Embedded Systems under this license tier
  (mobile phones are not considered Embedded Systems for this purpose).

If unifi-studio's use of Slint changes (e.g. a different Slint license
tier is adopted), update this section accordingly.

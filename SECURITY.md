# Security

## Reporting a vulnerability

Email **fredrik@ivar.studio** with a description, the version (tray menu → About) and, if possible, a model file that reproduces the problem. You will get an acknowledgement within 5 working days. Please do not open a public issue for security reports.

## What the app does and does not do

- **Fully offline.** No telemetry, no update checks, no network requests. The content security policy of the viewer page only allows the app's own origin and the local model protocol.
- **Keyboard hook.** A low-level keyboard hook is used to notice the Space key. The callback checks only whether the key is Space and whether an Explorer or desktop file list is in front. No other key is read, stored or sent anywhere, and Space is never swallowed.
- **File access.** The viewer reads files through an internal `model://` protocol that serves only model and texture file types (`.glb`, `.gltf`, `.bin`, images, KTX2), refuses device and verbatim paths and `..` segments, and caps files at 1 GiB. The webview cannot read anything else on disk.
- **Autostart** is opt-in via a question on first run and can be changed in the tray menu. The uninstaller removes the autostart entry.
- **Logs.** `%LOCALAPPDATA%\studio.ivar.gltf-viewer\logs\viewer.log` records errors and the paths of files that failed to open. It is rotated at 1 MB and never leaves the machine.

## Hardening still planned

See the issues labelled `security` and `release` in this repository, in particular code signing (#1) and an update channel (#3).

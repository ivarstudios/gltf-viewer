# IVAR glTF Viewer

A Quick Look-style previewer for `.glb` and `.gltf` files on Windows, built by IVAR Studios AB.

It runs in the system tray. Select a glTF file in Explorer (or on the desktop) and press **Space** to preview it; press **Space** or **Esc** again to close. It works fully offline.

## Install

1. Download `IVAR glTF Viewer_<version>_x64-setup.exe` from the [latest release](https://github.com/ivarstudios/gltf-viewer/releases/latest) (or from ivar.studio). `SHA256SUMS.txt` next to it holds the checksum.
2. Run it. It installs per user, needs no admin rights, and takes a few seconds.
3. On first start it asks whether to start with Windows. Say yes if you want Space to work right after login; you can change this any time from the tray menu.

**Windows SmartScreen** will show "Windows protected your PC" until the installer is code-signed (tracked in [#1](https://github.com/ivarstudios/gltf-viewer/issues/1)). Click *More info* → *Run anyway*. Compare the checksum first if you are careful.

**Requirements:** Windows 10 (1809 or later) or Windows 11, 64-bit. Windows 11 ships the WebView2 runtime the viewer renders with; on Windows 10 the installer downloads it if it is missing, which needs an internet connection during install only.

To make it the default for double-click, right-click a `.glb` → *Open with* → *Choose another app* → **IVAR glTF Viewer** → *Always*.

**Uninstall** from *Settings → Apps*. The uninstaller also removes the "start with Windows" entry.

### Known conflicts

Other Quick Look clones that bind Space (QuickLook, Seer) open at the same time. Disable Space in one of them.

## Features

- Three.js viewer modelled on [three-gltf-viewer](https://github.com/donmccurdy/three-gltf-viewer) by Don McCurdy (MIT)
- Draco, KTX2/Basis and Meshopt compressed files (decoders bundled, no network needed)
- `.gltf` files load their `.bin` and texture files from disk, including relative paths such as `../textures/`
- Neutral studio lighting plus 4 bundled HDR environments ([Poly Haven](https://polyhaven.com), CC0)
- Animation bar: clip selection, play/pause, scrubbing, speed
- Info panel: file and scene stats plus the [Khronos glTF-Validator](https://github.com/KhronosGroup/glTF-Validator) report (skipped above 64 MB to keep the viewer responsive)
- ←/→ steps through the other glTF files in the same folder (in Explorer's sort order)
- Double-click / "Open with" support, drag & drop onto the window, optional start with Windows

### Keyboard

| Key | Action |
| --- | --- |
| Space / Esc | Close the preview |
| ← → (or ↑ ↓) | Previous / next model in the folder |
| F | Fullscreen |
| I | Info & validation panel |
| S | Display settings |
| Home | Reset camera |

## Privacy

- The viewer is fully offline. It makes no network requests, has no telemetry and no update check.
- To notice the Space key it installs a low-level keyboard hook. The hook looks only at whether the key is Space and whether an Explorer or desktop file list is in front. No other key is read, stored or sent anywhere, and Space is never swallowed.
- Model files are read through an internal protocol that serves only model and texture file types from disk, never anything else.
- Errors, including the paths of files that failed to open, are written to `%LOCALAPPDATA%\studio.ivar.gltf-viewer\logs\viewer.log` (rotated at 1 MB). Nothing leaves the machine.

See [SECURITY.md](SECURITY.md) for how to report a vulnerability.

## Develop

Requirements: Node 20+, Rust (stable, MSVC toolchain), and Visual Studio Build Tools with the C++ workload. WebView2 ships with Windows 11.

```sh
npm install
npm run tauri dev        # run with hot reload (opens a file picker; pick a model)
npm run tauri build      # release build + NSIS installer in src-tauri/target/release/bundle/nsis/
npm run build            # type-check and build the frontend only
cargo test --manifest-path src-tauri/Cargo.toml
```

Frontend only, in a browser: `npm run dev` and open `http://localhost:1420/?model=/path/served/by/vite.glb`.

Dev builds never register themselves to start at login and never ask the first-run question.

### Test models

`test-assets/` is git-ignored. Fill it from the [Khronos glTF-Sample-Assets](https://github.com/KhronosGroup/glTF-Sample-Assets) repo. Good coverage comes from DamagedHelmet, Fox, CesiumMan, Duck (Draco), FlightHelmet (PNG and KTX2 variants), a `gltfpack -cc` Meshopt file, and a `.gltf` with textures in `../`.

### Releasing

CI (`.github/workflows/ci.yml`) type-checks, builds and runs the Rust tests on every push. A `v*` tag additionally builds the installer and attaches it, with `SHA256SUMS.txt`, to a **draft** GitHub Release for review.

1. Set the new version in `src-tauri/tauri.conf.json` (the source of truth) and copy it to `package.json` and `src-tauri/Cargo.toml`. CI fails if they differ.
2. Commit, then `git tag v1.2.3 && git push --tags`.
3. Review the draft release on GitHub, publish it, and update the download link on the website.

Code signing is not wired up yet; see [#1](https://github.com/ivarstudios/gltf-viewer/issues/1) for the plan and the placeholder step in the workflow.

## How it works

```
Tray process (Rust, always running)
 ├─ tray.rs           tray icon + menu
 ├─ hook.rs           low-level keyboard hook: Space in Explorer/desktop file list
 │                    (re-installed every 5 min; Windows drops slow hooks silently)
 ├─ explorer.rs       Shell COM: selected file + folder order of the active tab
 ├─ viewer_window.rs  viewer window: created on demand, hidden on close,
 │                    destroyed after 3 idle minutes (no WebView2 while idle)
 └─ protocol.rs       http://model.localhost/<path> serves local model files
                      (allow-listed file types only, 1 GiB cap)
Viewer (WebView2 + Vite/TypeScript/Three.js, src/)
```

## Roadmap

- Code signing and winget publication ([#1](https://github.com/ivarstudios/gltf-viewer/issues/1))
- Update channel for installed copies ([#3](https://github.com/ivarstudios/gltf-viewer/issues/3))
- FBX, OBJ, USDZ, STL, PLY and other formats
- Validation in a Web Worker
- Space inside file open/save dialogs, Explorer thumbnails

## Credits and licenses

All bundled third-party components and their licenses are listed in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md), which is also installed next to the application.

- [three.js](https://threejs.org) (MIT)
- [three-gltf-viewer](https://github.com/donmccurdy/three-gltf-viewer), Don McCurdy (MIT)
- [glTF-Validator](https://github.com/KhronosGroup/glTF-Validator), The Khronos Group (Apache-2.0)
- HDRIs from [Poly Haven](https://polyhaven.com) (CC0)

© IVAR Studios AB

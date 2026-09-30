# IVAR glTF Viewer

A Quick Look-style previewer for `.glb` and `.gltf` files on Windows, built by IVAR Studios AB.

It runs in the system tray. Select a glTF file in Explorer (or on the desktop) and press **Space** to preview it; press **Space** or **Esc** again to close. It works fully offline.

## Features

- Three.js viewer modelled on [three-gltf-viewer](https://github.com/donmccurdy/three-gltf-viewer) by Don McCurdy (MIT)
- Draco, KTX2/Basis and Meshopt compressed files (decoders bundled, no network needed)
- `.gltf` files load their `.bin` and texture files from disk, including relative paths such as `../textures/`
- Neutral studio lighting plus 4 bundled HDR environments ([Poly Haven](https://polyhaven.com), CC0)
- Animation bar: clip selection, play/pause, scrubbing, speed
- Info panel: file and scene stats plus the [Khronos glTF-Validator](https://github.com/KhronosGroup/glTF-Validator) report
- ←/→ steps through the other glTF files in the same folder (in Explorer's sort order)
- Double-click / "Open with" support, drag & drop onto the window, and it starts with Windows (can be turned off from the tray menu)

### Keyboard

| Key | Action |
| --- | --- |
| Space / Esc | Close the preview |
| ← → (or ↑ ↓) | Previous / next model in the folder |
| F | Fullscreen |
| I | Info & validation panel |
| S | Display settings |
| Home | Reset camera |

## Install

Run `IVAR glTF Viewer_<version>_x64-setup.exe` from `src-tauri/target/release/bundle/nsis/`. It installs per user and doesn't need admin rights. The installer isn't code-signed yet, so Windows SmartScreen will show a warning the first time ("More info" → "Run anyway").

To make it the default for double-click, right-click a `.glb` → *Open with* → *Choose another app* → **IVAR glTF Viewer** → *Always*.

## Develop

Requirements: Node 20+, Rust (stable, MSVC toolchain), and Visual Studio Build Tools with the C++ workload. WebView2 ships with Windows 11.

```sh
npm install
npm run tauri dev        # run with hot reload (opens a file picker; pick a model)
npm run tauri build      # release build + NSIS installer
```

Frontend only, in a browser: `npm run dev` and open `http://localhost:1420/?model=/path/served/by/vite.glb`.

Logs are written to `%LOCALAPPDATA%\studio.ivar.gltf-viewer\logs\viewer.log`.

### Test models

`test-assets/` is git-ignored. Fill it from the [Khronos glTF-Sample-Assets](https://github.com/KhronosGroup/glTF-Sample-Assets) repo. Good coverage comes from DamagedHelmet, Fox, CesiumMan, Duck (Draco), FlightHelmet (PNG and KTX2 variants), a `gltfpack -cc` Meshopt file, and a `.gltf` with textures in `../`.

## How it works

```
Tray process (Rust, always running)
 ├─ tray.rs           tray icon + menu
 ├─ hook.rs           low-level keyboard hook: Space in Explorer/desktop file list
 ├─ explorer.rs       Shell COM: selected file + folder order of the active tab
 ├─ viewer_window.rs  viewer window: created on demand, hidden on close,
 │                    destroyed after 3 idle minutes (no WebView2 while idle)
 └─ protocol.rs       http://model.localhost/<path> serves local model files
Viewer (WebView2 + Vite/TypeScript/Three.js, src/)
```

## Roadmap

- FBX, OBJ, USDZ, STL, PLY and other formats
- GitHub Actions build that attaches the installer to Releases
- Code signing for public distribution
- Space inside file open/save dialogs, Explorer thumbnails

## Credits

- [three.js](https://threejs.org) (MIT)
- [three-gltf-viewer](https://github.com/donmccurdy/three-gltf-viewer), Don McCurdy (MIT)
- [glTF-Validator](https://github.com/KhronosGroup/glTF-Validator), The Khronos Group (Apache-2.0)
- HDRIs from [Poly Haven](https://polyhaven.com) (CC0)

© IVAR Studios AB

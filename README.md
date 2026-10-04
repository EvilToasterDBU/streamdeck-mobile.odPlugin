# Stream Deck Mobile for OpenDeck

An [OpenDeck](https://github.com/nekename/OpenDeck) plugin that lets the official **Stream Deck Mobile** Android app (`com.corsair.android.streamdeck`, formerly Elgato) act as a virtual button panel for OpenDeck on Linux.

The plugin speaks the app's VSD2 protocol, so OpenDeck shows up to the phone like a Stream Deck desktop host: the phone finds it on the local network (or by QR code), you approve the pairing on the desktop, and a virtual device appears in OpenDeck. Icons are pushed to the phone, button presses come back as normal OpenDeck key events, and the phone can resize the key grid (up to 8x8) on the fly.

> **Disclaimer.** This is an unofficial, independent project. It is not affiliated with, endorsed by, or supported by Elgato, Corsair, or the OpenDeck authors. "Stream Deck" is a trademark of its respective owner. The protocol implementation was written by observing the app's behaviour; it may break whenever the app is updated.

## Status

Working and tested with the Corsair-branded Stream Deck Mobile app on Android, against OpenDeck on Linux (CachyOS, KDE Plasma / Wayland):

- Pairing by QR code and by network discovery (mDNS), including reconnecting to a known computer
- Pairing approval dialog on the desktop
- Virtual device creation, rename, and removal from either side
- Live key grid resize from the phone
- Icon rendering and key presses in both directions
- Management window (device list, rename, remove, add device with QR) themed from OpenDeck's own colours

Linux only (x86_64 bundle). It relies on Unix sockets and, for network discovery, on Avahi.

## Requirements

- OpenDeck
- Rust toolchain (stable, edition 2024 — Rust 1.85 or newer) and `binutils` (`strings`) to build from source
- `avahi-daemon` and `avahi-utils` (`avahi-publish-service`) for network discovery. Without them the plugin falls back to its own multicast announcer; QR pairing never needs mDNS
- Phone and PC on the same network, with TCP ports **28197–28199** reachable on the PC and multicast UDP (mDNS, port 5353) allowed

## Install

### From a release (recommended)

Releases ship the plugin as a single `.odPlugin` file — no Rust toolchain needed.

1. Download `com.toshtaru.streamdeck-mobile-v<version>.odPlugin` from the [Releases](../../releases) page. Optionally check it against the `.sha256` file next to it: `sha256sum -c <file>.sha256`.
2. In OpenDeck, open **Plugins** and click **Install from file**, then select the downloaded `.odPlugin` file.
3. Wait for the "Successfully installed" message. If the plugin does not show up or start, fully quit and restart OpenDeck.

To update, install the newer `.odPlugin` the same way; it replaces the old version. Your pairings and saved devices live outside the plugin folder (see [Files and ports](#files-and-ports)) and are kept.

An `.odPlugin` is an ordinary zip archive whose only top-level entry is the `com.toshtaru.streamdeck-mobile.sdPlugin` folder. If you prefer to install by hand, unzip it into `~/.config/opendeck/plugins/` and restart OpenDeck.

### From source

```bash
git clone <this repository>
cd streamdeck-mobile.odPlugin
./install-plugin.sh
```

The script does a clean release build, copies the plugin to `~/.config/opendeck/plugins/com.toshtaru.streamdeck-mobile.sdPlugin`, and checks that the manifest, `Cargo.toml`, `VERSION`, and the compiled binary all report the same version. Then **fully quit and restart OpenDeck** — it only loads native plugins at startup.

Options: `--reset-pairing` (forget all saved devices and trust), `--verify` (check the install and running process).

Set `OPENDECK_CONFIG_DIR` if your OpenDeck config is not in `~/.config/opendeck`.

### Building the `.odPlugin` yourself

```bash
./build-plugin.sh
```

This builds the release binary and writes `dist/com.toshtaru.streamdeck-mobile-v<version>.odPlugin` plus a `.sha256` file. It is the same script the GitHub workflows run. Set `SKIP_BUILD=1` to package an existing `target/release` binary without rebuilding.

## Usage

1. In OpenDeck, open the **Stream Deck Mobile** plugin settings. A management window opens.
2. Click **Add device** and scan the QR code with the Stream Deck Mobile app — or just open the app and pick this computer from its network search.
3. Compare the verification code on the pairing dialog with the one on your phone and click **Approve**.
4. Create (or open) a virtual device in the app. It appears in OpenDeck as a new device; drag actions onto it as usual.

Removing the virtual device — from the phone or from the management window — does not un-pair the phone. A trusted phone can recreate its device at any time without pairing again. To make a phone untrusted, clear the app's data on the phone, or run `./install-plugin.sh --reset-pairing` on the desktop.

## Files and ports

| Path / port | Purpose |
|---|---|
| `~/.config/opendeck/streamdeck-mobile.json` | Saved virtual devices and trusted phone keys |
| `~/.config/opendeck/streamdeck-mobile-workstation-id` | Stable host ID presented to phones. **Do not change or delete** — see below |
| `~/.config/opendeck/streamdeck-mobile-theme.json` | Colours copied from OpenDeck for the plugin's windows |
| `~/.local/state/opendeck-streamdeck-mobile/plugin.log` | Plugin log |
| TCP 28197 / 28198 / 28199 | Legacy, VSD2 (QR pairing), and VSD2 for network-discovery connections |

The Stream Deck Mobile app remembers a computer's ID the first time it sees it and rejects any later connection that presents a different one. The plugin therefore generates its ID once and keeps it forever. If you delete the file, phones that already know this computer will refuse to reconnect until you forget the computer in the app (or re-pair by QR).

## Troubleshooting

- **Plugin not listed in OpenDeck** — restart OpenDeck completely, and check `~/.local/share/opendeck/logs/opendeck.log` for a manifest error.
- **Phone does not find the computer** — check that the phone is on the same subnet, that `avahi-daemon` is running, and that a firewall is not blocking UDP 5353 or TCP 28197–28199. QR pairing works without discovery.
- **Phone connects but immediately drops** — check `~/.local/state/opendeck-streamdeck-mobile/plugin.log`. For phone-side details, `adb logcat` (tags `StreamDeckConnection`, `DiscoveryService`) shows what the app is actually trying to do.
- **Debug capture** — set `OPENDECK_MOBILE_RAW=1` in the environment OpenDeck runs in to log every protocol frame to `~/.config/opendeck/streamdeck-mobile-wire.log`. This includes decrypted traffic; do not share the file publicly.

## Security notes

- Pairing always requires explicit approval on the desktop. The verification code in the dialog is the phone's key fingerprint; confirm it matches the phone before approving.
- The plugin listens on all interfaces on TCP 28197–28199 and announces itself over mDNS. Use it on networks you trust.
- The host's encryption key, workstation ID, and announced pairing token are derived deterministically from the hostname and `/etc/machine-id`, so no secret file has to be stored. That also means they are only as private as those two values.

## Uninstall

```bash
./uninstall-plugin.sh
```

This removes the plugin bundle (you can also remove it from OpenDeck's **Plugins** dialog). Saved state in `~/.config/opendeck/streamdeck-mobile*` and the log directory are left in place; delete them by hand for a clean slate.

## Development

[`CLAUDE.md`](CLAUDE.md) contains detailed technical notes: protocol structure, the three-port layout, mDNS design, the trust-versus-virtual-device model, how OpenDeck handles grid resizing, and why the windows run as separate processes.

Source layout: `src/mobile.rs` (VSD2 protocol, pairing, mDNS), `src/openaction.rs` (OpenDeck WebSocket client), `src/ui.rs` (egui windows), `src/ipc.rs` (plugin ↔ window commands), `src/main.rs` (entry point).

When you change anything, keep `VERSION`, `Cargo.toml`, and `com.toshtaru.streamdeck-mobile.sdPlugin/manifest.json` in sync — the installer and the build script refuse to continue if they differ.

### CI and releases

- **CI** (`.github/workflows/ci.yml`) builds the plugin on every push to `main` and on every pull request, and attaches the resulting `.odPlugin` to the run as an artifact.
- **Release** (`.github/workflows/release.yml`) runs when a `v*` tag is pushed. It builds the plugin, packages the `.odPlugin` with a SHA-256 checksum, and publishes both as a GitHub Release with generated notes. The tag must equal `VERSION` (for example `VERSION` `0.8.9` → tag `v0.8.9`).

To cut a release: bump the version in the three files, commit, then

```bash
git tag v<version>
git push origin v<version>
```

## License

[MIT](LICENSE)

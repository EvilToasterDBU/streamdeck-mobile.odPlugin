# OpenDeck Stream Deck Mobile — technical reference

A Rust plugin for OpenDeck that implements the VSD2 protocol used by the official
**Stream Deck Mobile** Android app (now Corsair-branded,
`com.corsair.android.streamdeck`, after Corsair's acquisition of Elgato). The plugin makes
OpenDeck look to the phone like a real Stream Deck Desktop/VSD2 server: it advertises itself
over mDNS, accepts pairing by QR code or through the phone's "known device" reconnect, creates
a virtual device in OpenDeck, and relays icons and key presses in both directions.

## Hard architectural requirement

**This MUST be an OpenDeck plugin, not a fork or patch of OpenDeck itself.** The project owner
was explicit and categorical about it: the goal is an ordinary, installable plugin that other
people can use with a vanilla OpenDeck, not a personally rebuilt copy of the application. Any
idea that requires touching OpenDeck's source or build is out of the question; solve things on
the plugin side (subprocesses, IPC, files on disk — see below).

## Code layout

```
src/
  main.rs       — entry point, parses OpenDeck's CLI arguments, dispatches --open-management/--open-approval
  openaction.rs — WebSocket client to OpenDeck (JSON, OpenAction protocol)
  mobile.rs     — all VSD2 logic: protocol, state, mDNS, per-client handler (~1900 lines)
  ipc.rs        — Unix-socket IPC between the long-running plugin process and short-lived UI processes
  ui.rs         — eframe/egui windows (Management, Approval), each launched as a separate process
```

Manifest: `com.toshtaru.streamdeck-mobile.sdPlugin/manifest.json`. The `"Actions": []` field is
mandatory: OpenDeck's runtime (`src-tauri/src/plugins/manifest.rs`, `PluginManifest.actions:
Vec<Action>` without `#[serde(default)]`) silently skips a plugin at load time when the
`Actions` key is missing entirely (`list_plugins()` does a `continue` on a parse error) — the
plugin simply never shows up in the list, with no visible error in the OpenDeck UI.

## The VSD2 protocol

Hand-written with `prost::Message` — there are no `.proto` files; all structures are declared
directly in Rust with `#[prost(...)]` attributes at the very bottom of `mobile.rs`.
`ServerMessage`/`ClientMessage` are protobuf `oneof`s with explicit tags (Hello=1/10,
Authenticate=2, CreateVirtualDevice=10/11/12, etc. — see the code; the tags must match exactly
what the app expects).

Encryption: `crypto_box::SalsaBox` (NaCl box, X25519 + XSalsa20-Poly1305). The server key is
deterministic (`server_secret()` — Blake2b512 over hostname + `/etc/machine-id`), so it is
stable across restarts without a separate key file. Encrypted frame format:
`nonce (24 bytes) || ciphertext`.

### Three TCP ports — and why exactly three

- **28197** (`LEGACY_PORT`) — the legacy protocol, used mostly for the simple mDNS
  advertisement (`a=` in the TXT record).
- **28198** (`PORT`) — "regular" VSD2. This is the port embedded in QR pairing (`av2=` in the
  TXT record and in the URL itself, `https://streamdeck.elgato.com/connect/?...`). QR pairing
  has always worked through this port.
- **28199** (`PORT + 1`, `modern`) — added after analysing `adb logcat`: the current
  Corsair build of the app, when it connects through **mDNS discovery**
  (`source=ELGATO_DISCOVERY`, whether for a "known device" or a fresh network search), always
  connects to `av2_port + 1`, never to `av2` itself. Before this listener existed every such
  connection failed with `Connection refused`/a timeout — mDNS itself worked fine and the
  address was found correctly, nobody was simply listening on that port. It runs the very same
  `handle_client`; no separate logic is needed.

If a future app version again shows "nothing connects over the network but QR works", first
check `adb logcat` (tags `StreamDeckConnection`, `DiscoveryService`) for the port the TCP
connection actually targets.

### workstation_id — must be pinned once and kept forever

`workstation_id()` in `mobile.rs` generates/reads the ID from
`~/.config/opendeck/streamdeck-mobile-workstation-id`. **Important**: the phone app caches this
ID the first time it discovers the host and, on later reconnects to a "known device", compares
it with the `HelloFromServer.workstation_id` it receives on EVERY new connection — but the cached
value is **not refreshed automatically** by a new mDNS TXT record, nor by a plain "clear app
cache" (Android's "Clear cache" does not touch it). On a mismatch the app drops the
already-established TCP/WS connection with
`M3.d$h: Unexpected Workstation ID received from server` — the connection itself works, the
server is simply "not who they expected".

Hence the rule: **once a value has been cached on a phone, the server must present that same ID
forever** instead of honestly generating a new one from a deterministic hash.
`workstation_id()` therefore trusts any non-empty value in the file (it used to demand exactly
`sr-<19 base64url>`, 22 characters — that is still the default format on first generation, but
reading no longer requires that shape). If you ever need to re-pin a different cached ID,
overwrite that file by hand (kill the plugin first, see the race below).

**Deployment pitfall when changing this file**: if the old plugin version is still alive when you
edit the file, it will overwrite it back with its own logic on the next 10-second mDNS broadcast
(`run_mdns_raw`/`run_mdns_avahi` call `workstation_id()` every cycle). The order must be: kill
the `opendeck` process → write the file → rebuild/install the new version → start `opendeck`.

## mDNS / network discovery

Two independent mechanisms run SIMULTANEOUSLY and UNCONDITIONALLY (as in the reference
implementation — the "raw" announcer used to be gated as a fallback for avahi failing, which
practically never happens on a normal Linux desktop, so the raw publisher effectively never ran
during the whole debugging effort):

1. **`run_mdns_avahi()`** — spawns `avahi-publish-service` as a child process.
   `PR_SET_PDEATHSIG` (via `libc::prctl` in `pre_exec`) guarantees that if the plugin itself is
   killed abruptly (SIGKILL/`pkill`/an unclean shutdown by OpenDeck), the orphaned
   `avahi-publish-service` does not outlive it and block registration of the same service name
   on the next start. Every 15 seconds it checks whether the local IP changed (e.g. on a
   network or hotspot switch) and restarts publishing with a fresh TXT record if so — the
   address used to be baked in once at startup and never revisited.
2. **`run_mdns_raw()`** — its own raw multicast UDP sender (224.0.0.251:5353) of a
   hand-assembled mDNS response, every 10 seconds, regardless of whether avahi is alive. The
   source socket is bound strictly to port 5353 via `bind_mdns_source_port()`
   (`SO_REUSEADDR`+`SO_REUSEPORT`, because the system `avahi-daemon` almost always already holds
   5353) — some resolvers (Android's included) silently ignore announcements whose source port
   is not 5353 (RFC 6762).

`local_ipv4()` prefers an interface named `wl*` (WiFi) over whatever the OS considers the
default route — on a machine with both Ethernet and WiFi active (for example a phone hotspot
used for testing), routing almost always picks the wired address while the phone is physically
on WiFi.

## Trust vs Virtual Device — two different, independent concepts

`PersistedState` in `mobile.rs`:
```rust
struct PersistedState {
    devices: Vec<SavedDevice>,             // the virtual device visible in OpenDeck
    trusted_keys: HashMap<String, String>, // the phone's trusted public key — permanent
}
```
Pairing a phone for the first time does BOTH things at once — but afterwards they are
independent. Deleting the virtual device (from either side: the desktop "Remove" button or
`DeleteVirtualDevice` from the phone) **must not** touch `trusted_keys`. The real Elgato
ecosystem has no concept of "revoking trust" on the desktop at all — the only way a phone loses
trust is by getting a new identity itself (for example, clearing the app's data). Both
operations (the desktop Remove button and the phone's DeleteVirtualDevice) call the same
`remove_device()`.

Before advertising a virtual device to a trusted fingerprint, `build_vdl()` checks that an entry
in `devices` really exists — otherwise a trusted phone with no device would see a "phantom"
device that was then actually created anew the moment it tried to open it.

## Resizing the button grid — OpenDeck does not resize its storage by itself

When the phone changes its layout (`UpdateKeypadConfig`/`OpenVirtualDevice` with a new
`layout_crop`, for example 5×3 → 8×4), it is not enough to just send `registerDevice` again with
the new `rows`/`columns`. In OpenDeck (see its source,
`src-tauri/src/store/profiles.rs`, `get_profile_store_mut`) the profile's key storage
(`Store.keys`, of length `rows*columns+touchpoints`) is allocated and resized **only once** — the
first time that device+profile pair is loaded into its in-memory cache (the `stores` HashMap). A
repeated `registerDevice` for an already-registered device is treated as a pure metadata update:
`DeviceInfo` (and therefore the rendered grid) does pick up the new size, but `Store.keys` keeps
its old length — so positions ≥ the old `rows*columns` silently refuse to accept a dropped
action (`get_slot_mut` returns "index out of bounds") even though the grid looks bigger. The
cache is only cleared in `remove_profile`, which is called from `deregister_device`.

The Stream Deck Mobile module built into the project owner's OpenDeck fork
(`EvilToasterDBU/OpenDeck`, not the upstream `nekename/OpenDeck`:
`src-tauri/src/streamdeck_mobile.rs`, `update_mobile_device_layout`) works around this exactly the
way this plugin now does: **`deregisterDevice` first, then `registerDevice`** with the new size —
this forces the `Store` to be recreated via `Store::new()`+`resize()` with the right length.

Implementation in `update_device_registration()` (`mobile.rs`): the static
`REGISTERED_DEVICE_SIZE: HashMap<device_id, (rows, cols)>` remembers what we last actually told
OpenDeck about the device's size. If the new `(rows, cols)` differs from the remembered value we
send `deregisterDevice` first, then the normal `registerDevice`. If it is the same (for example
a plain reconnect of a "known device" with the same saved layout), no `deregister` is needed and
a cheap update suffices, without making the device flicker. It is important not to do
`deregister`+`register` unconditionally on every call — otherwise every phone reconnect would
cause a needless disconnect/reconnect of the device in OpenDeck.

A full restart of OpenDeck itself (see "Build, install, versioning") also resets this cache —
so the truncated-grid bug only reproduces when resizing **within one live OpenDeck session**,
without restarting it between the size change and the check.

## Pairing flow (short version)

1. `Hello` (unencrypted) → the server replies with `HelloFromServer`, with
   `client_needs_authentication` depending on whether the fingerprint is in `AUTHENTICATED`.
2. If not trusted, the phone sends an encrypted `Authenticate`. The server puts the request in
   `PENDING` and spawns the approval window (`ui::spawn_approval_process`, but only if that
   fingerprint has no active pending request yet — otherwise a duplicated `Authenticate` from
   the same phone would open a second window).
3. The user presses Approve/Reject in a separate process, which sends
   `APPROVE <fp>`/`REJECT <fp>` to the main process through `ipc.rs`.
4. The main process commits trust (`trusted_keys`), creates/updates the `SavedDevice`, and sends
   `Ok` + `VirtualDeviceList` on the same socket.
5. The phone sends `CreateVirtualDevice`/`OpenVirtualDevice` → the server registers the device in
   OpenDeck (`registerDevice`) and sends `VirtualDeviceOpened` + `Context` (the list of actions
   on the keys).

Subtleties that used to break everything:
- The `CreateVirtualDevice` ack is **only** `VirtualDeviceOpened`, with no separate leading `Ok`
  (the real client mixes up request/response matching and stalls for ~20 seconds if an extra
  `Ok` is sent).
- An unencrypted, not-yet-authenticated `Hello` socket must not be sent anything extra after
  `HelloFromServer` — the reference stays silent until `Authenticate`, and any additional
  plaintext VDL confuses the client.
- Parallel `Authenticate` sockets from the same phone are serialized through
  `TRUST_COMMIT_LOCK`, so trust is not committed twice independently.
- When a device is removed you must not only send an empty VDL to the discovery sockets but also
  forcibly close them (`refresh_trusted_discovery`) — otherwise there is a race in which a socket
  whose key was already wiped receives encrypted traffic and silently drops it
  (`ENCRYPTED_DROP`), leaving the phone hanging with no reply.

## UI: separate processes instead of one window

`eframe`/`winit` do not allow recreating the `EventLoop` within one process
(`EVENT_LOOP_CREATED` is a process-wide static, with no reset outside wasm). On this
Wayland+KDE combination, none of the following worked reliably:
`ViewportCommand::Visible(false)` (the window stays mapped, just "frozen"), `Minimized(true)`
(restoring is ignored on Wayland — a client physically cannot un-minimize itself), or
multi-viewport with a permanently invisible root viewport (it still shows up as a ghost window
in the window switcher).

**The solution**: the Management and Approval windows are launched as separate short-lived OS
processes — a re-invocation of the same binary with the flags `--open-management` /
`--open-approval --fingerprint ... --name ... --peer ...` (dispatched at the start of `main.rs`,
before connecting to OpenDeck). Each such process is an ordinary single-window
`eframe::run_native` that simply ends the whole process when its window closes — there is no
hide/show/minimize state to maintain, because nothing is left running once the window is gone.

Since the processes are separate, they cannot access the main process's in-memory state. Shared
state lives on disk:
- The device list: read directly from `~/.config/opendeck/streamdeck-mobile.json`
  (`saved_devices_from_disk()`); the Management window re-reads the file every 500 ms.
- OpenDeck's colour theme: the main process parses `-info` (passed by OpenDeck when it launches
  native plugins) once at startup and writes it to
  `~/.config/opendeck/streamdeck-mobile-theme.json` (`ui::persist_theme_from_info`); the UI
  processes read that file at startup.
- Actions that must run in the main process (approve/reject a pairing, rename/remove a device)
  go through `ipc.rs`, a simple Unix socket `~/.config/opendeck/streamdeck-mobile-ui.sock` with a
  line-based text protocol (`APPROVE <fp>`, `REJECT <fp>`, `REMOVE <fp>`,
  `RENAME <fp> <name...>`). The UI processes are synchronous (no tokio runtime of their own), so
  they send commands over a blocking `std::os::unix::net::UnixStream` (`send_command_blocking`).
- `ApprovalApp` has a `decided` flag, set BEFORE `ViewportCommand::Close` is sent — our own
  `Close` makes `close_requested() == true` on the next frame, which without that flag is
  indistinguishable from the user closing the window without choosing, and would send an extra
  REJECT on top of the APPROVE that was just sent.

## Persistent files (`~/.config/opendeck/`)

| File | Purpose |
|---|---|
| `streamdeck-mobile.json` | `PersistedState`: `devices` + `trusted_keys` |
| `streamdeck-mobile-workstation-id` | Stable workstation ID (see above — do not change without dire need) |
| `streamdeck-mobile-theme.json` | Theme colours taken from the live OpenDeck |
| `streamdeck-mobile-ui.sock` | Unix socket for IPC between the main process and the UI processes |
| `streamdeck-mobile-wire.log` | Raw wire capture, only with `OPENDECK_MOBILE_RAW=1` |

## Build, install, versioning

```bash
./install-plugin.sh                   # clean cargo build --release, copies the binary into the bundle and into ~/.config/opendeck/plugins/
./install-plugin.sh --reset-pairing   # + wipe streamdeck-mobile.json
./install-plugin.sh --verify          # + check the running process
./build-plugin.sh                     # build + package dist/com.toshtaru.streamdeck-mobile-v<version>.odPlugin (+ .sha256)
SKIP_BUILD=1 ./build-plugin.sh        # package an existing target/release binary without rebuilding
```
`install-plugin.sh` compares the version in `VERSION` with `Cargo.toml` and with the string
embedded in the binary itself (via `strings`: a release build with `strip+lto` packs adjacent
string literals with no separator, so an exact whole-line match is unreliable and a substring
search is used; `strings` runs into a variable first instead of being piped straight into
`grep -q`, so that `grep`'s early exit cannot cause a SIGPIPE which `pipefail` would otherwise
report as a real failure). `build-plugin.sh` enforces that `VERSION`, `Cargo.toml` and
`manifest.json` agree.

**Mandatory rule from the project owner**: every time a built plugin is handed over for
testing, the version is bumped by 1 (last digit) in three places at once: `VERSION`,
`Cargo.toml` (`[package] version`), `manifest.json` (`Version`). `0.8.7 → 0.8.8`; after `.9` the
next minor (`0.8.9 → 0.9.0`). Without this the owner cannot be sure they are testing the fresh
build.

After installing, always `pkill -x opendeck` and then
`nohup /usr/bin/opendeck > /tmp/opendeck.log 2>&1 & disown` (a full restart, not just "reload
plugins" — when a plugin is replaced on disk, OpenDeck only picks up native plugins at a full
start).

### The `.odPlugin` package and releases

An `.odPlugin` is a plain zip whose **only** top-level entry is the
`com.toshtaru.streamdeck-mobile.sdPlugin/` folder (containing `manifest.json`, `icons/`, `LICENSE`
and `bin/<target>/opendeck-streamdeck-mobile` with the executable bit set). This is what
OpenDeck's Plugins → "Install from file" expects: `install_plugin` in OpenDeck's
`src-tauri/src/events/frontend/plugins.rs` reads the chosen file as a zip regardless of its
extension, takes the first path component ending in `.sdPlugin` as the plugin ID
(`zip_extract::dir_name`), and extracts the **whole** archive into `plugins/` preserving relative
paths and Unix modes — so any extra file next to the `.sdPlugin` folder (README, scripts) would
end up loose in OpenDeck's `plugins/` directory. `build-plugin.sh` therefore assembles the
bundle in a scratch directory and zips only that folder.

GitHub Actions (`.github/workflows/`):
- `ci.yml` — on push to `main` and on pull requests: builds, packages, uploads the `.odPlugin`
  as a run artifact.
- `release.yml` — on a pushed `v*` tag: checks the tag equals `v$(cat VERSION)`, builds,
  packages, and publishes a GitHub Release with the `.odPlugin` and its `.sha256`.
  Releasing = bump the three version files, commit, `git tag v<version> && git push origin
  v<version>`.

Both workflows run `./build-plugin.sh`, so what CI builds is exactly what gets released.

## Diagnostics

- Plugin log: `~/.local/state/opendeck-streamdeck-mobile/plugin.log` (or
  `$OPENDECK_MOBILE_LOG`).
- OpenDeck's own log: `~/.local/share/opendeck/logs/opendeck.log`.
- Raw wire capture (decrypted and raw bytes of VSD2 frames): `OPENDECK_MOBILE_RAW=1`, written to
  `streamdeck-mobile-wire.log`.
- **`adb logcat`** — the source of truth for the Android app's own behaviour when the plugin's
  logs are not enough (for example, the port-28199 and workstation-id bugs were found only
  through it). Useful tags: `StreamDeckConnection`, `DiscoveryService`. Run `adb logcat -c`
  before a test and `adb logcat -d > file` after it, then look for
  `Connecting to StreamDeckEndpoint`, `Connection established`, `Unexpected Workstation ID`.
- Reference implementation for cross-checking protocol details: the OpenDeck fork with built-in
  Stream Deck Mobile support (`EvilToasterDBU/OpenDeck`, "tessttt"); look at it whenever you are
  unsure of the exact format of a message or field. The author's local copy is
  `~/.config/opendeck/streamdeck_mobile.rs.orig` — it is not part of this repository.

## Known gotchas — do not step on these again

- Never gate `run_mdns_raw()` behind avahi succeeding/failing — both must always run at the
  same time.
- Do not rely on "the OS will pick the interface" for the local IP when Ethernet and WiFi are
  both present — prefer the `wl*` interface explicitly.
- `workstation_id()` is not "deterministically plausible", it is "the very value already cached
  on real phones". Do not touch the format without dire need, and kill the old process before
  editing the file.
- Trust and Virtual Device — never merge their removal/reset logic again.
- egui cannot render glyphs outside its bundled fonts (for example `←`) — use ASCII (`<`) or
  emoji instead of Unicode arrows.
- Any new UI window must go through a separate process (`spawn_self`); never try to show/hide an
  already-created eframe viewport again.
- An `.odPlugin` must contain nothing but the `.sdPlugin` folder at its top level (see above).

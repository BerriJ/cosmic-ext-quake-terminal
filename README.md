# COSMIC Quake Terminal

A quake-style dropdown terminal for [COSMIC Desktop](https://github.com/pop-os/cosmic-epoch).

Runs as a background daemon and toggles `cosmic-term`'s visibility via a keyboard shortcut, similar to Guake, Yakuake, or the Quake console.

## How it works

- Runs as a background daemon with no visible window
- On first toggle, spawns `cosmic-term`
- Subsequent toggles hide (minimize) or show (activate + focus) the terminal
- Uses COSMIC's Wayland toplevel management protocol (`zcosmic_toplevel_manager_v1`) for window control
- D-Bus activation handles IPC between the CLI toggle command and the running daemon

## Installation

### Building from source

**Dependencies:**

- Rust toolchain (stable)
- [just](https://github.com/casey/just) command runner
- Wayland development libraries
- A running COSMIC Desktop session

```sh
git clone https://github.com/m0rf30/cosmic-ext-quake-terminal.git
cd cosmic-ext-quake-terminal
just build-release
sudo just install
```

### Uninstall

```sh
sudo just uninstall
```

## Required cosmic-comp setup — make the terminal float

If you use cosmic-comp's **tiling** layout, the spawned `cosmic-term` window will be placed into the tiling layout by default, which defeats the Quake-style dropdown. To prevent this, add an application exception so cosmic-comp always treats `cosmic-term` as floating.

### Via cosmic-settings (GUI)

1. Open **Settings → Desktop → Window management → Window rules**.
2. Add a new application exception:
   - **Application ID:** `com.system76.CosmicTerm`
   - **Title:** leave empty
3. Save. New `cosmic-term` windows will now open floating.

### By editing the config file

Append an entry to `~/.config/cosmic/com.system76.CosmicSettings.WindowRules/v1/tiling_exception_defaults`. The file is a RON list of `ApplicationException { appid, title }` entries (both are regex strings):

```ron
[
    (appid: "com.system76.CosmicTerm", title: ""),
]
```

After the rule is in place, cosmic-comp will remember the floating window's geometry per-application — position and resize the dropdown once and subsequent toggles will reuse that geometry.

### Trade-offs

This daemon spawns `cosmic-term` as a regular Wayland toplevel and controls it via the toplevel-management protocol. Without a layer-shell surface, the following are **not** possible from this app:

- Forcing a specific size, position, or monitor
- Always-on-top / show-on-all-workspaces
- Slide-down / slide-up animation

These require an embedded terminal widget rendered into a layer-shell surface, which is a future direction for the project.

## Configuration

Configuration is stored at `~/.config/cosmic/com.github.m0rf30.CosmicExtQuakeTerminal/v2/` using COSMIC's config system.

### Additional terminal arguments

```sh
mkdir -p ~/.config/cosmic/com.github.m0rf30.CosmicExtQuakeTerminal/v2
echo '["--some-flag", "value"]' > ~/.config/cosmic/com.github.m0rf30.CosmicExtQuakeTerminal/v2/terminal_args
```

Changes are picked up automatically without restarting the daemon. The same setting is also exposed in the in-app Settings window (`cosmic-ext-quake-terminal settings`).

## Keyboard shortcut

### Via COSMIC Settings

Add a custom shortcut in **Settings > Keyboard > Shortcuts > Custom**:
- Key: `F12` (or your preferred key)
- Command: `cosmic-ext-quake-terminal toggle`

### Via config file

Add to `~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom`:

```ron
(
    modifiers: [],
    key: "F12",
): Spawn("cosmic-ext-quake-terminal toggle"),
```

### Known issue: shortcut stops working after Alt-Tab

There is a [known bug](https://github.com/pop-os/cosmic-epoch/issues/2481) in the COSMIC compositor where custom `Spawn` shortcuts may stop firing after using Alt-Tab. The [GlobalShortcuts portal](https://github.com/pop-os/xdg-desktop-portal-cosmic/issues/4) is not yet implemented in COSMIC, so the app cannot register its own global shortcut.

**Workaround:** Use an evdev-based hotkey daemon that bypasses the compositor's shortcut system:

#### swhkd (Simple Wayland HotKey Daemon)

```sh
# Install (AUR)
paru -S swhkd

# Create config
mkdir -p ~/.config/swhkd
cat > ~/.config/swhkd/swhkdrc << 'EOF'
F12
  cosmic-ext-quake-terminal toggle
EOF

# Run (swhks handles the unprivileged side, swhkd needs root for evdev)
swhks &
pkexec swhkd
```

#### Manual toggle

If the shortcut stops responding, you can always toggle from any terminal:

```sh
cosmic-ext-quake-terminal toggle
```

## Usage

The daemon starts automatically via D-Bus activation when you first run the toggle command. You can also start it manually:

```sh
# Start the daemon
cosmic-ext-quake-terminal &

# Toggle the terminal
cosmic-ext-quake-terminal toggle
```

### Debug logging

```sh
RUST_LOG=cosmic_ext_quake_terminal=debug cosmic-ext-quake-terminal
```

## License

GPL-3.0-only

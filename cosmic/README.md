# COSMIC panel applet

Native COSMIC 1.8 frontend for the installed `~/.local/bin/g6-output` CLI. Left-click the **G6** panel indicator to toggle headphones/speakers; right-click (or press Shift+F10 while focused) to choose an output explicitly. The icon reflects the actual device reply, and the popup uses the official G6 product photo installed during migration. HID errors appear in the popup; failed reads never claim that a cached output is current. Commands run asynchronously, one at a time, with a three-second deadline. Device connection/output is checked quietly every five seconds. Background reads keep controls enabled and preserve the displayed output; a click during a read is queued until it finishes. Only a user-requested switch displays progress.

Build from this directory with project-scoped tools:

```sh
unset APPIMAGE APPDIR LD_LIBRARY_PATH
cargo test --locked --release --jobs 2
cargo build --locked --release --jobs 2
install -Dm755 target/release/g6-output-cosmic ~/.local/bin/g6-output-cosmic
install -Dm644 io.github.netherguy4.G6Output.desktop ~/.local/share/applications/io.github.netherguy4.G6Output.desktop
```

Add **Sound Blaster G6 Output** in COSMIC panel settings. Keep the existing CLI and udev rule; the applet needs no new system service or root access. `Cargo.lock` pins libcosmic and cosmic-config together at COSMIC 1.8 commit `d4d71fd53e5ed6bd3a430089114dffa2da3cd498`.

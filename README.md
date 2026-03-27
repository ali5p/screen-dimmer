# screen-dimmer

Windows fullscreen click-through dimmer overlay.

## Shortcuts (Alt+S chord)

| Keys     | Action        |
|----------|---------------|
| Alt+S+↑  | Opacity down  |
| Alt+S+↓  | Opacity up    |
| Alt+S+A  | Quit          |

Opacity is persisted per hour in `usage.json` (in the current working directory).


## Build

```powershell
cargo build --release
```


## Build portable Windows binary

1. Install MSYS2  
2. In MSYS2 MINGW64 terminal:
   pacman -S mingw-w64-x86_64-gcc

### Recommended

Build:
   ```powershell
   ./build-gnu.ps1
   ```

### Manual (requires adding MinGW to PATH)

Add to PATH: ...\mingw64\bin

Build:
   ```powershell
   cargo build --release --target x86_64-pc-windows-gnu
   ```

Output binary:
target/x86_64-pc-windows-gnu/release/screen-dimmer.exe

### Experimental thread
cargo run --features gamma_exp --bin gamma_test


## Project Status

This project is a personal project developed for personal use and learning purposes.


## License

MIT — see LICENSE.txt.


## Author

© 2026 Aliona Sîrf 
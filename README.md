


## Build (portable Windows binary)

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
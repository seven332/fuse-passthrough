# FUSE Passthrough Filesystem

A FUSE (Filesystem in Userspace) implementation in Rust that mirrors one directory to another.

## Features

- ✅ File read and write
- ✅ Directory listing (readdir)
- ✅ Create/delete files and directories
- ✅ Rename files and directories
- ✅ Symbolic link support
- ✅ File attribute operations (chmod, chown, truncate)
- ✅ Auto unmount on Ctrl+C

## Dependencies

### macOS

```bash
brew install macfuse
```

> Note: You may need to restart your system after installing macFUSE.

### Linux

```bash
# Ubuntu/Debian
sudo apt-get install fuse3 libfuse3-dev

# CentOS/RHEL
sudo yum install fuse3 fuse3-devel

# Arch Linux
sudo pacman -S fuse3
```

## Build

```bash
cargo build --release
```

## Usage

### Basic Usage

```bash
# Mirror /path/to/source to /path/to/mountpoint
./target/release/fuse-passthrough -s /path/to/source -m /path/to/mountpoint
```

### Command Line Arguments

| Argument | Description |
|----------|-------------|
| `-s, --source <PATH>` | Source directory path (the directory to be mirrored) |
| `-m, --mountpoint <PATH>` | Mountpoint path (where the source will be mounted) |
| `--allow-other` | Allow other users to access the mounted filesystem |
| `-h, --help` | Show help information |
| `-V, --version` | Show version information |

### Example

```bash
# Create test directories
mkdir -p /tmp/source /tmp/mount

# Create some test files in the source directory
echo "Hello, FUSE!" > /tmp/source/hello.txt
mkdir /tmp/source/subdir
echo "Nested file" > /tmp/source/subdir/nested.txt

# Mount
./target/release/fuse-passthrough -s /tmp/source -m /tmp/mount

# Test in another terminal
ls /tmp/mount
cat /tmp/mount/hello.txt
echo "New file" > /tmp/mount/new.txt

# Press Ctrl+C to unmount
```

### Enable Debug Logging

```bash
RUST_LOG=debug ./target/release/fuse-passthrough -s /tmp/source -m /tmp/mount
```

### Allow Other Users

```bash
# Requires user_allow_other to be set in /etc/fuse.conf
./target/release/fuse-passthrough -s /tmp/source -m /tmp/mount --allow-other
```

## Manual Unmount

If you need to unmount manually:

```bash
# macOS
umount /tmp/mount

# Linux
fusermount -u /tmp/mount
# or
sudo umount /tmp/mount
```

## Project Structure

```
fuse/
├── Cargo.toml          # Project configuration and dependencies
├── README.md           # This file
└── src/
    └── main.rs         # Main program and FUSE implementation
```

## Technical Details

This project uses the [fuser](https://crates.io/crates/fuser) library to implement the FUSE interface, which is the most popular FUSE library for Rust.

### Implemented FUSE Operations

| Operation | Description |
|-----------|-------------|
| `lookup` | Look up a file/directory |
| `getattr` | Get file attributes |
| `setattr` | Set file attributes |
| `read` | Read file contents |
| `write` | Write file contents |
| `readdir` | Read directory contents |
| `open` | Open a file |
| `release` | Close a file |
| `create` | Create a file |
| `mkdir` | Create a directory |
| `unlink` | Delete a file |
| `rmdir` | Delete a directory |
| `rename` | Rename a file/directory |
| `symlink` | Create a symbolic link |
| `readlink` | Read a symbolic link |
| `access` | Check access permissions |
| `statfs` | Get filesystem statistics |
| `flush` | Flush buffers |
| `fsync` | Sync file |

## License

MIT

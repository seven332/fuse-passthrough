#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

/// Maximum time to wait for mount
const MOUNT_TIMEOUT: Duration = Duration::from_secs(10);
/// Maximum time to wait for file operations
const MAX_WAIT: Duration = Duration::from_secs(5);
/// Polling interval
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Wait for a condition to become true, with timeout
pub fn wait_for<F>(condition: F) -> bool
where
    F: Fn() -> bool,
{
    let start = Instant::now();
    while start.elapsed() < MAX_WAIT {
        if condition() {
            return true;
        }
        thread::sleep(POLL_INTERVAL);
    }
    false
}

/// Wait for a file to exist
pub fn wait_for_file(path: &Path) -> bool {
    wait_for(|| path.exists())
}

/// Wait for a file to not exist
pub fn wait_for_file_gone(path: &Path) -> bool {
    wait_for(|| !path.exists())
}

/// Wait for a directory to exist
pub fn wait_for_dir(path: &Path) -> bool {
    wait_for(|| path.is_dir())
}

pub struct MountGuard {
    mountpoint: PathBuf,
    child: Option<Child>,
}

impl MountGuard {
    pub fn new(source: &PathBuf, mountpoint: &PathBuf) -> Self {
        // Get the binary path
        let binary = env!("CARGO_BIN_EXE_fuse-passthrough");

        let child = Command::new(binary)
            .arg("-s")
            .arg(source)
            .arg("-m")
            .arg(mountpoint)
            .spawn()
            .expect("Failed to start fuse-passthrough");

        let guard = MountGuard {
            mountpoint: mountpoint.clone(),
            child: Some(child),
        };

        // Wait for mount to be ready
        if !guard.wait_for_mount() {
            panic!("Failed to mount filesystem at {:?}", mountpoint);
        }

        guard
    }

    fn wait_for_mount(&self) -> bool {
        let start = Instant::now();
        // Give the process a moment to start
        thread::sleep(Duration::from_millis(100));

        while start.elapsed() < MOUNT_TIMEOUT {
            // Check if we can list the directory - this means FUSE is responding
            if let Ok(entries) = self.mountpoint.read_dir() {
                // Try to actually iterate to confirm FUSE is working
                let _ = entries.count();
                return true;
            }
            thread::sleep(POLL_INTERVAL);
        }
        false
    }
}

impl Drop for MountGuard {
    fn drop(&mut self) {
        // Unmount
        let _ = Command::new("fusermount3")
            .arg("-u")
            .arg(&self.mountpoint)
            .output();

        // Kill the process if still running
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub fn setup_test_dirs() -> (PathBuf, PathBuf, tempfile::TempDir) {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let source = temp_dir.path().join("source");
    let mountpoint = temp_dir.path().join("mount");

    fs::create_dir_all(&source).expect("Failed to create source dir");
    fs::create_dir_all(&mountpoint).expect("Failed to create mountpoint dir");

    (source, mountpoint, temp_dir)
}

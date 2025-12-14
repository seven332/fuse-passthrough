use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

struct MountGuard {
    mountpoint: PathBuf,
    child: Option<Child>,
}

impl MountGuard {
    fn new(source: &PathBuf, mountpoint: &PathBuf) -> Self {
        // Get the binary path
        let binary = env!("CARGO_BIN_EXE_fuse-passthrough");
        
        let child = Command::new(binary)
            .arg("-s")
            .arg(source)
            .arg("-m")
            .arg(mountpoint)
            .spawn()
            .expect("Failed to start fuse-passthrough");

        // Wait longer for mount to complete
        thread::sleep(Duration::from_secs(2));

        MountGuard {
            mountpoint: mountpoint.clone(),
            child: Some(child),
        }
    }

    fn wait_for_mount(&self) -> bool {
        // Try to access the mountpoint to verify it's mounted
        for _ in 0..10 {
            if self.mountpoint.read_dir().is_ok() {
                return true;
            }
            thread::sleep(Duration::from_millis(200));
        }
        false
    }
}

impl Drop for MountGuard {
    fn drop(&mut self) {
        // Unmount
        let _ = Command::new("umount")
            .arg(&self.mountpoint)
            .output();

        // Wait a bit for unmount to complete
        thread::sleep(Duration::from_millis(500));

        // Kill the process if still running
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn setup_test_dirs() -> (PathBuf, PathBuf, tempfile::TempDir) {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let source = temp_dir.path().join("source");
    let mountpoint = temp_dir.path().join("mount");
    
    fs::create_dir_all(&source).expect("Failed to create source dir");
    fs::create_dir_all(&mountpoint).expect("Failed to create mountpoint dir");
    
    (source, mountpoint, temp_dir)
}

#[test]
fn test_read_file() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create a test file in source
    let test_content = "Hello, FUSE!";
    fs::write(source.join("test.txt"), test_content).expect("Failed to write test file");
    
    let guard = MountGuard::new(&source, &mountpoint);
    assert!(guard.wait_for_mount(), "Failed to mount filesystem");
    
    // Read from mountpoint
    let mut content = String::new();
    File::open(mountpoint.join("test.txt"))
        .expect("Failed to open file")
        .read_to_string(&mut content)
        .expect("Failed to read file");
    
    assert_eq!(content, test_content);
}

#[test]
fn test_write_file() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    let guard = MountGuard::new(&source, &mountpoint);
    assert!(guard.wait_for_mount(), "Failed to mount filesystem");
    
    // Write to mountpoint
    let test_content = "Written through FUSE";
    {
        let mut file = File::create(mountpoint.join("new.txt")).expect("Failed to create file");
        file.write_all(test_content.as_bytes()).expect("Failed to write file");
        file.sync_all().expect("Failed to sync file");
    }
    
    // Wait for sync
    thread::sleep(Duration::from_millis(500));
    
    // Verify in source
    let content = fs::read_to_string(source.join("new.txt")).expect("Failed to read from source");
    assert_eq!(content, test_content);
}

#[test]
fn test_list_directory() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create test files
    fs::write(source.join("file1.txt"), "content1").unwrap();
    fs::write(source.join("file2.txt"), "content2").unwrap();
    fs::create_dir(source.join("subdir")).unwrap();
    
    let guard = MountGuard::new(&source, &mountpoint);
    assert!(guard.wait_for_mount(), "Failed to mount filesystem");
    
    // List directory
    let entries: Vec<_> = fs::read_dir(&mountpoint)
        .expect("Failed to read directory")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    
    assert!(entries.contains(&"file1.txt".to_string()), "file1.txt not found in {:?}", entries);
    assert!(entries.contains(&"file2.txt".to_string()), "file2.txt not found in {:?}", entries);
    assert!(entries.contains(&"subdir".to_string()), "subdir not found in {:?}", entries);
}

#[test]
fn test_create_directory() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    let guard = MountGuard::new(&source, &mountpoint);
    assert!(guard.wait_for_mount(), "Failed to mount filesystem");
    
    // Create directory through mountpoint
    fs::create_dir(mountpoint.join("newdir")).expect("Failed to create directory");
    
    // Wait for sync
    thread::sleep(Duration::from_millis(500));
    
    // Verify in source
    assert!(source.join("newdir").is_dir(), "Directory not created in source");
}

#[test]
fn test_delete_file() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create test file
    fs::write(source.join("to_delete.txt"), "delete me").unwrap();
    
    let guard = MountGuard::new(&source, &mountpoint);
    assert!(guard.wait_for_mount(), "Failed to mount filesystem");
    
    // Delete through mountpoint
    fs::remove_file(mountpoint.join("to_delete.txt")).expect("Failed to delete file");
    
    // Wait for sync
    thread::sleep(Duration::from_millis(500));
    
    // Verify in source
    assert!(!source.join("to_delete.txt").exists(), "File still exists in source");
}

#[test]
fn test_rename_file() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create a test file in source
    let test_content = "Rename me!";
    fs::write(source.join("original.txt"), test_content).expect("Failed to write test file");
    
    let guard = MountGuard::new(&source, &mountpoint);
    assert!(guard.wait_for_mount(), "Failed to mount filesystem");
    
    // Rename through mountpoint
    fs::rename(
        mountpoint.join("original.txt"),
        mountpoint.join("renamed.txt"),
    ).expect("Failed to rename file");
    
    // Wait for sync
    thread::sleep(Duration::from_millis(500));
    
    // Verify in source: old file should not exist, new file should exist with same content
    assert!(!source.join("original.txt").exists(), "Original file still exists");
    assert!(source.join("renamed.txt").exists(), "Renamed file does not exist");
    
    let content = fs::read_to_string(source.join("renamed.txt")).expect("Failed to read renamed file");
    assert_eq!(content, test_content);
}

#[test]
fn test_delete_directory() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create a directory in source
    fs::create_dir(source.join("to_delete_dir")).expect("Failed to create directory");
    
    let guard = MountGuard::new(&source, &mountpoint);
    assert!(guard.wait_for_mount(), "Failed to mount filesystem");
    
    // Delete directory through mountpoint
    fs::remove_dir(mountpoint.join("to_delete_dir")).expect("Failed to delete directory");
    
    // Wait for sync
    thread::sleep(Duration::from_millis(500));
    
    // Verify in source
    assert!(!source.join("to_delete_dir").exists(), "Directory still exists in source");
}

#[test]
fn test_symlink() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create a target file
    let test_content = "Target content";
    fs::write(source.join("target.txt"), test_content).expect("Failed to write target file");
    
    let guard = MountGuard::new(&source, &mountpoint);
    assert!(guard.wait_for_mount(), "Failed to mount filesystem");
    
    // Create symlink through mountpoint
    std::os::unix::fs::symlink("target.txt", mountpoint.join("link.txt"))
        .expect("Failed to create symlink");
    
    // Wait for sync
    thread::sleep(Duration::from_millis(500));
    
    // Verify symlink exists in source
    let link_path = source.join("link.txt");
    assert!(link_path.is_symlink(), "Symlink not created in source");
    
    // Verify symlink target
    let target = fs::read_link(&link_path).expect("Failed to read symlink");
    assert_eq!(target.to_string_lossy(), "target.txt");
    
    // Read through symlink via mountpoint
    let content = fs::read_to_string(mountpoint.join("link.txt"))
        .expect("Failed to read through symlink");
    assert_eq!(content, test_content);
}

#[test]
fn test_file_permissions() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create a test file
    fs::write(source.join("perm_test.txt"), "test").expect("Failed to write test file");
    
    let guard = MountGuard::new(&source, &mountpoint);
    assert!(guard.wait_for_mount(), "Failed to mount filesystem");
    
    // Change permissions through mountpoint
    let new_mode = 0o644;
    fs::set_permissions(
        mountpoint.join("perm_test.txt"),
        fs::Permissions::from_mode(new_mode),
    ).expect("Failed to set permissions");
    
    // Wait for sync
    thread::sleep(Duration::from_millis(500));
    
    // Verify permissions in source
    let metadata = fs::metadata(source.join("perm_test.txt")).expect("Failed to get metadata");
    let mode = metadata.permissions().mode() & 0o777;
    assert_eq!(mode, new_mode, "Permissions not set correctly");
}

#[test]
fn test_truncate_file() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create a test file with content
    let original_content = "This is a long content that will be truncated";
    fs::write(source.join("truncate.txt"), original_content).expect("Failed to write test file");
    
    let guard = MountGuard::new(&source, &mountpoint);
    assert!(guard.wait_for_mount(), "Failed to mount filesystem");
    
    // Truncate file through mountpoint
    {
        let file = File::options()
            .write(true)
            .open(mountpoint.join("truncate.txt"))
            .expect("Failed to open file");
        file.set_len(10).expect("Failed to truncate file");
    }
    
    // Wait for sync
    thread::sleep(Duration::from_millis(500));
    
    // Verify truncation in source
    let content = fs::read_to_string(source.join("truncate.txt")).expect("Failed to read file");
    assert_eq!(content.len(), 10);
    assert_eq!(content, "This is a ");
}

#[test]
fn test_rename_across_directories() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create source directory structure
    fs::create_dir(source.join("dir1")).expect("Failed to create dir1");
    fs::create_dir(source.join("dir2")).expect("Failed to create dir2");
    let test_content = "Moving file";
    fs::write(source.join("dir1/file.txt"), test_content).expect("Failed to write test file");
    
    let guard = MountGuard::new(&source, &mountpoint);
    assert!(guard.wait_for_mount(), "Failed to mount filesystem");
    
    // Move file from dir1 to dir2 through mountpoint
    fs::rename(
        mountpoint.join("dir1/file.txt"),
        mountpoint.join("dir2/file.txt"),
    ).expect("Failed to rename file across directories");
    
    // Wait for sync
    thread::sleep(Duration::from_millis(500));
    
    // Verify in source
    assert!(!source.join("dir1/file.txt").exists(), "File still exists in dir1");
    assert!(source.join("dir2/file.txt").exists(), "File not found in dir2");
    
    let content = fs::read_to_string(source.join("dir2/file.txt")).expect("Failed to read file");
    assert_eq!(content, test_content);
}

#[test]
fn test_append_write() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create initial file
    let initial_content = "Initial content\n";
    fs::write(source.join("append.txt"), initial_content).expect("Failed to write test file");
    
    let guard = MountGuard::new(&source, &mountpoint);
    assert!(guard.wait_for_mount(), "Failed to mount filesystem");
    
    // Append to file through mountpoint
    let append_content = "Appended content";
    {
        let mut file = File::options()
            .append(true)
            .open(mountpoint.join("append.txt"))
            .expect("Failed to open file for append");
        file.write_all(append_content.as_bytes()).expect("Failed to append");
        file.sync_all().expect("Failed to sync");
    }
    
    // Wait for sync
    thread::sleep(Duration::from_millis(500));
    
    // Verify in source
    let content = fs::read_to_string(source.join("append.txt")).expect("Failed to read file");
    assert_eq!(content, format!("{}{}", initial_content, append_content));
}

#[test]
fn test_large_file() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    let guard = MountGuard::new(&source, &mountpoint);
    assert!(guard.wait_for_mount(), "Failed to mount filesystem");
    
    // Create a large file (1MB) through mountpoint
    let size = 1024 * 1024; // 1MB
    let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
    
    {
        let mut file = File::create(mountpoint.join("large.bin")).expect("Failed to create file");
        file.write_all(&data).expect("Failed to write large file");
        file.sync_all().expect("Failed to sync");
    }
    
    // Wait for sync
    thread::sleep(Duration::from_millis(500));
    
    // Verify file size in source
    let metadata = fs::metadata(source.join("large.bin")).expect("Failed to get metadata");
    assert_eq!(metadata.len(), size as u64, "File size mismatch");
    
    // Read back and verify content
    let read_data = fs::read(mountpoint.join("large.bin")).expect("Failed to read large file");
    assert_eq!(read_data.len(), size);
    assert_eq!(read_data, data, "Content mismatch in large file");
}

#[test]
fn test_seek_and_read() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create a test file
    let test_content = "0123456789ABCDEFGHIJ";
    fs::write(source.join("seek.txt"), test_content).expect("Failed to write test file");
    
    let guard = MountGuard::new(&source, &mountpoint);
    assert!(guard.wait_for_mount(), "Failed to mount filesystem");
    
    // Open file and seek to middle, then read
    let mut file = File::open(mountpoint.join("seek.txt")).expect("Failed to open file");
    file.seek(SeekFrom::Start(10)).expect("Failed to seek");
    
    let mut buffer = [0u8; 5];
    file.read_exact(&mut buffer).expect("Failed to read");
    
    assert_eq!(&buffer, b"ABCDE");
}

#[test]
fn test_nested_directories() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    let guard = MountGuard::new(&source, &mountpoint);
    assert!(guard.wait_for_mount(), "Failed to mount filesystem");
    
    // Create nested directories through mountpoint
    fs::create_dir_all(mountpoint.join("a/b/c")).expect("Failed to create nested directories");
    
    // Create a file in the nested directory
    let test_content = "Nested file content";
    fs::write(mountpoint.join("a/b/c/file.txt"), test_content).expect("Failed to write file");
    
    // Wait for sync
    thread::sleep(Duration::from_millis(500));
    
    // Verify in source
    assert!(source.join("a/b/c").is_dir(), "Nested directories not created");
    let content = fs::read_to_string(source.join("a/b/c/file.txt")).expect("Failed to read file");
    assert_eq!(content, test_content);
}

#[test]
fn test_file_metadata() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create a test file
    let test_content = "Metadata test content";
    fs::write(source.join("metadata.txt"), test_content).expect("Failed to write test file");
    
    let guard = MountGuard::new(&source, &mountpoint);
    assert!(guard.wait_for_mount(), "Failed to mount filesystem");
    
    // Get metadata through mountpoint
    let mount_metadata = fs::metadata(mountpoint.join("metadata.txt"))
        .expect("Failed to get metadata from mountpoint");
    let source_metadata = fs::metadata(source.join("metadata.txt"))
        .expect("Failed to get metadata from source");
    
    // Verify metadata matches
    assert_eq!(mount_metadata.len(), source_metadata.len(), "Size mismatch");
    assert_eq!(mount_metadata.is_file(), source_metadata.is_file(), "Type mismatch");
    assert_eq!(
        mount_metadata.permissions().mode() & 0o777,
        source_metadata.permissions().mode() & 0o777,
        "Permissions mismatch"
    );
}
mod common;

use common::{setup_test_dirs, wait_for, wait_for_dir, wait_for_file, wait_for_file_gone, MountGuard};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::PermissionsExt;

#[test]
fn test_read_file() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create a test file in source
    let test_content = "Hello, FUSE!";
    fs::write(source.join("test.txt"), test_content).expect("Failed to write test file");
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
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
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Write to mountpoint
    let test_content = "Written through FUSE";
    {
        let mut file = File::create(mountpoint.join("new.txt")).expect("Failed to create file");
        file.write_all(test_content.as_bytes()).expect("Failed to write file");
        file.sync_all().expect("Failed to sync file");
    }
    
    // Wait for file to appear in source
    assert!(wait_for_file(&source.join("new.txt")), "File not created in source");
    
    // Verify content
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
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
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
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Create directory through mountpoint
    fs::create_dir(mountpoint.join("newdir")).expect("Failed to create directory");
    
    // Wait for directory to appear in source
    assert!(wait_for_dir(&source.join("newdir")), "Directory not created in source");
}

#[test]
fn test_delete_file() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create test file
    fs::write(source.join("to_delete.txt"), "delete me").unwrap();
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Delete through mountpoint
    fs::remove_file(mountpoint.join("to_delete.txt")).expect("Failed to delete file");
    
    // Wait for file to be gone in source
    assert!(wait_for_file_gone(&source.join("to_delete.txt")), "File still exists in source");
}

#[test]
fn test_rename_file() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create a test file in source
    let test_content = "Rename me!";
    fs::write(source.join("original.txt"), test_content).expect("Failed to write test file");
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Rename through mountpoint
    fs::rename(
        mountpoint.join("original.txt"),
        mountpoint.join("renamed.txt"),
    ).expect("Failed to rename file");
    
    // Wait for rename to complete
    assert!(wait_for_file(&source.join("renamed.txt")), "Renamed file does not exist");
    assert!(wait_for_file_gone(&source.join("original.txt")), "Original file still exists");
    
    let content = fs::read_to_string(source.join("renamed.txt")).expect("Failed to read renamed file");
    assert_eq!(content, test_content);
}

#[test]
fn test_delete_directory() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create a directory in source
    fs::create_dir(source.join("to_delete_dir")).expect("Failed to create directory");
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Delete directory through mountpoint
    fs::remove_dir(mountpoint.join("to_delete_dir")).expect("Failed to delete directory");
    
    // Wait for directory to be gone
    assert!(wait_for_file_gone(&source.join("to_delete_dir")), "Directory still exists in source");
}

#[test]
fn test_symlink() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create a target file
    let test_content = "Target content";
    fs::write(source.join("target.txt"), test_content).expect("Failed to write target file");
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Create symlink through mountpoint
    std::os::unix::fs::symlink("target.txt", mountpoint.join("link.txt"))
        .expect("Failed to create symlink");
    
    // Wait for symlink to appear
    let link_path = source.join("link.txt");
    assert!(wait_for_file(&link_path), "Symlink not created in source");
    assert!(link_path.is_symlink(), "Path is not a symlink");
    
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
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Change permissions through mountpoint
    let new_mode = 0o644;
    fs::set_permissions(
        mountpoint.join("perm_test.txt"),
        fs::Permissions::from_mode(new_mode),
    ).expect("Failed to set permissions");
    
    // Wait for permissions to be updated
    let source_file = source.join("perm_test.txt");
    assert!(wait_for(|| {
        if let Ok(metadata) = fs::metadata(&source_file) {
            (metadata.permissions().mode() & 0o777) == new_mode
        } else {
            false
        }
    }), "Permissions not set correctly");
}

#[test]
fn test_truncate_file() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create a test file with content
    let original_content = "This is a long content that will be truncated";
    fs::write(source.join("truncate.txt"), original_content).expect("Failed to write test file");
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Truncate file through mountpoint
    {
        let file = File::options()
            .write(true)
            .open(mountpoint.join("truncate.txt"))
            .expect("Failed to open file");
        file.set_len(10).expect("Failed to truncate file");
    }
    
    // Wait for truncation to complete
    let source_file = source.join("truncate.txt");
    assert!(wait_for(|| {
        if let Ok(metadata) = fs::metadata(&source_file) {
            metadata.len() == 10
        } else {
            false
        }
    }), "File not truncated");
    
    // Verify content
    let content = fs::read_to_string(&source_file).expect("Failed to read file");
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
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Move file from dir1 to dir2 through mountpoint
    fs::rename(
        mountpoint.join("dir1/file.txt"),
        mountpoint.join("dir2/file.txt"),
    ).expect("Failed to rename file across directories");
    
    // Wait for move to complete
    assert!(wait_for_file(&source.join("dir2/file.txt")), "File not found in dir2");
    assert!(wait_for_file_gone(&source.join("dir1/file.txt")), "File still exists in dir1");
    
    let content = fs::read_to_string(source.join("dir2/file.txt")).expect("Failed to read file");
    assert_eq!(content, test_content);
}

#[test]
fn test_append_write() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create initial file
    let initial_content = "Initial content\n";
    fs::write(source.join("append.txt"), initial_content).expect("Failed to write test file");
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
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
    
    // Wait for append to complete
    let expected_len = initial_content.len() + append_content.len();
    let source_file = source.join("append.txt");
    assert!(wait_for(|| {
        if let Ok(metadata) = fs::metadata(&source_file) {
            metadata.len() == expected_len as u64
        } else {
            false
        }
    }), "Append not completed");
    
    // Verify content
    let content = fs::read_to_string(&source_file).expect("Failed to read file");
    assert_eq!(content, format!("{}{}", initial_content, append_content));
}

#[test]
fn test_large_file() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Create a large file (1MB) through mountpoint
    let size = 1024 * 1024; // 1MB
    let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
    
    {
        let mut file = File::create(mountpoint.join("large.bin")).expect("Failed to create file");
        file.write_all(&data).expect("Failed to write large file");
        file.sync_all().expect("Failed to sync");
    }
    
    // Wait for file to be fully written
    let source_file = source.join("large.bin");
    assert!(wait_for(|| {
        if let Ok(metadata) = fs::metadata(&source_file) {
            metadata.len() == size as u64
        } else {
            false
        }
    }), "Large file not fully written");
    
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
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
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
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Create nested directories through mountpoint
    fs::create_dir_all(mountpoint.join("a/b/c")).expect("Failed to create nested directories");
    
    // Create a file in the nested directory
    let test_content = "Nested file content";
    fs::write(mountpoint.join("a/b/c/file.txt"), test_content).expect("Failed to write file");
    
    // Wait for file to appear
    assert!(wait_for_file(&source.join("a/b/c/file.txt")), "Nested file not created");
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
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
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

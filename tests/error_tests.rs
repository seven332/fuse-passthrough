mod common;

use common::{setup_test_dirs, MountGuard};
use std::fs::{self, File};

#[test]
fn test_read_nonexistent_file() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Try to read a file that doesn't exist
    let result = File::open(mountpoint.join("nonexistent.txt"));
    assert!(result.is_err(), "Expected error when opening nonexistent file");
    
    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn test_delete_nonexistent_file() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Try to delete a file that doesn't exist
    let result = fs::remove_file(mountpoint.join("nonexistent.txt"));
    assert!(result.is_err(), "Expected error when deleting nonexistent file");
    
    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn test_delete_nonexistent_directory() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Try to delete a directory that doesn't exist
    let result = fs::remove_dir(mountpoint.join("nonexistent_dir"));
    assert!(result.is_err(), "Expected error when deleting nonexistent directory");
    
    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn test_delete_nonempty_directory() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create a directory with a file inside
    fs::create_dir(source.join("nonempty")).expect("Failed to create directory");
    fs::write(source.join("nonempty/file.txt"), "content").expect("Failed to write file");
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Try to delete non-empty directory (should fail)
    let result = fs::remove_dir(mountpoint.join("nonempty"));
    assert!(result.is_err(), "Expected error when deleting non-empty directory");
}

#[test]
fn test_create_file_in_nonexistent_directory() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Try to create a file in a directory that doesn't exist
    let result = File::create(mountpoint.join("nonexistent_dir/file.txt"));
    assert!(result.is_err(), "Expected error when creating file in nonexistent directory");
    
    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn test_rename_nonexistent_file() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Try to rename a file that doesn't exist
    let result = fs::rename(
        mountpoint.join("nonexistent.txt"),
        mountpoint.join("new_name.txt"),
    );
    assert!(result.is_err(), "Expected error when renaming nonexistent file");
    
    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn test_read_directory_as_file() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create a directory
    fs::create_dir(source.join("testdir")).expect("Failed to create directory");
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Try to read a directory as a file
    // Note: File::open may succeed on directories in FUSE, but reading should fail
    let result = fs::read_to_string(mountpoint.join("testdir"));
    assert!(result.is_err(), "Expected error when reading directory as file");
}

#[test]
fn test_create_directory_that_exists() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create a directory
    fs::create_dir(source.join("existing")).expect("Failed to create directory");
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Try to create a directory that already exists
    let result = fs::create_dir(mountpoint.join("existing"));
    assert!(result.is_err(), "Expected error when creating existing directory");
    
    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
}

#[test]
fn test_read_symlink_to_nonexistent_target() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    // Create a broken symlink directly in source (before mounting)
    std::os::unix::fs::symlink("nonexistent_target.txt", source.join("broken_link"))
        .expect("Failed to create symlink");
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Try to read through the broken symlink via mountpoint
    let result = fs::read_to_string(mountpoint.join("broken_link"));
    assert!(result.is_err(), "Expected error when reading through broken symlink");
    
    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn test_getattr_nonexistent_file() {
    let (source, mountpoint, _temp_dir) = setup_test_dirs();
    
    let _guard = MountGuard::new(&source, &mountpoint);
    
    // Try to get metadata of a nonexistent file
    let result = fs::metadata(mountpoint.join("nonexistent.txt"));
    assert!(result.is_err(), "Expected error for nonexistent file metadata");
    
    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

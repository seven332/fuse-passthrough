use clap::Parser;
use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    ReplyOpen, ReplyWrite, Request, TimeOrNow,
};
use libc::{ENOENT, ENOSYS};
use log::{debug, error, info};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TTL: Duration = Duration::from_secs(1);

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Source directory path (the directory to be mirrored)
    #[arg(short, long)]
    source: String,

    /// Mountpoint path (where the source will be mounted)
    #[arg(short, long)]
    mountpoint: String,

    /// Allow other users to access the mounted filesystem
    #[arg(long, default_value = "false")]
    allow_other: bool,
}

/// Passthrough filesystem implementation
struct PassthroughFS {
    /// Source directory path
    source: PathBuf,
    /// Inode to path mapping
    inode_to_path: Mutex<HashMap<u64, PathBuf>>,
    /// Path to inode mapping
    path_to_inode: Mutex<HashMap<PathBuf, u64>>,
    /// Next available inode number
    next_inode: AtomicU64,
    /// Open file handles
    open_files: Mutex<HashMap<u64, File>>,
    /// Next available file handle
    next_fh: AtomicU64,
}

impl PassthroughFS {
    fn new(source: PathBuf) -> Self {
        let mut inode_to_path = HashMap::new();
        let mut path_to_inode = HashMap::new();

        // Root directory inode is 1
        inode_to_path.insert(1, PathBuf::from(""));
        path_to_inode.insert(PathBuf::from(""), 1);

        PassthroughFS {
            source,
            inode_to_path: Mutex::new(inode_to_path),
            path_to_inode: Mutex::new(path_to_inode),
            next_inode: AtomicU64::new(2),
            open_files: Mutex::new(HashMap::new()),
            next_fh: AtomicU64::new(1),
        }
    }

    /// Get the real path on the underlying filesystem
    fn real_path(&self, relative: &Path) -> PathBuf {
        self.source.join(relative)
    }

    /// Get relative path by inode
    fn get_path(&self, inode: u64) -> Option<PathBuf> {
        self.inode_to_path.lock().unwrap().get(&inode).cloned()
    }

    /// Allocate or get inode for a path
    fn get_or_create_inode(&self, path: &Path) -> u64 {
        let mut path_to_inode = self.path_to_inode.lock().unwrap();
        if let Some(&inode) = path_to_inode.get(path) {
            return inode;
        }

        let inode = self.next_inode.fetch_add(1, Ordering::SeqCst);
        path_to_inode.insert(path.to_path_buf(), inode);
        self.inode_to_path
            .lock()
            .unwrap()
            .insert(inode, path.to_path_buf());
        inode
    }

    /// Convert std::fs::Metadata to FileAttr
    fn metadata_to_attr(&self, metadata: &fs::Metadata, inode: u64) -> FileAttr {
        let kind = if metadata.is_dir() {
            FileType::Directory
        } else if metadata.is_symlink() {
            FileType::Symlink
        } else {
            FileType::RegularFile
        };

        let atime = metadata
            .accessed()
            .unwrap_or(UNIX_EPOCH);
        let mtime = metadata
            .modified()
            .unwrap_or(UNIX_EPOCH);
        let ctime = SystemTime::UNIX_EPOCH + Duration::from_secs(metadata.ctime() as u64);

        FileAttr {
            ino: inode,
            size: metadata.size(),
            blocks: metadata.blocks(),
            atime,
            mtime,
            ctime,
            crtime: UNIX_EPOCH,
            kind,
            perm: (metadata.mode() & 0o7777) as u16,
            nlink: metadata.nlink() as u32,
            uid: metadata.uid(),
            gid: metadata.gid(),
            rdev: metadata.rdev() as u32,
            blksize: metadata.blksize() as u32,
            flags: 0,
        }
    }
}

impl Filesystem for PassthroughFS {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        debug!("lookup: parent={}, name={:?}", parent, name);

        let parent_path = match self.get_path(parent) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let relative_path = parent_path.join(name);
        let real_path = self.real_path(&relative_path);

        match fs::metadata(&real_path) {
            Ok(metadata) => {
                let inode = self.get_or_create_inode(&relative_path);
                let attr = self.metadata_to_attr(&metadata, inode);
                reply.entry(&TTL, &attr, 0);
            }
            Err(_) => {
                reply.error(ENOENT);
            }
        }
    }

    fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        debug!("getattr: ino={}", ino);

        let path = match self.get_path(ino) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let real_path = self.real_path(&path);

        match fs::metadata(&real_path) {
            Ok(metadata) => {
                let attr = self.metadata_to_attr(&metadata, ino);
                reply.attr(&TTL, &attr);
            }
            Err(_) => {
                reply.error(ENOENT);
            }
        }
    }

    fn setattr(
        &mut self,
        _req: &Request,
        ino: u64,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<TimeOrNow>,
        _mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        _fh: Option<u64>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        debug!("setattr: ino={}", ino);

        let path = match self.get_path(ino) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let real_path = self.real_path(&path);

        // Handle file truncation
        if let Some(new_size) = size {
            if let Ok(file) = OpenOptions::new().write(true).open(&real_path) {
                let _ = file.set_len(new_size);
            }
        }

        // Handle permission change
        if let Some(new_mode) = mode {
            let _ = fs::set_permissions(&real_path, fs::Permissions::from_mode(new_mode));
        }

        // Handle uid/gid change
        if uid.is_some() || gid.is_some() {
            let uid = uid.unwrap_or(u32::MAX);
            let gid = gid.unwrap_or(u32::MAX);
            unsafe {
                let path_cstr = std::ffi::CString::new(real_path.to_str().unwrap()).unwrap();
                libc::chown(path_cstr.as_ptr(), uid, gid);
            }
        }

        // Return updated attributes
        match fs::metadata(&real_path) {
            Ok(metadata) => {
                let attr = self.metadata_to_attr(&metadata, ino);
                reply.attr(&TTL, &attr);
            }
            Err(_) => {
                reply.error(ENOENT);
            }
        }
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        debug!("read: ino={}, fh={}, offset={}, size={}", ino, fh, offset, size);

        let mut open_files = self.open_files.lock().unwrap();
        if let Some(file) = open_files.get_mut(&fh) {
            let mut buffer = vec![0u8; size as usize];
            if file.seek(SeekFrom::Start(offset as u64)).is_ok() {
                match file.read(&mut buffer) {
                    Ok(bytes_read) => {
                        reply.data(&buffer[..bytes_read]);
                        return;
                    }
                    Err(e) => {
                        error!("read error: {:?}", e);
                    }
                }
            }
        }
        reply.error(ENOENT);
    }

    fn write(
        &mut self,
        _req: &Request,
        ino: u64,
        fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        debug!("write: ino={}, fh={}, offset={}, size={}", ino, fh, offset, data.len());

        let mut open_files = self.open_files.lock().unwrap();
        if let Some(file) = open_files.get_mut(&fh) {
            if file.seek(SeekFrom::Start(offset as u64)).is_ok() {
                match file.write(data) {
                    Ok(bytes_written) => {
                        reply.written(bytes_written as u32);
                        return;
                    }
                    Err(e) => {
                        error!("write error: {:?}", e);
                    }
                }
            }
        }
        reply.error(ENOENT);
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        debug!("readdir: ino={}, offset={}", ino, offset);

        let path = match self.get_path(ino) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let real_path = self.real_path(&path);

        let entries = match fs::read_dir(&real_path) {
            Ok(entries) => entries,
            Err(_) => {
                reply.error(ENOENT);
                return;
            }
        };

        let mut all_entries: Vec<_> = vec![
            (ino, FileType::Directory, ".".to_string()),
            (ino, FileType::Directory, "..".to_string()),
        ];

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let relative_path = path.join(&name);
            let child_inode = self.get_or_create_inode(&relative_path);
            
            let file_type = if let Ok(metadata) = entry.metadata() {
                if metadata.is_dir() {
                    FileType::Directory
                } else if metadata.is_symlink() {
                    FileType::Symlink
                } else {
                    FileType::RegularFile
                }
            } else {
                FileType::RegularFile
            };

            all_entries.push((child_inode, file_type, name));
        }

        for (i, (inode, file_type, name)) in all_entries.iter().enumerate().skip(offset as usize) {
            if reply.add(*inode, (i + 1) as i64, *file_type, name) {
                break;
            }
        }

        reply.ok();
    }

    fn open(&mut self, _req: &Request, ino: u64, flags: i32, reply: ReplyOpen) {
        debug!("open: ino={}, flags={}", ino, flags);

        let path = match self.get_path(ino) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let real_path = self.real_path(&path);

        let read = (flags & libc::O_ACCMODE) == libc::O_RDONLY
            || (flags & libc::O_ACCMODE) == libc::O_RDWR;
        let write = (flags & libc::O_ACCMODE) == libc::O_WRONLY
            || (flags & libc::O_ACCMODE) == libc::O_RDWR;

        match OpenOptions::new()
            .read(read)
            .write(write)
            .append((flags & libc::O_APPEND) != 0)
            .open(&real_path)
        {
            Ok(file) => {
                let fh = self.next_fh.fetch_add(1, Ordering::SeqCst);
                self.open_files.lock().unwrap().insert(fh, file);
                reply.opened(fh, 0);
            }
            Err(e) => {
                error!("open error: {:?}", e);
                reply.error(ENOENT);
            }
        }
    }

    fn release(
        &mut self,
        _req: &Request,
        _ino: u64,
        fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: fuser::ReplyEmpty,
    ) {
        debug!("release: fh={}", fh);
        self.open_files.lock().unwrap().remove(&fh);
        reply.ok();
    }

    fn create(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        flags: i32,
        reply: fuser::ReplyCreate,
    ) {
        debug!("create: parent={}, name={:?}, mode={}", parent, name, mode);

        let parent_path = match self.get_path(parent) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let relative_path = parent_path.join(name);
        let real_path = self.real_path(&relative_path);

        let read = (flags & libc::O_ACCMODE) == libc::O_RDONLY
            || (flags & libc::O_ACCMODE) == libc::O_RDWR;
        let write = (flags & libc::O_ACCMODE) == libc::O_WRONLY
            || (flags & libc::O_ACCMODE) == libc::O_RDWR;

        match OpenOptions::new()
            .read(read)
            .write(write)
            .create(true)
            .truncate((flags & libc::O_TRUNC) != 0)
            .open(&real_path)
        {
            Ok(file) => {
                // Set permissions
                let _ = fs::set_permissions(&real_path, fs::Permissions::from_mode(mode));

                let inode = self.get_or_create_inode(&relative_path);
                let fh = self.next_fh.fetch_add(1, Ordering::SeqCst);
                self.open_files.lock().unwrap().insert(fh, file);

                match fs::metadata(&real_path) {
                    Ok(metadata) => {
                        let attr = self.metadata_to_attr(&metadata, inode);
                        reply.created(&TTL, &attr, 0, fh, 0);
                    }
                    Err(_) => {
                        reply.error(ENOENT);
                    }
                }
            }
            Err(e) => {
                error!("create error: {:?}", e);
                reply.error(ENOENT);
            }
        }
    }

    fn mkdir(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        debug!("mkdir: parent={}, name={:?}, mode={}", parent, name, mode);

        let parent_path = match self.get_path(parent) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let relative_path = parent_path.join(name);
        let real_path = self.real_path(&relative_path);

        match fs::create_dir(&real_path) {
            Ok(_) => {
                let _ = fs::set_permissions(&real_path, fs::Permissions::from_mode(mode));
                let inode = self.get_or_create_inode(&relative_path);
                match fs::metadata(&real_path) {
                    Ok(metadata) => {
                        let attr = self.metadata_to_attr(&metadata, inode);
                        reply.entry(&TTL, &attr, 0);
                    }
                    Err(_) => {
                        reply.error(ENOENT);
                    }
                }
            }
            Err(e) => {
                error!("mkdir error: {:?}", e);
                reply.error(libc::EEXIST);
            }
        }
    }

    fn unlink(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: fuser::ReplyEmpty) {
        debug!("unlink: parent={}, name={:?}", parent, name);

        let parent_path = match self.get_path(parent) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let relative_path = parent_path.join(name);
        let real_path = self.real_path(&relative_path);

        match fs::remove_file(&real_path) {
            Ok(_) => {
                // Clean up inode mapping
                if let Some(inode) = self.path_to_inode.lock().unwrap().remove(&relative_path) {
                    self.inode_to_path.lock().unwrap().remove(&inode);
                }
                reply.ok();
            }
            Err(e) => {
                error!("unlink error: {:?}", e);
                reply.error(ENOENT);
            }
        }
    }

    fn rmdir(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: fuser::ReplyEmpty) {
        debug!("rmdir: parent={}, name={:?}", parent, name);

        let parent_path = match self.get_path(parent) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let relative_path = parent_path.join(name);
        let real_path = self.real_path(&relative_path);

        match fs::remove_dir(&real_path) {
            Ok(_) => {
                // Clean up inode mapping
                if let Some(inode) = self.path_to_inode.lock().unwrap().remove(&relative_path) {
                    self.inode_to_path.lock().unwrap().remove(&inode);
                }
                reply.ok();
            }
            Err(e) => {
                error!("rmdir error: {:?}", e);
                reply.error(ENOENT);
            }
        }
    }

    fn rename(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &OsStr,
        newparent: u64,
        newname: &OsStr,
        _flags: u32,
        reply: fuser::ReplyEmpty,
    ) {
        debug!(
            "rename: parent={}, name={:?}, newparent={}, newname={:?}",
            parent, name, newparent, newname
        );

        let parent_path = match self.get_path(parent) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let newparent_path = match self.get_path(newparent) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let old_relative = parent_path.join(name);
        let new_relative = newparent_path.join(newname);
        let old_real = self.real_path(&old_relative);
        let new_real = self.real_path(&new_relative);

        match fs::rename(&old_real, &new_real) {
            Ok(_) => {
                // Update inode mapping - use a single lock scope to avoid deadlock
                {
                    let mut path_to_inode = self.path_to_inode.lock().unwrap();
                    if let Some(inode) = path_to_inode.remove(&old_relative) {
                        path_to_inode.insert(new_relative.clone(), inode);
                        self.inode_to_path.lock().unwrap().insert(inode, new_relative);
                    }
                }
                reply.ok();
            }
            Err(e) => {
                error!("rename error: {:?}", e);
                reply.error(ENOENT);
            }
        }
    }

    fn statfs(&mut self, _req: &Request, _ino: u64, reply: fuser::ReplyStatfs) {
        reply.statfs(0, 0, 0, 0, 0, 512, 255, 0);
    }

    /// macOS only: Exchange two files atomically
    #[cfg(target_os = "macos")]
    fn exchange(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &OsStr,
        newparent: u64,
        newname: &OsStr,
        _options: u64,
        reply: fuser::ReplyEmpty,
    ) {
        debug!(
            "exchange: parent={}, name={:?}, newparent={}, newname={:?}",
            parent, name, newparent, newname
        );

        // For non-atomic exchange, just do a rename
        let parent_path = match self.get_path(parent) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let newparent_path = match self.get_path(newparent) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let old_relative = parent_path.join(name);
        let new_relative = newparent_path.join(newname);
        let old_real = self.real_path(&old_relative);
        let new_real = self.real_path(&new_relative);

        match fs::rename(&old_real, &new_real) {
            Ok(_) => {
                // Update inode mapping - use a single lock scope to avoid deadlock
                {
                    let mut path_to_inode = self.path_to_inode.lock().unwrap();
                    if let Some(inode) = path_to_inode.remove(&old_relative) {
                        path_to_inode.insert(new_relative.clone(), inode);
                        self.inode_to_path.lock().unwrap().insert(inode, new_relative);
                    }
                }
                reply.ok();
            }
            Err(e) => {
                error!("exchange error: {:?}", e);
                reply.error(ENOENT);
            }
        }
    }

    fn access(&mut self, _req: &Request, ino: u64, mask: i32, reply: fuser::ReplyEmpty) {
        debug!("access: ino={}, mask={}", ino, mask);

        let path = match self.get_path(ino) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let real_path = self.real_path(&path);

        if real_path.exists() {
            reply.ok();
        } else {
            reply.error(ENOENT);
        }
    }

    fn readlink(&mut self, _req: &Request, ino: u64, reply: ReplyData) {
        debug!("readlink: ino={}", ino);

        let path = match self.get_path(ino) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let real_path = self.real_path(&path);

        match fs::read_link(&real_path) {
            Ok(target) => {
                reply.data(target.to_string_lossy().as_bytes());
            }
            Err(_) => {
                reply.error(ENOENT);
            }
        }
    }

    fn symlink(
        &mut self,
        _req: &Request,
        parent: u64,
        link_name: &OsStr,
        target: &Path,
        reply: ReplyEntry,
    ) {
        debug!("symlink: parent={}, name={:?}, target={:?}", parent, link_name, target);

        let parent_path = match self.get_path(parent) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let relative_path = parent_path.join(link_name);
        let real_path = self.real_path(&relative_path);

        match std::os::unix::fs::symlink(target, &real_path) {
            Ok(_) => {
                let inode = self.get_or_create_inode(&relative_path);
                match fs::symlink_metadata(&real_path) {
                    Ok(metadata) => {
                        let attr = self.metadata_to_attr(&metadata, inode);
                        reply.entry(&TTL, &attr, 0);
                    }
                    Err(_) => {
                        reply.error(ENOENT);
                    }
                }
            }
            Err(e) => {
                error!("symlink error: {:?}", e);
                reply.error(ENOSYS);
            }
        }
    }

    fn flush(
        &mut self,
        _req: &Request,
        _ino: u64,
        fh: u64,
        _lock_owner: u64,
        reply: fuser::ReplyEmpty,
    ) {
        debug!("flush: fh={}", fh);
        if let Some(file) = self.open_files.lock().unwrap().get_mut(&fh) {
            let _ = file.sync_all();
        }
        reply.ok();
    }

    fn fsync(
        &mut self,
        _req: &Request,
        _ino: u64,
        fh: u64,
        _datasync: bool,
        reply: fuser::ReplyEmpty,
    ) {
        debug!("fsync: fh={}", fh);
        if let Some(file) = self.open_files.lock().unwrap().get_mut(&fh) {
            let _ = file.sync_all();
        }
        reply.ok();
    }
}

fn main() {
    env_logger::init();

    let args = Args::parse();

    let source = PathBuf::from(&args.source);
    let mountpoint = PathBuf::from(&args.mountpoint);

    // Verify source directory exists
    if !source.exists() || !source.is_dir() {
        eprintln!("Error: source directory '{}' does not exist or is not a directory", args.source);
        std::process::exit(1);
    }

    // Verify mountpoint exists
    if !mountpoint.exists() || !mountpoint.is_dir() {
        eprintln!("Error: mountpoint '{}' does not exist or is not a directory", args.mountpoint);
        std::process::exit(1);
    }

    let source = source.canonicalize().expect("Failed to get absolute path for source directory");
    let mountpoint = mountpoint.canonicalize().expect("Failed to get absolute path for mountpoint");

    info!("Mounting {} to {}", source.display(), mountpoint.display());

    let fs = PassthroughFS::new(source);

    let mut options = vec![
        MountOption::RW,
        MountOption::FSName("passthrough".to_string()),
        MountOption::AutoUnmount,
    ];

    if args.allow_other {
        options.push(MountOption::AllowOther);
    }

    println!("Mounting filesystem...");
    println!("Source: {}", args.source);
    println!("Mountpoint: {}", args.mountpoint);
    println!("Press Ctrl+C to unmount and exit");

    // Use background session for mounting, allowing controlled unmount
    let session = match fuser::spawn_mount2(fs, &mountpoint, &options) {
        Ok(session) => session,
        Err(e) => {
            eprintln!("Mount failed: {}", e);
            std::process::exit(1);
        }
    };

    println!("Filesystem mounted");

    // Set up Ctrl+C signal handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    let mp = mountpoint.clone();

    ctrlc::set_handler(move || {
        println!("\nReceived Ctrl+C, unmounting...");
        r.store(false, Ordering::SeqCst);
    })
    .expect("Failed to set Ctrl+C handler");

    // Wait for exit signal
    while running.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(100));
    }

    // Unmount filesystem
    drop(session);

    // Ensure unmount completes
    let _ = std::process::Command::new("umount")
        .arg(&mp)
        .output();

    println!("Filesystem unmounted, exiting");
}

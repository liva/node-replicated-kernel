//! The core module for file management.

use crate::arch::process::UserSlice;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use core::sync::atomic::{AtomicUsize, Ordering};

use custom_error::custom_error;
use hashbrown::HashMap;

use kpi::io::*;
use kpi::SystemCallError;

pub use crate::fs::mnode::{MemNode, NodeType};

mod file;
mod mnode;
#[cfg(test)]
mod test;

/// The maximum number of open files for a process.
pub const MAX_FILES_PER_PROCESS: usize = 4096;

/// Mnode number.
pub type Mnode = u64;
/// Flags for fs calls.
pub type Flags = u64;
/// Modes for fs calls
pub type Modes = u64;
/// File descriptor.
pub type FD = u64;
/// Userspace buffer pointer to read or write a file.
pub type Buffer = u64;
/// Number of bytes to read or write a file.
pub type Len = u64;
/// Userspace-pointer to filename.
pub type Filename = u64;
/// File offset
pub type Offset = i64;

custom_error! {
    #[derive(PartialEq, Clone)]
    pub FileSystemError
    InvalidFileDescriptor = "Supplied file descriptor was invalid",
    InvalidFile = "Supplied file was invalid",
    InvalidFlags = "Supplied flags were invalid",
    InvalidOffset = "Supplied offset was invalid",
    PermissionError = "File/directory can't be read or written",
    AlreadyPresent = "Fd/File already exists",
    DirectoryError = "Can't read or write to a directory",
    OpenFileLimit = "Maximum files are opened for a process",
    OutOfMemory = "Unable to allocate memory for file",
}

impl Into<SystemCallError> for FileSystemError {
    fn into(self) -> SystemCallError {
        match self {
            FileSystemError::InvalidFileDescriptor => SystemCallError::BadFileDescriptor,
            FileSystemError::InvalidFile => SystemCallError::BadFileDescriptor,
            FileSystemError::InvalidFlags => SystemCallError::BadFlags,
            FileSystemError::InvalidOffset => SystemCallError::PermissionError,
            FileSystemError::PermissionError => SystemCallError::PermissionError,
            FileSystemError::AlreadyPresent => SystemCallError::PermissionError,
            FileSystemError::DirectoryError => SystemCallError::PermissionError,
            FileSystemError::OpenFileLimit => SystemCallError::OutOfMemory,
            FileSystemError::OutOfMemory => SystemCallError::OutOfMemory,
        }
    }
}

/// Abstract definition of file-system interface operations.
pub trait FileSystem {
    fn create(&mut self, pathname: &str, modes: Modes) -> Result<u64, FileSystemError>;
    fn write(
        &mut self,
        mnode_num: Mnode,
        buffer: &[u8],
        offset: usize,
    ) -> Result<usize, FileSystemError>;
    fn read(
        &self,
        mnode_num: Mnode,
        buffer: &mut UserSlice,
        offset: usize,
    ) -> Result<usize, FileSystemError>;
    fn lookup(&self, pathname: &str) -> Option<Arc<Mnode>>;
    fn file_info(&self, mnode: Mnode) -> FileInfo;
    fn delete(&mut self, pathname: &str) -> Result<bool, FileSystemError>;
    fn truncate(&mut self, pathname: &str) -> Result<bool, FileSystemError>;
    fn rename(&mut self, oldname: &str, newname: &str) -> Result<bool, FileSystemError>;
    fn mkdir(&mut self, pathname: &str, modes: Modes) -> Result<bool, FileSystemError>;
}

/// Abstract definition of a file descriptor.
pub trait FileDescriptor {
    fn init_fd() -> Fd;
    fn update_fd(&mut self, mnode: Mnode, flags: FileFlags);
    fn get_mnode(&self) -> Mnode;
    fn get_flags(&self) -> FileFlags;
    fn get_offset(&self) -> usize;
    fn update_offset(&self, new_offset: usize);
}

/// A file descriptor representaion.
#[derive(Debug, Default)]
pub struct Fd {
    mnode: Mnode,
    flags: FileFlags,
    offset: AtomicUsize,
}

impl FileDescriptor for Fd {
    fn init_fd() -> Fd {
        Fd {
            // Intial values are just the place-holders and shouldn't be used.
            mnode: core::u64::MAX,
            flags: Default::default(),
            offset: AtomicUsize::new(0),
        }
    }

    fn update_fd(&mut self, mnode: Mnode, flags: FileFlags) {
        self.mnode = mnode;
        self.flags = flags;
    }

    fn get_mnode(&self) -> Mnode {
        self.mnode.clone()
    }

    fn get_flags(&self) -> FileFlags {
        self.flags.clone()
    }

    fn get_offset(&self) -> usize {
        self.offset.load(Ordering::Relaxed)
    }

    fn update_offset(&self, new_offset: usize) {
        self.offset.store(new_offset, Ordering::Release);
    }
}

/// The in-memory file-system representation.
#[derive(Debug)]
pub struct MemFS {
    mnodes: HashMap<Mnode, MemNode>,
    files: HashMap<String, Arc<Mnode>>,
    root: (String, Mnode),
    nextmemnode: AtomicUsize,
}

impl MemFS {
    /// Get the next available memnode number.
    fn get_next_mno(&mut self) -> usize {
        self.nextmemnode.fetch_add(1, Ordering::Relaxed)
    }
}

impl Default for MemFS {
    /// Initialize the file system from the root directory.
    fn default() -> MemFS {
        let rootdir = "/";
        let rootmnode = 1;

        let mut mnodes = HashMap::new();
        mnodes.insert(
            rootmnode,
            MemNode::new(
                rootmnode,
                rootdir,
                FileModes::S_IRWXU.into(),
                NodeType::Directory,
            )
            .unwrap(),
        );
        let mut files = HashMap::new();
        files.insert(rootdir.to_string(), Arc::new(1));
        let root = (rootdir.to_string(), 1);

        MemFS {
            mnodes,
            files,
            root,
            nextmemnode: AtomicUsize::new(2),
        }
    }
}

impl FileSystem for MemFS {
    /// Create a file relative to the root directory.
    fn create(&mut self, pathname: &str, modes: Modes) -> Result<u64, FileSystemError> {
        // Check if the file with the same name already exists.
        match self.files.get(&pathname.to_string()) {
            Some(_) => return Err(FileSystemError::AlreadyPresent),
            None => {}
        }

        let mnode_num = self.get_next_mno() as u64;
        //TODO: For now all newly created mnode are for file. How to differentiate
        // between a file and a directory. Take input from the user?
        let memnode = match MemNode::new(mnode_num, pathname, modes, NodeType::File) {
            Ok(memnode) => memnode,
            Err(e) => return Err(e),
        };
        self.files.insert(pathname.to_string(), Arc::new(mnode_num));
        self.mnodes.insert(mnode_num, memnode);

        Ok(mnode_num)
    }

    /// Write data to a file.
    fn write(
        &mut self,
        mnode_num: Mnode,
        buffer: &[u8],
        offset: usize,
    ) -> Result<usize, FileSystemError> {
        match self.mnodes.get_mut(&mnode_num) {
            Some(mnode) => mnode.write(buffer, offset),
            None => Err(FileSystemError::InvalidFile),
        }
    }

    /// Read data from a file.
    fn read(
        &self,
        mnode_num: Mnode,
        buffer: &mut UserSlice,
        offset: usize,
    ) -> Result<usize, FileSystemError> {
        match self.mnodes.get(&mnode_num) {
            Some(mnode) => mnode.read(buffer, offset),
            None => Err(FileSystemError::InvalidFile),
        }
    }

    /// Check if a file exists in the file system or not.
    fn lookup(&self, pathname: &str) -> Option<Arc<Mnode>> {
        self.files
            .get(&pathname.to_string())
            .map(|mnode| Arc::clone(mnode))
    }

    /// Find the size and type by giving the mnode number.
    fn file_info(&self, mnode: Mnode) -> FileInfo {
        match self.mnodes.get(&mnode) {
            Some(mnode) => match mnode.get_mnode_type() {
                NodeType::Directory => FileInfo {
                    fsize: 0,
                    ftype: NodeType::Directory.into(),
                },
                NodeType::File => FileInfo {
                    fsize: mnode.get_file_size() as u64,
                    ftype: NodeType::File.into(),
                },
            },
            None => unreachable!("file_info: shouldn't reach here"),
        }
    }

    /// Delete a file from the file-system.
    fn delete(&mut self, pathname: &str) -> Result<bool, FileSystemError> {
        match self.files.remove(&pathname.to_string()) {
            Some(mnode) => {
                // If the pathname is the only link to the memnode, then remove it.
                match Arc::strong_count(&mnode) {
                    1 => {
                        self.mnodes.remove(&mnode);
                        return Ok(true);
                    }
                    _ => {
                        self.files.insert(pathname.to_string(), mnode);
                        return Err(FileSystemError::PermissionError);
                    }
                }
            }
            None => return Err(FileSystemError::InvalidFile),
        };
    }

    fn truncate(&mut self, pathname: &str) -> Result<bool, FileSystemError> {
        match self.files.get(&pathname.to_string()) {
            Some(mnode) => match self.mnodes.get_mut(mnode) {
                Some(memnode) => memnode.file_truncate(),
                None => return Err(FileSystemError::InvalidFile),
            },
            None => return Err(FileSystemError::InvalidFile),
        }
    }

    /// Rename a file from oldname to newname.
    fn rename(&mut self, oldname: &str, newname: &str) -> Result<bool, FileSystemError> {
        if self.files.get(oldname).is_none() {
            return Err(FileSystemError::InvalidFile);
        }

        // If the newfile exists then overwrite it with the oldfile.
        if self.files.get(newname).is_some() {
            self.delete(newname).unwrap();
        }

        let (_key, value) = self.files.remove_entry(oldname).unwrap();
        match self.files.insert(newname.to_string(), value) {
            None => return Ok(true),
            Some(_) => return Err(FileSystemError::PermissionError),
        }
    }

    /// Create a directory. The implementation is quite simplistic for now, and only used
    /// by leveldb benchmark.
    fn mkdir(&mut self, pathname: &str, modes: Modes) -> Result<bool, FileSystemError> {
        // Check if the file with the same name already exists.
        match self.files.get(&pathname.to_string()) {
            Some(_) => return Err(FileSystemError::AlreadyPresent),
            None => {}
        }

        let mnode_num = self.get_next_mno() as u64;
        let memnode = match MemNode::new(mnode_num, pathname, modes, NodeType::Directory) {
            Ok(memnode) => memnode,
            Err(e) => return Err(e),
        };
        self.files.insert(pathname.to_string(), Arc::new(mnode_num));
        self.mnodes.insert(mnode_num, memnode);

        Ok(true)
    }
}

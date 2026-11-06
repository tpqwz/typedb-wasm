//! Directory and file operations for the write-ahead log.
//!
//! The native build uses the real filesystem. The wasm build, and any crate
//! that turns on the `memfs` feature, uses a process-local map instead.

use std::fmt;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[cfg(not(any(target_arch = "wasm32", feature = "memfs")))]
mod sys {
    use super::*;
    use std::fs::{self, OpenOptions};

    pub fn exists(path: &Path) -> bool {
        path.exists()
    }

    pub fn create_dir(path: &Path) -> io::Result<()> {
        fs::create_dir(path)
    }

    pub fn create_dir_all(path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }

    pub fn remove_dir_all(path: &Path) -> io::Result<()> {
        fs::remove_dir_all(path)
    }

    pub fn remove_file(path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    pub fn export_tree(_root: &Path) -> Vec<u8> {
        Vec::new()
    }

    pub fn import_tree(_bytes: &[u8]) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "snapshot import needs the memory log"))
    }

    pub fn clear_tree(root: &Path) {
        if root.exists() {
            let _ = fs::remove_dir_all(root);
        }
    }

    pub fn read_dir(path: &Path) -> io::Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for entry in fs::read_dir(path)? {
            paths.push(entry?.path());
        }
        Ok(paths)
    }

    pub fn len(path: &Path) -> io::Result<u64> {
        Ok(fs::metadata(path)?.len())
    }

    pub fn is_dir(path: &Path) -> bool {
        path.is_dir()
    }

    pub fn read_file(path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    pub fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut file = OpenOptions::new().write(true).create(true).truncate(true).open(path)?;
        file.write_all(bytes)?;
        file.sync_all()
    }

    pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    pub struct WalIo {
        file: fs::File,
    }

    impl fmt::Debug for WalIo {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("WalIo")
        }
    }

    impl WalIo {
        pub fn open_read(path: &Path) -> io::Result<Self> {
            Ok(Self { file: fs::File::open(path)? })
        }

        pub fn open_append(path: &Path) -> io::Result<Self> {
            let file = OpenOptions::new().read(true).append(true).create(true).open(path)?;
            Ok(Self { file })
        }

        pub fn open_write(path: &Path) -> io::Result<Self> {
            Ok(Self { file: OpenOptions::new().write(true).open(path)? })
        }

        pub fn sync_all(&self) -> io::Result<()> {
            self.file.sync_all()
        }

        pub fn set_len(&self, len: u64) -> io::Result<()> {
            self.file.set_len(len)
        }
    }

    impl Read for WalIo {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.file.read(buf)
        }
    }

    impl Write for WalIo {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.file.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.file.flush()
        }
    }

    impl Seek for WalIo {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.file.seek(pos)
        }
    }
}

#[cfg(any(target_arch = "wasm32", feature = "memfs"))]
mod sys {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::OnceLock;

    #[derive(Clone)]
    enum Node {
        Dir,
        File(Vec<u8>),
    }

    fn fs() -> &'static Mutex<BTreeMap<String, Node>> {
        static FS: OnceLock<Mutex<BTreeMap<String, Node>>> = OnceLock::new();
        FS.get_or_init(|| Mutex::new(BTreeMap::new()))
    }

    fn key(path: &Path) -> String {
        let text = path.to_string_lossy().replace('\\', "/");
        if text.len() > 1 { text.trim_end_matches('/').to_string() } else { text }
    }

    pub fn exists(path: &Path) -> bool {
        let key = key(path);
        let fs = fs().lock().expect("memory filesystem lock");
        fs.contains_key(&key) || fs.keys().any(|other| other.starts_with(&(key.clone() + "/")))
    }

    pub fn create_dir(path: &Path) -> io::Result<()> {
        let mut fs = fs().lock().expect("memory filesystem lock");
        fs.insert(key(path), Node::Dir);
        Ok(())
    }

    pub fn create_dir_all(path: &Path) -> io::Result<()> {
        let key = key(path);
        let mut fs = fs().lock().expect("memory filesystem lock");
        let mut built = String::new();
        for part in key.split('/').filter(|part| !part.is_empty()) {
            if built != "/" && !built.is_empty() {
                built.push('/');
            } else if key.starts_with('/') && built.is_empty() {
                built.push('/');
            }
            built.push_str(part);
            fs.entry(built.clone()).or_insert(Node::Dir);
        }
        Ok(())
    }

    pub fn remove_dir_all(path: &Path) -> io::Result<()> {
        let key = key(path);
        let mut fs = fs().lock().expect("memory filesystem lock");
        let prefix = key.clone() + "/";
        fs.retain(|other, _| other != &key && !other.starts_with(&prefix));
        Ok(())
    }

    pub fn remove_file(path: &Path) -> io::Result<()> {
        let mut fs = fs().lock().expect("memory filesystem lock");
        fs.remove(&key(path));
        Ok(())
    }

    fn write_chunk(out: &mut Vec<u8>, bytes: &[u8]) {
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(bytes);
    }

    fn read_chunk(input: &mut &[u8]) -> io::Result<Vec<u8>> {
        if input.len() < 4 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "snapshot truncated"));
        }
        let len = u32::from_le_bytes(input[..4].try_into().unwrap()) as usize;
        *input = &input[4..];
        if input.len() < len {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "snapshot truncated"));
        }
        let chunk = input[..len].to_vec();
        *input = &input[len..];
        Ok(chunk)
    }

    pub fn export_tree(root: &Path) -> Vec<u8> {
        let root = key(root);
        let prefix = root.clone() + "/";
        let fs = fs().lock().expect("memory filesystem lock");
        let mut bytes = Vec::new();
        for (path, node) in fs.iter() {
            let Node::File(data) = node else { continue };
            if path != &root && !path.starts_with(&prefix) {
                continue;
            }
            write_chunk(&mut bytes, path.as_bytes());
            write_chunk(&mut bytes, data);
        }
        bytes
    }

    pub fn import_tree(mut bytes: &[u8]) -> io::Result<()> {
        let mut fs = fs().lock().expect("memory filesystem lock");
        while !bytes.is_empty() {
            let path = read_chunk(&mut bytes)?;
            let data = read_chunk(&mut bytes)?;
            let path = String::from_utf8(path).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
            if parts.len() > 1 {
                let mut built = if path.starts_with('/') { String::from("/") } else { String::new() };
                for part in &parts[..parts.len() - 1] {
                    if !built.is_empty() && built != "/" {
                        built.push('/');
                    }
                    built.push_str(part);
                    fs.entry(built.clone()).or_insert(Node::Dir);
                }
            }
            fs.insert(path, Node::File(data));
        }
        Ok(())
    }

    pub fn clear_tree(root: &Path) {
        let root = key(root);
        let prefix = root.clone() + "/";
        let mut fs = fs().lock().expect("memory filesystem lock");
        fs.retain(|other, _| other != &root && !other.starts_with(&prefix));
    }

    pub fn read_dir(path: &Path) -> io::Result<Vec<PathBuf>> {
        let key = key(path);
        let prefix = key.clone() + "/";
        let fs = fs().lock().expect("memory filesystem lock");
        if !fs.contains_key(&key) && !fs.keys().any(|other| other.starts_with(&prefix)) {
            return Err(io::Error::new(io::ErrorKind::NotFound, "directory missing"));
        }
        let mut paths = Vec::new();
        for other in fs.keys() {
            if let Some(rest) = other.strip_prefix(&prefix)
                && !rest.is_empty()
                && !rest.contains('/')
            {
                paths.push(PathBuf::from(other));
            }
        }
        Ok(paths)
    }

    pub fn len(path: &Path) -> io::Result<u64> {
        let fs = fs().lock().expect("memory filesystem lock");
        match fs.get(&key(path)) {
            Some(Node::File(bytes)) => Ok(bytes.len() as u64),
            Some(Node::Dir) => Ok(0),
            None => Err(io::Error::new(io::ErrorKind::NotFound, "file missing")),
        }
    }

    pub fn is_dir(path: &Path) -> bool {
        let key = key(path);
        let fs = fs().lock().expect("memory filesystem lock");
        match fs.get(&key) {
            Some(Node::Dir) => true,
            Some(Node::File(_)) => false,
            None => fs.keys().any(|other| other.starts_with(&(key.clone() + "/"))),
        }
    }

    pub fn read_file(path: &Path) -> io::Result<Vec<u8>> {
        let fs = fs().lock().expect("memory filesystem lock");
        match fs.get(&key(path)) {
            Some(Node::File(bytes)) => Ok(bytes.clone()),
            Some(Node::Dir) => Err(io::Error::new(io::ErrorKind::IsADirectory, "directory")),
            None => Err(io::Error::new(io::ErrorKind::NotFound, "file missing")),
        }
    }

    pub fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut fs = fs().lock().expect("memory filesystem lock");
        fs.insert(key(path), Node::File(bytes.to_vec()));
        Ok(())
    }

    pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
        let from = key(from);
        let to = key(to);
        let mut fs = fs().lock().expect("memory filesystem lock");
        let prefix = from.clone() + "/";
        let moved: Vec<_> = fs
            .iter()
            .filter(|(path, _)| *path == &from || path.starts_with(&prefix))
            .map(|(path, node)| (to.clone() + &path[from.len()..], node.clone()))
            .collect();
        if moved.is_empty() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "file missing"));
        }
        fs.retain(|other, _| other != &from && !other.starts_with(&prefix));
        for (path, node) in moved {
            fs.insert(path, node);
        }
        Ok(())
    }

    pub struct WalIo {
        path: String,
        pos: u64,
        append: bool,
    }

    impl fmt::Debug for WalIo {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "WalIo({})", self.path)
        }
    }

    impl WalIo {
        pub fn open_read(path: &Path) -> io::Result<Self> {
            let path = key(path);
            let fs = fs().lock().expect("memory filesystem lock");
            if !matches!(fs.get(&path), Some(Node::File(_))) {
                return Err(io::Error::new(io::ErrorKind::NotFound, "file missing"));
            }
            Ok(Self { path, pos: 0, append: false })
        }

        pub fn open_append(path: &Path) -> io::Result<Self> {
            let path = key(path);
            let mut fs = fs().lock().expect("memory filesystem lock");
            let pos = match fs.entry(path.clone()).or_insert_with(|| Node::File(Vec::new())) {
                Node::File(bytes) => bytes.len() as u64,
                Node::Dir => return Err(io::Error::new(io::ErrorKind::IsADirectory, "directory")),
            };
            Ok(Self { path, pos, append: true })
        }

        pub fn open_write(path: &Path) -> io::Result<Self> {
            let path = key(path);
            let fs = fs().lock().expect("memory filesystem lock");
            if !matches!(fs.get(&path), Some(Node::File(_))) {
                return Err(io::Error::new(io::ErrorKind::NotFound, "file missing"));
            }
            Ok(Self { path, pos: 0, append: false })
        }

        pub fn sync_all(&self) -> io::Result<()> {
            Ok(())
        }

        pub fn set_len(&self, len: u64) -> io::Result<()> {
            let mut fs = fs().lock().expect("memory filesystem lock");
            let Some(Node::File(bytes)) = fs.get_mut(&self.path) else {
                return Err(io::Error::new(io::ErrorKind::NotFound, "file missing"));
            };
            bytes.resize(len as usize, 0);
            Ok(())
        }
    }

    impl Read for WalIo {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let fs = fs().lock().expect("memory filesystem lock");
            let Some(Node::File(bytes)) = fs.get(&self.path) else {
                return Err(io::Error::new(io::ErrorKind::NotFound, "file missing"));
            };
            if self.pos as usize >= bytes.len() {
                return Ok(0);
            }
            let start = self.pos as usize;
            let end = (start + buf.len()).min(bytes.len());
            let n = end - start;
            buf[..n].copy_from_slice(&bytes[start..end]);
            self.pos += n as u64;
            Ok(n)
        }
    }

    impl Write for WalIo {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut fs = fs().lock().expect("memory filesystem lock");
            let Some(Node::File(bytes)) = fs.get_mut(&self.path) else {
                return Err(io::Error::new(io::ErrorKind::NotFound, "file missing"));
            };
            if self.append {
                self.pos = bytes.len() as u64;
            }
            let start = self.pos as usize;
            let end = start + buf.len();
            if end > bytes.len() {
                bytes.resize(end, 0);
            }
            bytes[start..end].copy_from_slice(buf);
            self.pos = end as u64;
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Seek for WalIo {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            let fs = fs().lock().expect("memory filesystem lock");
            let len = match fs.get(&self.path) {
                Some(Node::File(bytes)) => bytes.len() as u64,
                _ => 0,
            };
            let next = match pos {
                SeekFrom::Start(value) => value as i64,
                SeekFrom::End(value) => len as i64 + value,
                SeekFrom::Current(value) => self.pos as i64 + value,
            };
            if next < 0 {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start"));
            }
            self.pos = next as u64;
            Ok(self.pos)
        }
    }
}

pub use sys::{
    WalIo, clear_tree, create_dir, create_dir_all, exists, export_tree, import_tree, is_dir, read_dir, read_file,
    remove_dir_all, remove_file, rename, write_file,
};

pub fn len_or_zero(path: &Path) -> u64 {
    sys::len(path).unwrap_or(0)
}

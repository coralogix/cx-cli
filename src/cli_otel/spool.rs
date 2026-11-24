use std::collections::BTreeMap;
use std::fs::{self, DirBuilder, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::limits::MAX_BATCH_BYTES;
use super::otlp::validate_and_encode;

const MAX_BATCHES: usize = 128;
const MAX_DIRECTORY_ENTRIES: usize = 4096;
const MAX_QUEUE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const WRITER_WAIT: Duration = Duration::from_millis(100);
const TEMP_MAX_AGE: Duration = Duration::from_secs(60);
const ID_LEN: usize = 40;
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

type QueueIndex = BTreeMap<(SystemTime, String), u64>;

/// A best-effort queue of OTLP trace requests in an application-owned directory.
///
/// Use a dedicated private directory for each destination. The application
/// owns consent, transport and delivery retries. Cloning this handle does
/// not open any files.
#[derive(Clone)]
pub struct Spool {
    directory: PathBuf,
}

/// An original trace request ready to send as OTLP/HTTP protobuf.
pub struct PendingBatch {
    pub id: String,
    pub payload: Vec<u8>,
}

impl Spool {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    /// Validate and atomically publish one request. Older batches are evicted
    /// when necessary. A sender holding its lock can continue independently.
    pub fn enqueue(&self, payload: &[u8]) -> io::Result<()> {
        let payload = validate_and_encode(payload)?;
        self.prepare()?;

        let (mut file, temp, destination) = self.create_batch()?;
        let result = (|| {
            file.write_all(&payload)?;
            file.sync_all()?;
            drop(file);

            let _writer = self.lock_writer()?;
            let mut queue = self.scan()?;
            self.trim(
                &mut queue,
                MAX_BATCHES - 1,
                MAX_QUEUE_BYTES - payload.len() as u64,
            )?;
            fs::rename(&temp, &destination)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }

    /// Try to acquire the sender lock without waiting. Keep the returned file
    /// open until delivery and removal are complete.
    pub fn try_lock(&self) -> io::Result<File> {
        self.prepare()?;
        let lock = self.open_lock("sender.lock")?;
        lock.try_lock().map_err(io::Error::from)?;
        Ok(lock)
    }

    /// List validated requests oldest first. Invalid or expired files are
    /// discarded, and unknown protobuf fields are stripped by re-encoding.
    pub fn pending(&self) -> io::Result<Vec<PendingBatch>> {
        match self.check_directory() {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        }
        let writer = self.open_lock("queue.lock")?;
        writer.try_lock().map_err(io::Error::from)?;
        let mut queue = self.scan()?;
        self.trim(&mut queue, MAX_BATCHES, MAX_QUEUE_BYTES)?;
        drop(writer);

        let mut pending = Vec::with_capacity(queue.len());
        for ((_, id), len) in queue {
            let path = self.batch_path(&id);
            match read_batch(&path, len) {
                Ok(payload) => pending.push(PendingBatch { id, payload }),
                Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                    let _ = remove_file(&path);
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(pending)
    }

    /// Remove a delivered request. A request already removed is harmless.
    pub fn remove(&self, id: &str) -> io::Result<()> {
        if !valid_id(id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid batch ID",
            ));
        }
        match self.check_directory() {
            Ok(()) => remove_file(&self.batch_path(id)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn prepare(&self) -> io::Result<()> {
        let mut builder = DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&self.directory)?;
        self.check_directory()
    }

    fn check_directory(&self) -> io::Result<()> {
        let metadata = fs::symlink_metadata(&self.directory)?;
        if metadata.is_dir() && is_private(&metadata) {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "trace queue must be a private directory",
            ))
        }
    }

    fn open_lock(&self, name: &str) -> io::Result<File> {
        let path = self.directory.join(name);
        let mut options = private_options();
        options.read(true).write(true);
        match options.create_new(true).open(&path) {
            Ok(file) => Ok(file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&path)?;
                if !private_file(&metadata) {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "invalid queue lock",
                    ));
                }
                options.create_new(false).open(path)
            }
            Err(error) => Err(error),
        }
    }

    fn lock_writer(&self) -> io::Result<File> {
        let writer = self.open_lock("queue.lock")?;
        let start = Instant::now();
        loop {
            match writer.try_lock().map_err(io::Error::from) {
                Ok(()) => return Ok(writer),
                Err(error)
                    if error.kind() == io::ErrorKind::WouldBlock
                        && start.elapsed() < WRITER_WAIT =>
                {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn create_batch(&self) -> io::Result<(File, PathBuf, PathBuf)> {
        for _ in 0..8 {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            let pid = std::process::id();
            let counter = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let id = format!("{nanos:016x}{pid:08x}{counter:016x}");
            let destination = self.batch_path(&id);
            match fs::symlink_metadata(&destination) {
                Ok(_) => continue,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            let temp = self.directory.join(format!("{id}.tmp"));
            match private_options().write(true).create_new(true).open(&temp) {
                Ok(file) => return Ok((file, temp, destination)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "batch ID collision",
        ))
    }

    fn batch_path(&self, id: &str) -> PathBuf {
        self.directory.join(format!("{id}.otlp"))
    }

    // The writer lock is held during scans. Only files in our own namespace
    // are removed. The ordered index remains bounded even with old/corrupt data.
    fn scan(&self) -> io::Result<QueueIndex> {
        let mut queue = BTreeMap::new();
        let now = SystemTime::now();
        for (index, entry) in fs::read_dir(&self.directory)?.enumerate() {
            if index >= MAX_DIRECTORY_ENTRIES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "too many queue directory entries",
                ));
            }
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some((id, extension)) = name.rsplit_once('.') else {
                continue;
            };
            if !valid_id(id) || !matches!(extension, "tmp" | "otlp") {
                continue;
            }
            let metadata = match fs::symlink_metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if metadata.is_dir() {
                continue;
            }
            let modified = metadata.modified()?;
            let len = metadata.len();
            if extension == "tmp" {
                if !private_file(&metadata)
                    || now
                        .duration_since(modified)
                        .is_ok_and(|age| age > TEMP_MAX_AGE)
                {
                    remove_file(&entry.path())?;
                }
                continue;
            }
            if !private_file(&metadata)
                || len == 0
                || len > MAX_BATCH_BYTES as u64
                || now.duration_since(modified).is_ok_and(|age| age > MAX_AGE)
            {
                remove_file(&entry.path())?;
                continue;
            }

            queue.insert((modified, id.to_owned()), len);
            if queue.len() > MAX_BATCHES {
                let ((_, oldest), _) = queue.pop_first().expect("queue contains a batch");
                remove_file(&self.batch_path(&oldest))?;
            }
        }
        Ok(queue)
    }

    fn trim(&self, queue: &mut QueueIndex, max_batches: usize, max_bytes: u64) -> io::Result<()> {
        let mut bytes = queue.values().sum::<u64>();
        while queue.len() > max_batches || bytes > max_bytes {
            let ((_, id), len) = queue.pop_first().expect("queue contains a batch");
            remove_file(&self.batch_path(&id))?;
            bytes -= len;
        }
        Ok(())
    }
}

fn read_batch(path: &Path, len: u64) -> io::Result<Vec<u8>> {
    if !private_file(&fs::symlink_metadata(path)?) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid queue file",
        ));
    }
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !private_file(&metadata) || metadata.len() != len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid queue file",
        ));
    }
    let mut payload = Vec::with_capacity(len as usize);
    file.take(len + 1).read_to_end(&mut payload)?;
    if payload.len() as u64 != len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid queue file",
        ));
    }
    validate_and_encode(&payload)
}

fn valid_id(id: &str) -> bool {
    id.len() == ID_LEN
        && id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn remove_file(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

fn private_file(metadata: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return false;
        }
    }
    metadata.is_file() && is_private(metadata)
}

#[cfg(unix)]
fn is_private(metadata: &Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o077 == 0
}

#[cfg(not(unix))]
fn is_private(_: &Metadata) -> bool {
    true
}

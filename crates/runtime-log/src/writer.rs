use crate::{
    sanitize_event, sanitize_text, unique_id, LogContext, LogLevel, LogPhase, LogWriterHealth,
    RuntimeLogEvent,
};
use chrono::Utc;
use fs2::FileExt;
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    hash::{Hash, Hasher},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, Mutex, OnceLock, Weak,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

#[derive(Debug, Clone)]
pub struct RuntimeLogConfig {
    pub directory: PathBuf,
    pub run_id: String,
    pub max_age_days: u64,
    pub max_total_bytes: u64,
    pub segment_bytes: u64,
    pub queue_capacity: usize,
    pub flush_interval: Duration,
    pub shutdown_timeout: Duration,
}
impl RuntimeLogConfig {
    pub fn new(directory: PathBuf, run_id: String) -> Self {
        Self {
            directory,
            run_id,
            max_age_days: 30,
            max_total_bytes: 200 * 1024 * 1024,
            segment_bytes: 10 * 1024 * 1024,
            queue_capacity: 4096,
            flush_interval: Duration::from_secs(1),
            shutdown_timeout: Duration::from_secs(2),
        }
    }
}
#[derive(Default)]
struct Stats {
    dropped: AtomicU64,
    written: AtomicU64,
    failures: AtomicU64,
    pending: AtomicUsize,
    error: Mutex<Option<String>>,
    stopping: AtomicBool,
}
impl Stats {
    fn health(&self) -> LogWriterHealth {
        LogWriterHealth {
            dropped_events: self.dropped.load(Ordering::Relaxed),
            written_events: self.written.load(Ordering::Relaxed),
            write_failures: self.failures.load(Ordering::Relaxed),
            last_error: self.error.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            pending_events: self.pending.load(Ordering::Relaxed),
        }
    }
    fn failure(&self, error: impl std::fmt::Display) {
        self.failures.fetch_add(1, Ordering::Relaxed);
        *self.error.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(sanitize_text(&error.to_string()));
    }
}
struct Inner {
    sender: SyncSender<RuntimeLogEvent>,
    stats: Arc<Stats>,
    run_id: String,
}
#[derive(Clone)]
pub struct RuntimeLogHandle {
    inner: Arc<Inner>,
}
impl std::fmt::Debug for RuntimeLogHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeLogHandle")
            .field("health", &self.health())
            .finish()
    }
}
impl RuntimeLogHandle {
    pub fn record(&self, mut event: RuntimeLogEvent) -> bool {
        if self.inner.stats.stopping.load(Ordering::Acquire) {
            return false;
        }
        if event.source == crate::LogSource::Ux && event.context.run_id.is_none() {
            event.context.run_id = Some(self.inner.run_id.clone());
        }
        sanitize_event(&mut event);
        self.inner.stats.pending.fetch_add(1, Ordering::Relaxed);
        match self.inner.sender.try_send(event) {
            Ok(()) => true,
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                self.inner.stats.pending.fetch_sub(1, Ordering::Relaxed);
                self.inner.stats.dropped.fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }
    pub fn health(&self) -> LogWriterHealth {
        self.inner.stats.health()
    }
}
static GLOBAL: OnceLock<Mutex<Weak<Inner>>> = OnceLock::new();
pub fn install_global(handle: RuntimeLogHandle) -> Result<(), RuntimeLogHandle> {
    let mut slot = GLOBAL
        .get_or_init(|| Mutex::new(Weak::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if slot
        .upgrade()
        .is_some_and(|inner| !inner.stats.stopping.load(Ordering::Acquire))
    {
        return Err(handle);
    }
    *slot = Arc::downgrade(&handle.inner);
    Ok(())
}
pub fn global() -> Option<RuntimeLogHandle> {
    let inner = GLOBAL
        .get()?
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .upgrade()?;
    if inner.stats.stopping.load(Ordering::Acquire) {
        None
    } else {
        Some(RuntimeLogHandle { inner })
    }
}
pub fn record(event: RuntimeLogEvent) -> bool {
    global().is_some_and(|handle| handle.record(event))
}

pub struct RuntimeLogRuntime {
    handle: RuntimeLogHandle,
    done: Option<Receiver<()>>,
    shutdown_timeout: Duration,
}
impl RuntimeLogRuntime {
    pub fn start(mut config: RuntimeLogConfig) -> io::Result<Self> {
        if config.queue_capacity == 0
            || config.queue_capacity > 65536
            || config.segment_bytes == 0
            || config.max_total_bytes == 0
            || config.flush_interval.is_zero()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid runtime log capacity or flush interval",
            ));
        }
        config.run_id = sanitize_text(&config.run_id);
        ensure_directory(&config.directory)?;
        // Verify write access during startup without retaining an unmanaged file.
        let probe = config.directory.join(format!(".probe-{}", unique_id()));
        let file = private_create(&probe)?;
        drop(file);
        fs::remove_file(probe)?;
        let (sender, receiver) = mpsc::sync_channel(config.queue_capacity);
        let (done_tx, done) = mpsc::sync_channel(1);
        let stats = Arc::new(Stats::default());
        let handle = RuntimeLogHandle {
            inner: Arc::new(Inner {
                sender,
                stats: stats.clone(),
                run_id: config.run_id.clone(),
            }),
        };
        let shutdown_timeout = config.shutdown_timeout;
        thread::Builder::new()
            .name("runtime-log".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    Worker::new(config, stats.clone()).run(receiver);
                }));
                if result.is_err() {
                    stats.failure("runtime log writer panicked; queued events were not persisted");
                    let pending = stats.pending.swap(0, Ordering::AcqRel);
                    stats.dropped.fetch_add(pending as u64, Ordering::Relaxed);
                }
                stats.stopping.store(true, Ordering::Release);
                let _ = done_tx.send(());
            })?;
        Ok(Self {
            handle,
            done: Some(done),
            shutdown_timeout,
        })
    }
    pub fn handle(&self) -> RuntimeLogHandle {
        self.handle.clone()
    }
    pub fn shutdown(mut self) -> LogWriterHealth {
        self.stop();
        self.handle.health()
    }
    fn stop(&mut self) {
        self.handle
            .inner
            .stats
            .stopping
            .store(true, Ordering::Release);
        if let Some(done) = self.done.take() {
            if done.recv_timeout(self.shutdown_timeout).is_err() {
                self.handle
                    .inner
                    .stats
                    .failure("runtime log shutdown flush timed out");
            }
        }
    }
}
impl Drop for RuntimeLogRuntime {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) fn ensure_directory(path: &Path) -> io::Result<()> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "runtime log directory is not a real directory",
            ));
        }
    }
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
pub(crate) fn nofollow(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
}
pub(crate) fn private_create(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).read(true).create_new(true);
    nofollow(&mut options);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
fn hash(value: impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}
struct Segment {
    file: File,
    bytes: u64,
    day: chrono::NaiveDate,
    used: Instant,
}
struct Alert {
    latest: RuntimeLogEvent,
    count: u64,
    retries: u64,
    emitted_count: u64,
    first: chrono::DateTime<Utc>,
    last_emit: Instant,
}
struct Worker {
    config: RuntimeLogConfig,
    stats: Arc<Stats>,
    segments: HashMap<Option<String>, Segment>,
    alerts: HashMap<u64, Alert>,
    writer_id: String,
    last_cleanup: Instant,
    reported_drops: u64,
}
impl Worker {
    fn new(config: RuntimeLogConfig, stats: Arc<Stats>) -> Self {
        Self {
            config,
            stats,
            segments: HashMap::new(),
            alerts: HashMap::new(),
            writer_id: unique_id(),
            last_cleanup: Instant::now(),
            reported_drops: 0,
        }
    }
    fn run(mut self, receiver: Receiver<RuntimeLogEvent>) {
        self.cleanup();
        let mut last_flush = Instant::now();
        loop {
            let wait = self
                .config
                .flush_interval
                .saturating_sub(last_flush.elapsed())
                .min(Duration::from_millis(50));
            match receiver.recv_timeout(wait) {
                Ok(event) => {
                    self.stats.pending.fetch_sub(1, Ordering::Relaxed);
                    self.accept(event);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if last_flush.elapsed() >= self.config.flush_interval {
                self.flush_alerts(false);
                self.flush();
                last_flush = Instant::now();
            }
            if self.last_cleanup.elapsed() >= Duration::from_secs(60) {
                self.cleanup();
            }
            if self.stats.stopping.load(Ordering::Acquire) {
                while let Ok(event) = receiver.try_recv() {
                    self.stats.pending.fetch_sub(1, Ordering::Relaxed);
                    self.accept(event);
                }
                break;
            }
        }
        self.flush_alerts(true);
        self.report_drops();
        self.flush();
        self.segments.clear();
        self.cleanup();
    }
    fn alert_fingerprint(event: &RuntimeLogEvent) -> Option<u64> {
        event.alert_key.as_ref().map(|key| {
            hash((
                &event.context.owner_id,
                &event.context.module,
                &event.context.operation,
                &event.source_path,
                &event.target_path,
                key,
            ))
        })
    }
    fn accept(&mut self, mut event: RuntimeLogEvent) {
        let Some(key) = Self::alert_fingerprint(&event) else {
            self.write_event(&event);
            self.report_drops();
            return;
        };
        if matches!(
            event.phase,
            LogPhase::Recovered | LogPhase::Succeeded | LogPhase::Cancelled
        ) {
            if let Some(alert) = self.alerts.remove(&key) {
                event.first_seen = Some(alert.first);
                event.last_seen = Some(event.timestamp);
                event.repeat_count = alert.count;
                event.retry_count = alert.retries;
            }
            self.write_event(&event);
            return;
        }
        if !matches!(
            event.phase,
            LogPhase::Failed
                | LogPhase::Retry
                | LogPhase::Degraded
                | LogPhase::Observed
                | LogPhase::Repeated
        ) {
            self.write_event(&event);
            return;
        }
        if let Some(mut alert) = self.alerts.remove(&key) {
            let same = alert.latest.phase == event.phase
                && alert.latest.level == event.level
                && alert.latest.error_code == event.error_code
                && alert.latest.error_chain == event.error_chain;
            alert.count += 1;
            alert.retries += u64::from(event.phase == LogPhase::Retry);
            event.first_seen = Some(alert.first);
            event.last_seen = Some(event.timestamp);
            event.repeat_count = alert.count;
            event.retry_count = alert.retries;
            alert.latest = event.clone();
            if !same || alert.last_emit.elapsed() >= Duration::from_secs(60) {
                self.write_event(&event);
                alert.emitted_count = alert.count;
                alert.last_emit = Instant::now();
            }
            self.alerts.insert(key, alert);
        } else {
            if self.alerts.len() >= 256 {
                if let Some(key) = self
                    .alerts
                    .iter()
                    .min_by_key(|(_, a)| a.last_emit)
                    .map(|(key, _)| *key)
                {
                    if let Some(alert) = self.alerts.remove(&key) {
                        self.write_summary(alert);
                    }
                }
            }
            event.first_seen = Some(event.timestamp);
            event.last_seen = Some(event.timestamp);
            self.write_event(&event);
            self.alerts.insert(
                key,
                Alert {
                    first: event.timestamp,
                    retries: u64::from(event.phase == LogPhase::Retry),
                    latest: event,
                    count: 1,
                    emitted_count: 1,
                    last_emit: Instant::now(),
                },
            );
        }
        self.report_drops();
    }
    fn write_summary(&mut self, mut alert: Alert) {
        if alert.count == alert.emitted_count {
            return;
        }
        alert.latest.event_id = unique_id();
        alert.latest.phase = LogPhase::Repeated;
        alert.latest.repeat_count = alert.count;
        alert.latest.retry_count = alert.retries;
        self.write_event(&alert.latest);
    }
    fn flush_alerts(&mut self, final_flush: bool) {
        let keys: Vec<_> = self
            .alerts
            .iter()
            .filter(|(_, a)| final_flush || a.last_emit.elapsed() >= Duration::from_secs(60))
            .map(|(key, _)| *key)
            .collect();
        for key in keys {
            if let Some(mut alert) = self.alerts.remove(&key) {
                if alert.count > alert.emitted_count {
                    let mut summary = alert.latest.clone();
                    summary.event_id = unique_id();
                    summary.phase = LogPhase::Repeated;
                    summary.repeat_count = alert.count;
                    summary.retry_count = alert.retries;
                    self.write_event(&summary);
                    alert.emitted_count = alert.count;
                }
                alert.last_emit = Instant::now();
                if !final_flush {
                    self.alerts.insert(key, alert);
                }
            }
        }
    }
    fn report_drops(&mut self) {
        let dropped = self.stats.dropped.load(Ordering::Relaxed);
        if dropped <= self.reported_drops {
            return;
        }
        let event = RuntimeLogEvent::new(
            LogContext {
                run_id: Some(self.config.run_id.clone()),
                app: "logs".into(),
                module: "ux.runtime_log".into(),
                operation: "write".into(),
                ..Default::default()
            },
            LogLevel::Warning,
            LogPhase::Degraded,
            format!(
                "Runtime log dropped {} events; total {}",
                dropped - self.reported_drops,
                dropped
            ),
        );
        // Failure of the health summary must not manufacture further dropped
        // business events or recursively feed logging health back into itself.
        match self.try_write(&event) {
            Ok(()) => {
                self.stats.written.fetch_add(1, Ordering::Relaxed);
                self.reported_drops = dropped;
            }
            Err(error) => self.stats.failure(error),
        }
    }
    fn write_event(&mut self, event: &RuntimeLogEvent) -> bool {
        match self.try_write(event) {
            Ok(()) => {
                self.stats.written.fetch_add(1, Ordering::Relaxed);
                *self.stats.error.lock().unwrap_or_else(|e| e.into_inner()) = None;
                true
            }
            Err(error) => {
                self.stats.failure(error);
                self.stats.dropped.fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }
    fn try_write(&mut self, event: &RuntimeLogEvent) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(event)?;
        if bytes.len() >= crate::privacy::MAX_RECORD_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "runtime log event exceeds record limit",
            ));
        }
        bytes.push(b'\n');
        let owner = event.context.owner_id.clone();
        let day = Utc::now().date_naive();
        let rotate = self.segments.get(&owner).is_some_and(|s| {
            s.bytes + bytes.len() as u64 > self.config.segment_bytes || s.day != day
        });
        if rotate {
            self.close_segment(&owner);
        }
        if !self.segments.contains_key(&owner) {
            if self.segments.len() >= 32 {
                if let Some(oldest) = self
                    .segments
                    .iter()
                    .min_by_key(|(_, s)| s.used)
                    .map(|(owner, _)| owner.clone())
                {
                    self.close_segment(&oldest);
                }
            }
            let path = self.config.directory.join(format!(
                "runtime-{}-{:016x}-{}.jsonl",
                self.writer_id,
                hash(&owner),
                unique_id()
            ));
            // Coordinate the create-to-lock interval with other cleaners.
            let creation_guard = retention_lock(&self.config.directory)?;
            creation_guard.lock_exclusive()?;
            let file = private_create(&path)?;
            file.try_lock_exclusive()?;
            drop(creation_guard);
            self.segments.insert(
                owner.clone(),
                Segment {
                    file,
                    bytes: 0,
                    day,
                    used: Instant::now(),
                },
            );
            self.cleanup();
        }
        // Serialize reservation and writing across processes so active files
        // cannot take total storage beyond the configured cap.
        let reservation = retention_lock(&self.config.directory)?;
        reservation.lock_exclusive()?;
        let mut capacity = cleanup_reserved(&self.config, bytes.len() as u64)?;
        while !capacity {
            let closed = self
                .segments
                .iter()
                .filter(|(key, _)| **key != owner)
                .min_by_key(|(_, segment)| segment.used)
                .map(|(key, _)| key.clone());
            if let Some(closed) = closed {
                self.close_segment(&closed);
                capacity = cleanup_reserved(&self.config, bytes.len() as u64)?;
            } else {
                break;
            }
        }
        if !capacity {
            if self
                .segments
                .get(&owner)
                .is_some_and(|segment| segment.bytes > 0)
                && bytes.len() as u64 <= self.config.max_total_bytes
            {
                self.close_segment(&owner);
                drop(reservation);
                return self.try_write(event);
            }
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                "runtime log capacity is occupied by active files",
            ));
        }
        let segment = self.segments.get_mut(&owner).expect("segment inserted");
        if let Err(error) = segment.file.write_all(&bytes) {
            self.close_segment(&owner);
            return Err(error);
        }
        segment.bytes += bytes.len() as u64;
        segment.used = Instant::now();
        Ok(())
    }
    fn close_segment(&mut self, owner: &Option<String>) {
        if let Some(segment) = self.segments.remove(owner) {
            if let Err(error) = segment.file.sync_data() {
                self.stats.failure(error);
            }
        }
    }
    fn flush(&mut self) {
        for segment in self.segments.values_mut() {
            if let Err(error) = segment.file.sync_data() {
                self.stats.failure(error);
            }
        }
    }
    fn cleanup(&mut self) {
        self.last_cleanup = Instant::now();
        if let Err(error) = cleanup(&self.config) {
            self.stats.failure(error);
        }
    }
}
fn retention_lock(directory: &Path) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.read(true).write(true).create(true);
    nofollow(&mut opts);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(directory.join(".retention.lock"))
}
fn cleanup(config: &RuntimeLogConfig) -> io::Result<()> {
    let lock = retention_lock(&config.directory)?;
    if lock.try_lock_exclusive().is_err() {
        return Ok(());
    }
    cleanup_reserved(config, 0).map(|_| ())
}
// The caller owns the interprocess retention lock throughout this operation.
fn cleanup_reserved(config: &RuntimeLogConfig, reserve: u64) -> io::Result<bool> {
    let mut files = Vec::new();
    let mut total = 0_u64;
    for entry in fs::read_dir(&config.directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("runtime-") || !name.ends_with(".jsonl") {
            continue;
        }
        let meta = fs::symlink_metadata(entry.path())?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            continue;
        }
        total = total.saturating_add(meta.len());
        files.push((
            meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            meta.len(),
            entry.path(),
        ));
    }
    files.sort_by_key(|(time, _, _)| *time);
    let age = Duration::from_secs(config.max_age_days.saturating_mul(86400));
    for (modified, length, path) in files {
        if total.saturating_add(reserve) <= config.max_total_bytes
            && modified.elapsed().unwrap_or_default() < age
        {
            continue;
        }
        let mut opts = OpenOptions::new();
        opts.read(true).write(true);
        nofollow(&mut opts);
        let Ok(file) = opts.open(&path) else {
            continue;
        };
        if file.try_lock_exclusive().is_err() {
            continue;
        }
        match fs::remove_file(&path) {
            Ok(()) => total = total.saturating_sub(length),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(total.saturating_add(reserve) <= config.max_total_bytes)
}

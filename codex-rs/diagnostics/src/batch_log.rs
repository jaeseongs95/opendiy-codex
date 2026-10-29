//! Content-free tracing batches. The producer never performs filesystem I/O.
//! Completed local batches are copied once to an optional streaming-drive path.
use chrono::Local;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{
    Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};
use uuid::Uuid;

const QUEUE: usize = 4096;
const MAX_EVENT: usize = 4096;

pub fn metadata_enabled(metadata: &tracing::Metadata<'_>) -> bool {
    metadata.target().starts_with("codex") && *metadata.level() <= tracing::Level::INFO
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub destination: Option<PathBuf>,
    pub flush_seconds: u64,
    pub sample_seconds: u64,
    pub max_batch_bytes: usize,
    pub rpc: bool,
    pub events: bool,
    pub process: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            destination: None,
            flush_seconds: 60,
            sample_seconds: 5,
            max_batch_bytes: 16 * 1024 * 1024,
            rpc: true,
            events: true,
            process: true,
        }
    }
}

impl Settings {
    fn validate(&self, spool: &Path) -> io::Result<()> {
        if !(1..=3600).contains(&self.flush_seconds)
            || !(1..=3600).contains(&self.sample_seconds)
            || !(16 * 1024..=64 * 1024 * 1024).contains(&self.max_batch_bytes)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid CLI log interval or batch size",
            ));
        }
        if self
            .destination
            .as_ref()
            .is_some_and(|p| !p.is_absolute() || p.starts_with(spool) || spool.starts_with(p))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "CLI log destination must be absolute and separate from the local spool",
            ));
        }
        Ok(())
    }
}

#[derive(Default)]
struct Counters {
    dropped: AtomicU64,
    local_failures: AtomicU64,
    export_failures: AtomicU64,
    exported: AtomicU64,
    last_local_error: Mutex<Option<String>>,
    last_export_error: Mutex<Option<String>>,
}

#[derive(Clone)]
pub struct BatchLayer {
    sender: SyncSender<Command>,
    counters: Arc<Counters>,
    settings: Settings,
}

enum Command {
    Event(Vec<u8>),
    Shutdown,
}

/// Dropping never waits for streaming-drive I/O. A local writer receives a
/// shutdown marker; on abrupt process exit the last in-memory batch may be lost.
pub struct Guard {
    sender: SyncSender<Command>,
    stop_export: Arc<AtomicBool>,
    finished: Receiver<()>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.stop_export.store(true, Ordering::Release);
        let _ = self.sender.try_send(Command::Shutdown);
        // The writer only touches the local spool. Keep exit bounded even when
        // local I/O is unhealthy; completed files survive for the next exporter.
        let _ = self.finished.recv_timeout(Duration::from_millis(500));
    }
}

/// Configuration is deliberately outside model/project config: only the local
/// operator's CODEX_HOME/cli-logging.toml can enable diagnostic file output.
pub fn start(home: &Path) -> io::Result<(Option<BatchLayer>, Option<Guard>)> {
    let file = home.join("cli-logging.toml");
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((None, None)),
        Err(e) => return Err(e),
    };
    let settings: Settings =
        toml::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    if !settings.enabled {
        return Ok((None, None));
    }
    start_with(home.join("log").join("cli-batches"), settings)
        .map(|(layer, guard)| (Some(layer), Some(guard)))
}

fn start_with(spool: PathBuf, settings: Settings) -> io::Result<(BatchLayer, Guard)> {
    settings.validate(&spool)?;
    fs::create_dir_all(&spool)?;
    let (sender, receiver) = mpsc::sync_channel(QUEUE);
    let counters = Arc::new(Counters::default());
    let stop_export = Arc::new(AtomicBool::new(false));
    let (finished_tx, finished) = mpsc::channel();
    let writer_settings = settings.clone();
    let writer_spool = spool.clone();
    let writer_counters = counters.clone();
    let writer_stopping = stop_export.clone();
    thread::Builder::new()
        .name("cli-log-buffer".into())
        .spawn(move || {
            writer(
                receiver,
                &writer_spool,
                &writer_settings,
                &writer_counters,
                &writer_stopping,
            );
            let _ = finished_tx.send(());
        })?;
    if let Some(destination) = settings.destination.clone() {
        let stopping = stop_export.clone();
        let export_counters = counters.clone();
        thread::Builder::new()
            .name("cli-log-export".into())
            .spawn(move || {
                while !stopping.load(Ordering::Acquire) {
                    export_ready(&spool, &destination, &export_counters);
                    for _ in 0..600 {
                        if stopping.load(Ordering::Acquire) {
                            return;
                        }
                        thread::sleep(Duration::from_millis(100));
                    }
                }
            })
            .map_err(|error| {
                stop_export.store(true, Ordering::Release);
                let _ = sender.try_send(Command::Shutdown);
                error
            })?;
    }
    let layer = BatchLayer {
        sender: sender.clone(),
        counters,
        settings,
    };
    Ok((
        layer,
        Guard {
            sender,
            stop_export,
            finished,
        },
    ))
}

impl BatchLayer {
    fn submit(&self, value: Value) {
        let Ok(mut bytes) = serde_json::to_vec(&value) else {
            return;
        };
        bytes.push(b'\n');
        if bytes.len() > MAX_EVENT || self.sender.try_send(Command::Event(bytes)).is_err() {
            self.counters.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[derive(Default)]
struct SafeFields(Map<String, Value>);

impl Visit for SafeFields {
    fn record_str(&mut self, field: &Field, value: &str) {
        let name = field.name();
        if name == "rpc.method"
            && value.len() <= 128
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
        {
            self.0.insert(name.into(), value.into());
        } else if matches!(
            name,
            "thread_id" | "turn_id" | "rpc.request_id" | "thread.id" | "turn.id"
        ) && (Uuid::parse_str(value).is_ok() || value.parse::<u64>().is_ok())
        {
            self.0.insert(name.into(), value.into());
        }
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        if numeric_field(field.name()) {
            self.0.insert(field.name().into(), value.into());
        }
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        if numeric_field(field.name()) {
            self.0.insert(field.name().into(), value.into());
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        // Official RPC spans use `%id`. Only allowlisted, bounded UUID/numeric
        // IDs may cross this visitor; never format arbitrary payload fields.
        if matches!(
            field.name(),
            "rpc.request_id" | "thread_id" | "turn_id" | "thread.id" | "turn.id"
        ) {
            struct Limited(String);
            impl std::fmt::Write for Limited {
                fn write_str(&mut self, value: &str) -> std::fmt::Result {
                    if self.0.len() + value.len() > 128 {
                        return Err(std::fmt::Error);
                    }
                    self.0.push_str(value);
                    Ok(())
                }
            }
            let mut text = Limited(String::new());
            if std::fmt::write(&mut text, format_args!("{value:?}")).is_ok() {
                self.record_str(field, text.0.trim_matches('"'));
            }
        }
    }
}

fn numeric_field(name: &str) -> bool {
    matches!(
        name,
        "rpc.request_id"
            | "elapsed_ms"
            | "duration_ms"
            | "latency_ms"
            | "queue_ms"
            | "pending"
            | "active"
            | "input_tokens"
            | "cached_input_tokens"
            | "output_tokens"
            | "total_tokens"
            | "bytes"
            | "count"
            | "attempt"
            | "status_code"
    )
}

struct SpanData {
    started: Instant,
    fields: Map<String, Value>,
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for BatchLayer {
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        if !self.settings.rpc || !attrs.metadata().target().starts_with("codex") {
            return;
        }
        let mut fields = SafeFields::default();
        attrs.record(&mut fields);
        // The registry has resolved explicit, contextual and detached ancestry.
        // Attributes::parent only contains explicitly supplied parent IDs.
        let parent = ctx
            .span(id)
            .and_then(|span| span.parent().map(|parent| parent.id().into_u64()));
        self.submit(json!({"kind":"span_open", "at":Local::now().to_rfc3339(),
            "pid":std::process::id(), "span":id.into_u64(),
            "parent":parent,
            "target":attrs.metadata().target(), "name":attrs.metadata().name(), "fields":fields.0}));
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(SpanData {
                started: Instant::now(),
                fields: fields.0,
            });
        }
    }
    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let mut extensions = span.extensions_mut();
            if let Some(data) = extensions.get_mut::<SpanData>() {
                let mut fields = SafeFields::default();
                values.record(&mut fields);
                data.fields.extend(fields.0);
            }
        }
    }
    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(&id) {
            if let Some(data) = span.extensions().get::<SpanData>() {
                self.submit(json!({"kind":"span_close", "at":Local::now().to_rfc3339(),
                    "pid":std::process::id(), "span":id.into_u64(), "target":span.metadata().target(),
                    "name":span.metadata().name(), "elapsed_us":data.started.elapsed().as_micros().min(u64::MAX as u128) as u64,
                    "fields":data.fields}));
            }
        }
    }
    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        let meta = event.metadata();
        if !self.settings.events
            || !meta.target().starts_with("codex")
            || *meta.level() > tracing::Level::INFO
        {
            return;
        }
        let mut fields = SafeFields::default();
        event.record(&mut fields);
        let span = ctx.event_span(event).map(|s| s.id().into_u64());
        self.submit(
            json!({"kind":"event", "at":Local::now().to_rfc3339(), "pid":std::process::id(),
            "target":meta.target(), "name":meta.name(), "level":meta.level().as_str(),
            "line":meta.line(), "span":span, "fields":fields.0}),
        );
    }
}

fn counters_value(c: &Counters) -> Value {
    json!({"dropped":c.dropped.load(Ordering::Relaxed), "local_failures":c.local_failures.load(Ordering::Relaxed),
        "export_failures":c.export_failures.load(Ordering::Relaxed), "exported":c.exported.load(Ordering::Relaxed),
        "last_local_error":c.last_local_error.lock().ok().and_then(|v|v.clone()),
        "last_export_error":c.last_export_error.lock().ok().and_then(|v|v.clone())})
}

fn writer(
    receiver: Receiver<Command>,
    spool: &Path,
    settings: &Settings,
    counters: &Counters,
    stopping: &AtomicBool,
) {
    let mut buffer = Vec::new();
    let mut last_flush = Instant::now();
    let mut last_sample = Instant::now() - Duration::from_secs(settings.sample_seconds);
    loop {
        let mut shutdown = false;
        let next = if stopping.load(Ordering::Acquire) {
            match receiver.try_recv() {
                Ok(command) => Ok(command),
                Err(_) => Err(mpsc::RecvTimeoutError::Disconnected),
            }
        } else {
            receiver.recv_timeout(Duration::from_millis(100))
        };
        match next {
            Ok(Command::Event(bytes)) => {
                if buffer.len() + bytes.len() <= settings.max_batch_bytes {
                    buffer.extend(bytes);
                } else {
                    counters.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => shutdown = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if settings.process && last_sample.elapsed() >= Duration::from_secs(settings.sample_seconds)
        {
            let snapshot = crate::snapshot();
            let gauges: Map<String, Value> = snapshot
                .gauges
                .into_iter()
                .map(|v| (v.name.into(), v.value.into()))
                .collect();
            let row = json!({"kind":"process", "at":Local::now().to_rfc3339(), "pid":snapshot.process.id,
                "resident_memory_bytes":snapshot.process.resident_memory_bytes, "gauges":gauges,
                "collector":counters_value(counters)});
            if let Ok(mut bytes) = serde_json::to_vec(&row) {
                bytes.push(b'\n');
                if buffer.len() + bytes.len() <= settings.max_batch_bytes {
                    buffer.extend(bytes);
                } else {
                    counters.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
            last_sample = Instant::now();
        }
        if shutdown
            || (buffer.len() >= settings.max_batch_bytes.saturating_sub(MAX_EVENT)
                && last_flush.elapsed() >= Duration::from_secs(1))
            || last_flush.elapsed() >= Duration::from_secs(settings.flush_seconds)
        {
            if !buffer.is_empty() {
                match write_batch(spool, &buffer, counters) {
                    Ok(_) => buffer.clear(),
                    Err(error) => {
                        counters.local_failures.fetch_add(1, Ordering::Relaxed);
                        if let Ok(mut last) = counters.last_local_error.lock() {
                            *last =
                                Some(format!("{:?}; os={:?}", error.kind(), error.raw_os_error()));
                        }
                    }
                }
            }
            last_flush = Instant::now();
        }
        if shutdown {
            return;
        }
    }
}

fn write_batch(root: &Path, bytes: &[u8], counters: &Counters) -> io::Result<PathBuf> {
    let now = Local::now();
    let date = root.join(now.format("%Y%m%d").to_string());
    fs::create_dir_all(&date)?;
    let name = format!(
        "{}-{}-{}",
        now.format("%Y%m%d%H%M%S%6f"),
        std::process::id(),
        Uuid::new_v4()
    );
    let partial = date.join(format!("{name}.partial"));
    let finished = date.join(format!("{name}.jsonl"));
    let mut created = false;
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&partial)?;
        created = true;
        file.write_all(bytes)?;
        serde_json::to_writer(
            &mut file,
            &json!({"kind":"batch_summary","collector":counters_value(counters)}),
        )?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        fs::rename(&partial, &finished)?;
        Ok(finished)
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(&partial);
    }
    result
}

fn export_ready(spool: &Path, destination: &Path, counters: &Counters) {
    let Ok(days) = fs::read_dir(spool) else {
        return;
    };
    for day in days.flatten() {
        if !day.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Ok(files) = fs::read_dir(day.path()) else {
            continue;
        };
        for entry in files.flatten() {
            if entry.path().extension().is_none_or(|ext| ext != "jsonl")
                || !entry.file_type().is_ok_and(|t| t.is_file())
            {
                continue;
            }
            let result = export_file(
                &entry.path(),
                &destination.join(day.file_name()).join(entry.file_name()),
            );
            match result {
                Ok(()) => {
                    counters.exported.fetch_add(1, Ordering::Relaxed);
                }
                Err(error) => {
                    counters.export_failures.fetch_add(1, Ordering::Relaxed);
                    if let Ok(mut last) = counters.last_export_error.lock() {
                        *last = Some(format!("{:?}; os={:?}", error.kind(), error.raw_os_error()));
                    }
                    // Another process may already be publishing this batch.
                    // Preserve the local source without starving other files.
                    if error.kind() != io::ErrorKind::AlreadyExists {
                        return;
                    }
                }
            }
        }
    }
}

fn export_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(
        destination
            .parent()
            .ok_or_else(|| io::Error::other("destination has no parent"))?,
    )?;
    let mut input = fs::File::open(source)?;
    // create_new: never append to or overwrite a published streaming-drive file.
    let mut target = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let copied = (|| {
        io::copy(&mut input, &mut target)?;
        target.flush()?;
        Ok::<_, io::Error>(())
    })();
    drop(target);
    if let Err(error) = copied {
        let _ = fs::remove_file(destination);
        return Err(error);
    }
    // This is a successful local filesystem copy, not a cloud upload ACK.
    fs::remove_file(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::prelude::*;

    #[test]
    fn settings_bounds_and_unknown_keys_are_rejected() {
        let root = Path::new("/spool");
        assert!(
            Settings {
                flush_seconds: 0,
                ..Settings::default()
            }
            .validate(root)
            .is_err()
        );
        assert!(toml::from_str::<Settings>("enable = true").is_err());
        assert!(
            Settings {
                destination: Some(root.join("inside")),
                ..Settings::default()
            }
            .validate(root)
            .is_err()
        );
    }

    #[test]
    fn batches_are_unique_immutable_and_export_does_not_overwrite() {
        let root = tempfile::tempdir().unwrap();
        let counters = Counters::default();
        let a = write_batch(root.path(), b"one\n", &counters).unwrap();
        let before = fs::read(&a).unwrap();
        let b = write_batch(root.path(), b"two\n", &counters).unwrap();
        assert_ne!(a, b);
        assert_eq!(before, fs::read(&a).unwrap());
        let target = root.path().join("export/one.jsonl");
        export_file(&a, &target).unwrap();
        assert_eq!(before, fs::read(&target).unwrap());
        assert!(!a.exists());
        assert!(export_file(&b, &target).is_err());
        assert!(b.exists());
        assert_eq!(before, fs::read(&target).unwrap());
    }

    #[test]
    fn tracing_parent_correlation_matches_contextual_explicit_and_root_spans() {
        let (sender, receiver) = mpsc::sync_channel(64);
        let counters = Arc::new(Counters::default());
        let layer = BatchLayer {
            sender,
            counters: counters.clone(),
            settings: Settings {
                enabled: true,
                ..Settings::default()
            },
        };
        let subscriber = tracing_subscriber::registry().with(layer);
        let (outer_id, implicit_id, detached_id, explicit_id) =
            tracing::subscriber::with_default(subscriber, || {
                let outer = tracing::info_span!(target: "codex_test", "outer");
                let _outer_scope = outer.enter();
                let implicit = tracing::info_span!(target: "codex_test", "implicit");
                {
                    let _child_scope = implicit.enter();
                    tracing::info!(target: "codex_test", count = 1_u64);
                }
                let detached = tracing::info_span!(target: "codex_test", parent: None, "detached");
                let explicit =
                    tracing::info_span!(target: "codex_test", parent: &detached, "explicit");
                tracing::info!(target: "codex_test", parent: &explicit, count = 2_u64);
                tracing::info!(target: "codex_test", parent: None, count = 3_u64);
                (
                    outer.id().unwrap().into_u64(),
                    implicit.id().unwrap().into_u64(),
                    detached.id().unwrap().into_u64(),
                    explicit.id().unwrap().into_u64(),
                )
            });
        let rows = receiver
            .try_iter()
            .filter_map(|command| match command {
                Command::Event(bytes) => Some(serde_json::from_slice::<Value>(&bytes).unwrap()),
                Command::Shutdown => None,
            })
            .collect::<Vec<_>>();
        let opened = |name| {
            rows.iter()
                .find(|row| row["kind"] == "span_open" && row["name"] == name)
                .unwrap()
        };
        assert_eq!(opened("outer")["span"], json!(outer_id));
        assert_eq!(opened("outer")["parent"], Value::Null);
        assert_eq!(opened("implicit")["parent"], json!(outer_id));
        assert_eq!(opened("detached")["span"], json!(detached_id));
        assert_eq!(opened("detached")["parent"], Value::Null);
        assert_eq!(opened("explicit")["parent"], json!(detached_id));
        let event_span = |count| {
            rows.iter()
                .find(|row| row["kind"] == "event" && row["fields"]["count"] == json!(count))
                .unwrap()["span"]
                .clone()
        };
        assert_eq!(event_span(1_u64), json!(implicit_id));
        assert_eq!(event_span(2_u64), json!(explicit_id));
        assert_eq!(event_span(3_u64), Value::Null);
        assert_eq!(counters.dropped.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn tracing_boundary_keeps_ids_and_durations_but_never_secret_payloads() {
        let root = tempfile::tempdir().unwrap();
        let (layer, guard) = start_with(
            root.path().into(),
            Settings {
                enabled: true,
                process: false,
                ..Settings::default()
            },
        )
        .unwrap();
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(target:"codex_test", "rpc", rpc.method="config/read", rpc.request_id=%7_u64, message="secret-prompt", authorization="Bearer secret-token");
            let _enter = span.enter();
            tracing::info!(target:"codex_test", input_tokens=42_u64, prompt="private text", "secret freeform");
        });
        drop(guard);
        let text = fs::read_dir(root.path())
            .unwrap()
            .flat_map(|d| fs::read_dir(d.unwrap().path()).unwrap())
            .map(|f| fs::read_to_string(f.unwrap().path()).unwrap())
            .collect::<String>();
        assert!(text.contains("config/read"));
        assert!(text.contains("elapsed_us"));
        assert!(text.contains("\"rpc.request_id\":\"7\""));
        assert!(text.contains("42"));
        for secret in [
            "secret-prompt",
            "secret-token",
            "private text",
            "secret freeform",
        ] {
            assert!(!text.contains(secret));
        }
    }

    #[test]
    fn unavailable_destination_preserves_local_batch_for_later_retry() {
        let root = tempfile::tempdir().unwrap();
        let counters = Counters::default();
        let source = write_batch(root.path(), b"retained\n", &counters).unwrap();
        let not_directory = root.path().join("offline");
        fs::write(&not_directory, "x").unwrap();
        assert!(export_file(&source, &not_directory.join("batch")).is_err());
        assert!(source.exists());
        export_file(&source, &root.path().join("online/batch")).unwrap();
        assert!(!source.exists());
    }

    #[test]
    fn backpressure_is_nonblocking_and_loss_is_counted() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        let counters = Arc::new(Counters::default());
        let layer = BatchLayer {
            sender,
            counters: counters.clone(),
            settings: Settings::default(),
        };
        layer.submit(json!({"kind":"first"}));
        layer.submit(json!({"kind":"overflow"}));
        assert_eq!(counters.dropped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn existing_export_collision_does_not_starve_other_batches() {
        let root = tempfile::tempdir().unwrap();
        let spool = root.path().join("spool");
        let destination = root.path().join("drive");
        let counters = Counters::default();
        let first = write_batch(&spool, b"first\n", &counters).unwrap();
        let second = write_batch(&spool, b"second\n", &counters).unwrap();
        let day = first.parent().unwrap().file_name().unwrap();
        let blocked = destination.join(day).join(first.file_name().unwrap());
        fs::create_dir_all(blocked.parent().unwrap()).unwrap();
        fs::write(&blocked, b"already published\n").unwrap();
        export_ready(&spool, &destination, &counters);
        assert!(first.exists());
        assert_eq!(fs::read(&blocked).unwrap(), b"already published\n");
        assert!(!second.exists());
        assert_eq!(counters.exported.load(Ordering::Relaxed), 1);
        assert!(
            counters_value(&counters)["last_export_error"]
                .as_str()
                .unwrap()
                .contains("AlreadyExists")
        );
    }

    #[test]
    #[ignore = "helper subprocess for competing_exporters_preserve_the_loser"]
    fn exporter_child() {
        let source = PathBuf::from(std::env::var_os("CODEX_TEST_EXPORT_SOURCE").unwrap());
        let target = PathBuf::from(std::env::var_os("CODEX_TEST_EXPORT_TARGET").unwrap());
        match export_file(&source, &target) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => panic!("export failed: {error}"),
        }
    }

    #[test]
    fn competing_exporters_preserve_the_loser() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("drive/batch.jsonl");
        let sources = [root.path().join("one"), root.path().join("two")];
        for (index, source) in sources.iter().enumerate() {
            fs::write(source, format!("process-{index}")).unwrap();
        }
        let mut children = Vec::new();
        for source in &sources {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", "batch_log::tests::exporter_child", "--ignored"])
                .env("CODEX_TEST_EXPORT_SOURCE", source)
                .env("CODEX_TEST_EXPORT_TARGET", &target)
                .stdout(std::process::Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            children.push(command.spawn().unwrap());
        }
        for mut child in children {
            assert!(child.wait().unwrap().success());
        }
        assert_eq!(sources.iter().filter(|source| source.exists()).count(), 1);
        let published = fs::read_to_string(&target).unwrap();
        assert!(published == "process-0" || published == "process-1");
        assert_eq!(
            published,
            if sources[0].exists() {
                "process-1"
            } else {
                "process-0"
            }
        );
    }
}

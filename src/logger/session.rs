use crate::inspector::{InspectorBus, InspectorWriter};
use crate::logger::{
    binary::BinaryLogger,
    flow::FlowLogger,
    stream::{LogStreamHandle, LogStreamLayer},
};
use crate::schema::{InspectorEntry, LogEntry};
use anyhow::{Context, Result};
use chrono::Local;
use std::{
    collections::HashMap,
    fmt as stdfmt,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};
use tokio::sync::broadcast;
use tracing::{Event, Subscriber};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    filter::Targets,
    fmt::{
        self,
        format::{DefaultFields, Writer},
        time::{ChronoLocal, FormatTime},
        FmtContext, FormatEvent, FormatFields,
    },
    layer::SubscriberExt,
    registry::LookupSpan,
    util::SubscriberInitExt,
    EnvFilter, Layer, Registry,
};

/// Compact event formatter for file outputs.
///
/// Renders `TIMESTAMP LEVEL target file:line: fields` — deliberately drops
/// the span ancestry list that the default `Full` formatter prefixes onto
/// every event. Third-party crates (e.g. `hudsucker`) wrap our handlers in
/// nested `#[instrument]` spans, producing prefixes longer than the actual
/// message; we don't need them in the file.
struct CompactNoSpans {
    timer: ChronoLocal,
}

fn quiet_protocol_noise(filter: EnvFilter) -> EnvFilter {
    // chromiumoxide 0.9 warns for harmless CDP event variants that it does not
    // model. Akagi's own CDP loop reports terminal failures, so retain errors
    // from the dependency instead of writing hundreds of identical warnings.
    filter.add_directive(
        "chromiumoxide::handler=error"
            .parse()
            .expect("static tracing directive"),
    )
}

impl<S, N> FormatEvent<S, N> for CompactNoSpans
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> stdfmt::Result {
        self.timer.format_time(&mut writer)?;
        let meta = event.metadata();
        write!(writer, " {:>5} {}", meta.level(), meta.target())?;
        if let (Some(file), Some(line)) = (meta.file(), meta.line()) {
            write!(writer, " {file}:{line}")?;
        }
        write!(writer, ": ")?;
        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

/// One named target → one log file. Events whose `tracing` target matches
/// `prefix` (longest-prefix rule) are also written to `<name>.log`.
#[derive(Debug, Clone)]
pub struct LogTarget {
    pub name: String,
    pub prefix: String,
}

impl LogTarget {
    pub fn new(name: impl Into<String>, prefix: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            prefix: prefix.into(),
        }
    }
}

const SENSITIVE_README: &str = "\
Files in this folder hold every captured WebSocket frame in full, which can
include login tokens and other account credentials. Do not post them
publicly. Use Logs > Pack logs to share a session: it leaves this folder out,
or encrypts it with a password you choose.
";

/// Active logging session. Holds the session directory, registered binary
/// loggers, and tracing-appender worker guards (must stay alive for log
/// flushing).
pub struct Session {
    dir: PathBuf,
    binary_loggers: RwLock<HashMap<String, Arc<BinaryLogger>>>,
    _guards: Vec<WorkerGuard>,
    /// Handle to the JSONL + broadcast layer. Kept on `Session` so the
    /// IPC layer can call `subscribe_log_events` long after init, and so
    /// the file handle survives any errant drops of the layer itself.
    stream: LogStreamHandle,
    /// Inspector writer (frames / mjai / bot reactions). Cloned out and
    /// passed to every emitter (`Session::inspector()` returns a clone).
    inspector_writer: InspectorWriter,
    /// Inspector broadcast sender. `subscribe_inspector` IPC grabs a
    /// receiver from this. Kept separately so subscribers don't have to
    /// touch the writer.
    inspector_bus: InspectorBus,
}

impl Session {
    pub fn init(
        log_root: &Path,
        default_level: &str,
        all_level: &str,
        targets: &[LogTarget],
    ) -> Result<Self> {
        let ts = Local::now().format("%Y%m%d-%H%M%S").to_string();
        let dir = log_root.join(&ts);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create log session dir {}", dir.display()))?;

        let timer_fmt = "%Y-%m-%d %H:%M:%S%.3f".to_string();
        let mut guards: Vec<WorkerGuard> = Vec::new();
        let mut layers: Vec<Box<dyn Layer<Registry> + Send + Sync>> = Vec::new();

        // Console (stderr): env-controlled level, ANSI on.
        let env_filter = quiet_protocol_noise(
            EnvFilter::try_from_default_env()
                .or_else(|_| EnvFilter::try_new(default_level))
                .unwrap_or_else(|_| EnvFilter::new("info")),
        );
        layers.push(
            fmt::layer()
                .with_timer(ChronoLocal::new(timer_fmt.clone()))
                .with_target(true)
                .with_file(true)
                .with_line_number(true)
                .with_writer(std::io::stderr)
                .with_filter(env_filter)
                .boxed(),
        );

        // Combined all.log — captures every event regardless of target.
        let all_path = dir.join("all.log");
        let all_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&all_path)
            .with_context(|| format!("Failed to open {}", all_path.display()))?;
        let (all_writer, all_guard) = tracing_appender::non_blocking(all_file);
        guards.push(all_guard);
        let all_filter = quiet_protocol_noise(
            EnvFilter::try_new(all_level).unwrap_or_else(|_| EnvFilter::new("info")),
        );
        layers.push(
            fmt::layer()
                .event_format(CompactNoSpans {
                    timer: ChronoLocal::new(timer_fmt.clone()),
                })
                .fmt_fields(DefaultFields::new())
                .with_ansi(false)
                .with_writer(all_writer)
                .with_filter(all_filter)
                .boxed(),
        );

        // Combined all.jsonl — same severity filter as `all.log`, but the
        // line shape is `serde_json::to_writer(LogEntry)`. The layer also
        // broadcasts each entry on a tokio channel so the frontend log
        // viewer can live-tail without polling. Keep `all.log` in parallel
        // for humans tailing from a terminal.
        let jsonl_path = dir.join("all.jsonl");
        let (stream_layer, stream_handle) = LogStreamLayer::open(&jsonl_path, 1024)
            .with_context(|| format!("Failed to open {}", jsonl_path.display()))?;
        let jsonl_filter = quiet_protocol_noise(
            EnvFilter::try_new(all_level).unwrap_or_else(|_| EnvFilter::new("info")),
        );
        layers.push(stream_layer.with_filter(jsonl_filter).boxed());

        // Inspector pipeline timeline. Separate file from `all.jsonl`
        // because the events are different — these are game-data records
        // (frames, mjai events, bot reactions), not application logs —
        // and conflating them would multiply file size and complicate
        // filtering on either side.
        let sensitive = dir.join(super::SENSITIVE_DIR);
        std::fs::create_dir_all(&sensitive)
            .with_context(|| format!("Failed to create {}", sensitive.display()))?;
        let _ = std::fs::write(sensitive.join("README.txt"), SENSITIVE_README);
        let inspector_path = dir.join("inspector.jsonl");
        let (inspector_writer, inspector_bus) =
            InspectorWriter::open(&inspector_path, &sensitive.join("inspector.jsonl"), 1024)
                .with_context(|| format!("Failed to open {}", inspector_path.display()))?;

        // Per-target files.
        for t in targets {
            let path = dir.join(format!("{}.log", t.name));
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .with_context(|| format!("Failed to open {}", path.display()))?;
            let (writer, guard) = tracing_appender::non_blocking(file);
            guards.push(guard);
            let filter = Targets::new().with_target(t.prefix.clone(), tracing::Level::TRACE);
            layers.push(
                fmt::layer()
                    .event_format(CompactNoSpans {
                        timer: ChronoLocal::new(timer_fmt.clone()),
                    })
                    .fmt_fields(DefaultFields::new())
                    .with_ansi(false)
                    .with_writer(writer)
                    .with_filter(filter)
                    .boxed(),
            );
        }

        tracing_subscriber::registry()
            .with(layers)
            .try_init()
            .context("Failed to install tracing subscriber")?;

        Ok(Self {
            dir,
            binary_loggers: RwLock::new(HashMap::new()),
            _guards: guards,
            stream: stream_handle,
            inspector_writer,
            inspector_bus,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Parent of `dir()` — the configured log root that holds every
    /// session sub-directory. Returned for the in-app session picker so
    /// it can list sibling runs of the current process.
    pub fn root(&self) -> &Path {
        // `dir` is always `<root>/<YYYYMMDD-HHMMSS>` per `Session::init`,
        // so `parent()` is always `Some` in practice. Fall back to the
        // session dir itself if it isn't (e.g. the OS handed us a
        // root-level path), which still produces a valid query target.
        self.dir.parent().unwrap_or(&self.dir)
    }

    /// Subscribe to the live broadcast of `LogEntry` events. The IPC
    /// `subscribe_log_events` command grabs one of these per frontend
    /// channel; the stream is lossy under load (consumer-side `Lagged(n)`
    /// surfaces as a synthetic warn entry to keep the UI honest).
    pub fn subscribe(&self) -> broadcast::Receiver<LogEntry> {
        self.stream.subscribe()
    }

    /// Clone of the inspector writer. Call sites are the proxy handler,
    /// chromium capture, mjai-bus subscriber, and bot manager. Cheap
    /// (Arc<Inner>); each clone independently writes to disk + broadcast.
    pub fn inspector(&self) -> InspectorWriter {
        self.inspector_writer.clone()
    }

    /// Subscribe to the live broadcast of `InspectorEntry` rows. Used by
    /// the `subscribe_inspector` IPC command; same lossy semantics as the
    /// log stream.
    pub fn subscribe_inspector(&self) -> broadcast::Receiver<InspectorEntry> {
        self.inspector_bus.subscribe()
    }

    /// Create a fresh flow logger writing to
    /// `<session>/<subdir>/<file_name>`. The caller supplies the full
    /// filename (extension included). Each call opens a new file handle;
    /// callers (e.g. one per WebSocket flow) own the returned `Arc` and
    /// drop it when the flow ends.
    pub fn flow_logger(
        &self,
        subdir: &str,
        file_name: &str,
        label: impl Into<String>,
    ) -> Result<Arc<FlowLogger>> {
        Ok(Arc::new(FlowLogger::new(
            &self.dir, subdir, file_name, label,
        )?))
    }

    /// Get-or-create a binary logger by name. The file lives at
    /// `<session>/sensitive/<name>.binlog`: it holds every frame verbatim.
    pub fn binary_logger(&self, name: &str) -> Result<Arc<BinaryLogger>> {
        if let Some(existing) = self
            .binary_loggers
            .read()
            .expect("binary logger map poisoned")
            .get(name)
        {
            return Ok(existing.clone());
        }
        let mut w = self
            .binary_loggers
            .write()
            .expect("binary logger map poisoned");
        if let Some(existing) = w.get(name) {
            return Ok(existing.clone());
        }
        let logger = Arc::new(BinaryLogger::new(
            &self.dir.join(super::SENSITIVE_DIR),
            name,
        )?);
        w.insert(name.to_string(), logger.clone());
        Ok(logger)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{self, Write},
        sync::{Arc, Mutex},
    };
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone, Default)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    struct SharedWriterGuard(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriterGuard {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("log buffer poisoned").write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for SharedWriter {
        type Writer = SharedWriterGuard;

        fn make_writer(&'a self) -> Self::Writer {
            SharedWriterGuard(self.0.clone())
        }
    }

    #[test]
    fn chromium_handler_warning_is_filtered_but_errors_and_app_warnings_remain() {
        let output = SharedWriter::default();
        let subscriber = tracing_subscriber::registry().with(
            fmt::layer()
                .without_time()
                .with_ansi(false)
                .with_writer(output.clone())
                .with_filter(quiet_protocol_noise(EnvFilter::new("trace"))),
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(target: "chromiumoxide::handler", "protocol noise");
            tracing::error!(target: "chromiumoxide::handler", "terminal chromium failure");
            tracing::warn!(target: "akagi::logger_test", "actionable app warning");
        });

        let rendered = String::from_utf8(output.0.lock().expect("log buffer poisoned").to_vec())
            .expect("tracing output should be utf-8");
        assert!(!rendered.contains("protocol noise"), "{rendered}");
        assert!(rendered.contains("terminal chromium failure"), "{rendered}");
        assert!(rendered.contains("actionable app warning"), "{rendered}");
    }
}

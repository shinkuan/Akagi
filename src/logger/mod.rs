mod binary;
mod flow;
pub mod pack;
mod session;
mod stream;

/// Session subdirectory for everything that may hold account credentials:
/// the full WS frames (`*.binlog`, the full `inspector.jsonl`). The rest of
/// a session is shareable. [`pack`] encrypts this part on request.
pub const SENSITIVE_DIR: &str = "sensitive";

pub use binary::BinaryLogger;
pub use flow::FlowLogger;
pub use session::{LogTarget, Session};
pub use stream::LogStreamHandle;

use anyhow::Result;
use std::path::Path;

/// Initialise the logging subsystem and return an active `Session`.
///
/// Creates `<log_root>/<YYYYMMDD-HHMMSS>/` as the session directory and
/// installs a tracing subscriber with three kinds of layers:
///
/// - stderr console (env-controlled level, defaults to `default_level`)
/// - combined `all.log` (severity-filtered by `all_level`, accepts the same
///   syntax as `RUST_LOG` / `EnvFilter`)
/// - one `<target.name>.log` per entry in `targets`, filtered by tracing
///   target prefix (e.g. `akagi::proxy` matches the proxy module tree)
///
/// The returned `Session` must be kept alive for the lifetime of the app —
/// dropping it flushes pending writes and tears down the file appenders.
pub fn init(
    log_root: &Path,
    default_level: &str,
    all_level: &str,
    targets: &[LogTarget],
) -> Result<Session> {
    Session::init(log_root, default_level, all_level, targets)
}

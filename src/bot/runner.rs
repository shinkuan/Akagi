//! `BotRunner` trait + `SubprocessBot` impl.
//!
//! `SubprocessBot` spawns the bot's `bot.py` under the runtime-managed
//! venv, talks JSONL over stdin/stdout, and pumps stderr into `tracing`
//! with a `bot=<name>` field so logs are searchable. One long-lived child
//! per game; `reset()` ends the current game and respawns for the next.

use crate::bot::runtime::PythonRuntime;
use crate::bot::types::BotResponse;
use crate::schema::MjaiEvent;
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tracing::{info, warn};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
#[cfg(target_os = "windows")]
#[cfg(target_os = "windows")]

/// Default per-call timeout. Mahjong turn budget is several seconds; 5 s
/// is well above legitimate NN inference cost (Mortal ≈ 100 ms) and below
/// any user-perceptible hang.
const DEFAULT_REACT_TIMEOUT_MS: u64 = 5_000;

/// Grace period for graceful shutdown on `reset()`. End_game is written,
/// child usually exits within a few hundred ms; we wait this long before
/// SIGKILL'ing.
const RESET_GRACE_MS: u64 = 500;

/// One bot instance, alive across one game.
///
/// Implementations own the transport (subprocess pipe, in-process Python,
/// ...) and any per-game state. Calls are sequential — no internal queueing.
#[async_trait]
pub trait BotRunner: Send {
    /// Push a batch of events; return the bot's reaction.
    ///
    /// Mjai contract: bot sees every event but only "reacts" at decision
    /// points. When no action is owed, the bot returns
    /// `MjaiEvent::None` wrapped in a `BotResponse`.
    async fn react(&mut self, events: &[MjaiEvent]) -> Result<BotResponse>;

    /// Tear down and respawn for a new game.
    async fn reset(&mut self) -> Result<()>;
}

/// Subprocess-backed bot runner.
///
/// Pipes:
/// - stdin: JSON array per line (one event batch).
/// - stdout: JSON object per line (one mjai action).
/// - stderr: pumped line-by-line into `tracing` at INFO with a
///   `bot=<name>` field.
pub struct SubprocessBot {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    bot_dir: PathBuf,
    bot_name: String,
    runtime: PythonRuntime,
    actor_id: u8,
    react_timeout: std::time::Duration,
}

impl SubprocessBot {
    /// Production spawn: `uv sync` if needed, then launch under the venv
    /// interpreter.
    ///
    /// Invocation is `python bot.py <actor_id>` — `actor_id` is also
    /// exposed via the `AKAGI_PLAYER_ID` env var. Argv form matches the
    /// mjai.app convention so bots written for that platform run
    /// unmodified.
    pub async fn spawn(runtime: &PythonRuntime, bot_dir: &Path, actor_id: u8) -> Result<Self> {
        runtime.ensure_synced(bot_dir).await?;
        let mut cmd = runtime.command_for(bot_dir, &["bot.py"]);
        cmd.arg(actor_id.to_string());
        Self::spawn_with_command(cmd, runtime.clone(), bot_dir, actor_id).await
    }

    /// Test / advanced spawn: caller supplies a fully-built `Command`
    /// (e.g. system `python3` for unit tests). Skips `ensure_synced`.
    pub async fn spawn_with_command(
        mut cmd: Command,
        runtime: PythonRuntime,
        bot_dir: &Path,
        actor_id: u8,
    ) -> Result<Self> {
        let bot_name = bot_dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("bot")
            .to_owned();

        cmd.env("AKAGI_PLAYER_ID", actor_id.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

        let mut child = cmd
            .spawn()
            .with_context(|| format!("spawn bot {bot_name}"))?;
        let stdin = child
            .stdin
            .take()
            .context("child stdin missing — Stdio::piped() should have set it")?;
        let stdout = BufReader::new(
            child
                .stdout
                .take()
                .context("child stdout missing — Stdio::piped() should have set it")?,
        );
        if let Some(stderr) = child.stderr.take() {
            spawn_stderr_pump(stderr, bot_name.clone());
        }

        info!(bot = %bot_name, actor_id, "bot subprocess spawned");

        Ok(Self {
            child,
            stdin,
            stdout,
            bot_dir: bot_dir.to_owned(),
            bot_name,
            runtime,
            actor_id,
            react_timeout: std::time::Duration::from_millis(DEFAULT_REACT_TIMEOUT_MS),
        })
    }

    pub fn bot_name(&self) -> &str {
        &self.bot_name
    }

    pub fn actor_id(&self) -> u8 {
        self.actor_id
    }

    /// Override the default 5 s react timeout. Mostly for tests.
    pub fn set_react_timeout(&mut self, t: std::time::Duration) {
        self.react_timeout = t;
    }
}

#[async_trait]
impl BotRunner for SubprocessBot {
    async fn react(&mut self, events: &[MjaiEvent]) -> Result<BotResponse> {
        let line = serde_json::to_string(events)?;
        self.stdin
            .write_all(line.as_bytes())
            .await
            .context("write events to bot stdin")?;
        self.stdin.write_all(b"\n").await.context("write newline")?;
        self.stdin.flush().await.context("flush bot stdin")?;

        let mut buf = String::new();
        let read = tokio::time::timeout(self.react_timeout, self.stdout.read_line(&mut buf)).await;
        let n = match read {
            Ok(r) => r.context("read bot stdout")?,
            Err(_) => bail!(
                "bot {} react() timed out after {:?}",
                self.bot_name,
                self.react_timeout
            ),
        };
        if n == 0 {
            bail!("bot {} stdout EOF (process exited)", self.bot_name);
        }

        // {"type":"none"} flows through MjaiEvent::None — no special case.
        let resp: BotResponse = serde_json::from_str(buf.trim())
            .with_context(|| format!("bot {} reply malformed: {}", self.bot_name, buf.trim()))?;
        Ok(resp)
    }

    async fn reset(&mut self) -> Result<()> {
        // Best-effort graceful shutdown: write end_game, give the bot
        // RESET_GRACE_MS to exit cleanly, then SIGKILL.
        let _ = self.stdin.write_all(b"[{\"type\":\"end_game\"}]\n").await;
        let _ = self.stdin.flush().await;

        let waited = tokio::time::timeout(
            std::time::Duration::from_millis(RESET_GRACE_MS),
            self.child.wait(),
        )
        .await;
        if waited.is_err() {
            warn!(bot = %self.bot_name, "bot did not exit within grace period; killing");
            let _ = self.child.start_kill();
            let _ = self.child.wait().await;
        }

        let new = SubprocessBot::spawn(&self.runtime, &self.bot_dir, self.actor_id).await?;
        // Replace our state with the freshly-spawned instance.
        let SubprocessBot {
            child,
            stdin,
            stdout,
            react_timeout,
            ..
        } = new;
        self.child = child;
        self.stdin = stdin;
        self.stdout = stdout;
        self.react_timeout = react_timeout;
        Ok(())
    }
}

fn spawn_stderr_pump(stderr: ChildStderr, bot_name: String) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => info!(bot = %bot_name, "{line}"),
                Ok(None) => break,
                Err(e) => {
                    warn!(bot = %bot_name, "stderr pump error: {e}");
                    break;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::runtime::RuntimeMode;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn system_python() -> Option<PathBuf> {
        which::which("python3")
            .or_else(|_| which::which("python"))
            .ok()
    }

    /// Tiny stdlib-only echo bot. Returns `{"type":"none"}` for any batch
    /// and exits cleanly on `end_game`. No pyproject — we use system
    /// python directly via `spawn_with_command`.
    fn write_echo_bot(dir: &Path) {
        std::fs::write(
            dir.join("bot.py"),
            r#"import json, sys
for line in sys.stdin:
    try:
        events = json.loads(line)
    except Exception as e:
        print(f"parse error: {e}", file=sys.stderr, flush=True)
        print('{"type":"none"}', flush=True)
        continue
    print('{"type":"none"}', flush=True)
    if any(e.get("type") == "end_game" for e in events):
        break
"#,
        )
        .unwrap();
    }

    fn dummy_runtime(python: &Path) -> PythonRuntime {
        PythonRuntime::from_paths(
            python.to_owned(),
            PathBuf::from("/dev/null/uv"),
            RuntimeMode::System,
        )
    }

    async fn spawn_echo(bot_dir: &Path) -> Result<SubprocessBot> {
        let py = system_python().expect("test requires python on PATH");
        let mut cmd = Command::new(&py);
        cmd.current_dir(bot_dir).arg("bot.py");
        SubprocessBot::spawn_with_command(cmd, dummy_runtime(&py), bot_dir, 0).await
    }

    #[tokio::test]
    async fn echo_bot_returns_none() {
        if system_python().is_none() {
            eprintln!("skip: no python3 on PATH");
            return;
        }
        let tmp = TempDir::new().unwrap();
        write_echo_bot(tmp.path());

        let mut bot = spawn_echo(tmp.path()).await.unwrap();
        let resp = bot
            .react(&[MjaiEvent::StartGame {
                names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
                kyoku_first: None,
                aka_flag: None,
                id: Some(0),
                num_players: 4,
            }])
            .await
            .unwrap();
        assert!(matches!(resp.action, MjaiEvent::None));
        assert!(resp.meta.is_none());

        // Graceful shutdown: end_game line lets the bot break its loop.
        let _ = bot.react(&[MjaiEvent::EndGame]).await.unwrap();
    }

    #[tokio::test]
    async fn react_times_out_when_bot_silent() {
        if system_python().is_none() {
            eprintln!("skip: no python3 on PATH");
            return;
        }
        let tmp = TempDir::new().unwrap();
        // Bot reads stdin but never replies — react() must time out.
        std::fs::write(
            tmp.path().join("bot.py"),
            r#"import sys
for line in sys.stdin:
    pass
"#,
        )
        .unwrap();

        let mut bot = spawn_echo(tmp.path()).await.unwrap();
        bot.set_react_timeout(std::time::Duration::from_millis(200));
        let err = bot.react(&[MjaiEvent::EndGame]).await.unwrap_err();
        assert!(
            err.to_string().contains("timed out"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn malformed_reply_surfaces_error() {
        if system_python().is_none() {
            eprintln!("skip: no python3 on PATH");
            return;
        }
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("bot.py"),
            r#"import sys
for line in sys.stdin:
    print("not json", flush=True)
    break
"#,
        )
        .unwrap();

        let mut bot = spawn_echo(tmp.path()).await.unwrap();
        let err = bot.react(&[MjaiEvent::EndGame]).await.unwrap_err();
        assert!(
            err.to_string().contains("malformed"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn eof_before_reply_surfaces_error() {
        if system_python().is_none() {
            eprintln!("skip: no python3 on PATH");
            return;
        }
        let tmp = TempDir::new().unwrap();
        // Exits immediately without reading.
        std::fs::write(tmp.path().join("bot.py"), "import sys\nsys.exit(0)\n").unwrap();

        let mut bot = spawn_echo(tmp.path()).await.unwrap();
        let err = bot.react(&[MjaiEvent::EndGame]).await.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("EOF") || msg.contains("Broken pipe") || msg.contains("write events"),
            "unexpected error: {err}"
        );
    }
}

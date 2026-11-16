//! The continuation worker belongs to this Akagi process. The webview talks
//! only to Tauri commands; its loopback address and token never enter the UI.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

pub struct NextGameService {
    running: Result<Running, String>,
}

struct Running {
    child: Mutex<Option<Child>>,
    client: reqwest::Client,
    origin: String,
    token: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NextGameControl {
    enabled: bool,
    limited: bool,
    limit: u32,
}

impl NextGameService {
    pub fn start(config_path: &Path, log_dir: &Path) -> Self {
        Self {
            running: Self::spawn(config_path, log_dir).map_err(|e| e.to_string()),
        }
    }

    fn spawn(config_path: &Path, log_dir: &Path) -> anyhow::Result<Running> {
        let exe = std::env::current_exe()?;
        let app_dir = exe
            .parent()
            .ok_or_else(|| anyhow::anyhow!("无法定位 Akagi 安装目录"))?;
        let worker = app_dir.join("next-game");
        let node = worker.join("runtime/node.exe");
        if !node.is_file() || !worker.join("server.mjs").is_file() {
            anyhow::bail!("续局组件缺失，请使用完整的 Akagi 整合版目录");
        }
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let token = format!(
            "{:032x}{:032x}",
            rand::random::<u128>(),
            rand::random::<u128>()
        );
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(15))
            .build()?;
        let mut command = Command::new(node);
        command
            .arg(worker.join("server.mjs"))
            .current_dir(&worker)
            .env("AKAGI_APP_DIR", app_dir)
            .env("AKAGI_CONFIG_PATH", std::path::absolute(config_path)?)
            .env("AKAGI_LOG_DIR", std::path::absolute(log_dir)?)
            .env("AKAGI_NEXT_PORT", port.to_string())
            .env("AKAGI_NEXT_TOKEN", &token)
            .env("AKAGI_NEXT_EMBEDDED", "1")
            .env("AKAGI_PARENT_PID", std::process::id().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        drop(listener);
        let child = command.spawn()?;
        Ok(Running {
            child: Mutex::new(Some(child)),
            client,
            origin: format!("http://127.0.0.1:{port}"),
            token,
        })
    }

    pub fn stop(&self) {
        if let Ok(r) = &self.running {
            if let Ok(mut guard) = r.child.lock() {
                if let Some(mut child) = guard.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }

    async fn request(&self, control: Option<&NextGameControl>) -> Result<Value, String> {
        let r = self.running.as_ref().map_err(Clone::clone)?;
        {
            let mut guard = r.child.lock().map_err(|_| "续局组件状态不可用")?;
            let child = guard.as_mut().ok_or("续局组件已关闭，请重新启动 Akagi")?;
            if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                return Err("续局组件已退出，请重新启动 Akagi".into());
            }
        }
        let request = if let Some(input) = control {
            if input.enabled && !(1..=99).contains(&input.limit) {
                return Err("额外场数必须是 1–99 的整数".into());
            }
            r.client.post(format!("{}/control", r.origin)).json(input)
        } else {
            r.client.get(format!("{}/state", r.origin))
        };
        let response = request
            .header("X-Akagi-Token", &r.token)
            .header("Origin", &r.origin)
            .send()
            .await
            .map_err(|_| "续局组件连接中，请稍后重试".to_string())?;
        let success = response.status().is_success();
        let body: Value = response
            .json()
            .await
            .map_err(|_| "续局组件返回了无效状态")?;
        if !success {
            return Err(body
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("续局操作失败")
                .to_string());
        }
        Ok(body)
    }
}

impl Drop for NextGameService {
    fn drop(&mut self) {
        self.stop();
    }
}

#[tauri::command]
pub async fn get_next_game_status(
    service: tauri::State<'_, NextGameService>,
) -> Result<Value, String> {
    service.request(None).await
}

#[tauri::command]
pub async fn control_next_game(
    service: tauri::State<'_, NextGameService>,
    control: NextGameControl,
) -> Result<Value, String> {
    service.request(Some(&control)).await
}

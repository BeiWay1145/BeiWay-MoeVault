// BeiWay-MoeVault Tauri 桌面壳。
// 启动时拉起 Rust 后端（moevault-app），窗口加载 http://127.0.0.1:9178。
// - dev：spawn ../backend/target/debug/moevault-app.exe
// - prod：spawn sidecar（打包的 moevault-app.exe），需配置 externalBin

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use tauri::Manager;

const BACKEND_URL: &str = "http://127.0.0.1:9178";
const BACKEND_PORT: u16 = 9178;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .setup(|app| {
      // 启动后端
      match start_backend() {
        Ok(child) => {
          // 持有子进程句柄防止被回收（Tauri 退出时 child drop 自动清理）
          let _ = Box::leak(Box::new(child)) as *mut Child;
          // 后台等待后端就绪
          let handle = app.handle().clone();
          thread::spawn(move || {
            wait_for_backend();
            // 加载后端 URL
            if let Some(window) = handle.get_webview_window("main") {
              let _ = window.navigate(BACKEND_URL.parse().unwrap());
            }
          });
        }
        Err(e) => {
          eprintln!("[MoeVault] 后端启动失败: {e}");
        }
      }
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

/// 启动后端进程。
/// - debug：直接跑 backend 的编译产物
/// - release：跑打包的 sidecar（externalBin 同名的 target/release/moevault-app.exe）
fn start_backend() -> std::io::Result<Child> {
  let exe = backend_exe_path();
  let mut cmd = Command::new(&exe);
  cmd
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    // 默认数据目录（当前目录下 data/）
    .current_dir(working_dir());
  eprintln!("[MoeVault] 启动后端: {} ({})", exe.display(), cmd.get_current_dir().map(|d| d.display().to_string()).unwrap_or_default());
  cmd.spawn()
}

/// 定位后端 exe：
/// - debug：workspace 根 ../backend/target/debug/moevault-app.exe（相对 src-tauri）
/// - release：sidecar（tauri 会复制到 target/release/moevault-app.exe）
fn backend_exe_path() -> PathBuf {
  if cfg!(debug_assertions) {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    PathBuf::from(manifest)
      .join("..")
      .join("backend")
      .join("target")
      .join("debug")
      .join(if cfg!(target_os = "windows") { "moevault-app.exe" } else { "moevault-app" })
  } else {
    // sidecar 在 exe 同目录
    let exe_dir = std::env::current_exe()
      .ok()
      .and_then(|p| p.parent().map(|p| p.to_path_buf()))
      .unwrap_or_else(|| PathBuf::from("."));
    exe_dir.join(if cfg!(target_os = "windows") { "moevault-app.exe" } else { "moevault-app" })
  }
}

/// 后端工作目录：库数据放哪？开发期用项目根，生产用 exe 目录。
fn working_dir() -> PathBuf {
  if cfg!(debug_assertions) {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    PathBuf::from(manifest).join("..").join("..")
  } else {
    std::env::current_exe()
      .ok()
      .and_then(|p| p.parent().map(|p| p.to_path_buf()))
      .unwrap_or_else(|| PathBuf::from("."))
  }
}

/// 轮询后端端口直到就绪（最多 30 秒）。
fn wait_for_backend() {
  for _ in 0..150 {
    if port_open(BACKEND_PORT) {
      eprintln!("[MoeVault] 后端就绪: {BACKEND_URL}");
      return;
    }
    thread::sleep(Duration::from_millis(200));
  }
  eprintln!("[MoeVault] 后端 30 秒内未就绪");
}

/// 检查 TCP 端口是否可连接。
fn port_open(port: u16) -> bool {
  use std::net::TcpStream;
  TcpStream::connect_timeout(
    &format!("127.0.0.1:{port}").parse().unwrap(),
    Duration::from_millis(150),
  )
  .is_ok()
}

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
/// 使用 CREATE_NO_WINDOW 隐藏控制台；日志重定向到 <cwd>/backend.log 便于排查。
fn start_backend() -> std::io::Result<Child> {
  let exe = backend_exe_path();
  let dir = working_dir();
  // 日志文件（backend.log 建在数据目录旁）
  let log_path = dir.join("backend.log");
  let log_file = std::fs::OpenOptions::new()
    .create(true)
    .append(true)
    .open(&log_path)?;

  let mut cmd = Command::new(&exe);
  cmd
    .current_dir(&dir)
    .stdout(Stdio::from(log_file.try_clone()?))
    .stderr(Stdio::from(log_file));
  // Windows：隐藏后端控制台窗口
  #[cfg(target_os = "windows")]
  {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
  }
  eprintln!("[MoeVault] 启动后端: {} (cwd: {}, log: {})", exe.display(), dir.display(), log_path.display());
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

/// 轮询后端 /health 直到就绪（最多 30 秒）。
fn wait_for_backend() {
  for _ in 0..150 {
    if backend_healthy() {
      eprintln!("[MoeVault] 后端就绪: {BACKEND_URL}");
      return;
    }
    thread::sleep(Duration::from_millis(200));
  }
  eprintln!("[MoeVault] 后端 30 秒内未就绪");
}

/// 检查后端 /health 是否返回 200。
fn backend_healthy() -> bool {
  match std::net::TcpStream::connect_timeout(
    &format!("127.0.0.1:{BACKEND_PORT}").parse().unwrap(),
    Duration::from_millis(150),
  ) {
    Ok(mut stream) => {
      use std::io::Write;
      // 发送 HTTP 请求并读取响应头，确认 200
      let _ = stream.write_all(
        b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
      );
      use std::io::Read;
      let mut buf = [0u8; 128];
      match stream.read(&mut buf) {
        Ok(n) if n > 0 => {
          let text = String::from_utf8_lossy(&buf[..n]);
          text.contains(" 200 ")
        }
        _ => false,
      }
    }
    Err(_) => false,
  }
}

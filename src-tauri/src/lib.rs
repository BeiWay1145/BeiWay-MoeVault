// BeiWay-MoeVault Tauri 桌面壳。
// 启动时拉起 Rust 后端（moevault-app），窗口加载 http://127.0.0.1:9178。
// - dev：spawn ../backend/target/debug/moevault-app.exe
// - prod：spawn sidecar（打包的 moevault-app.exe），需配置 externalBin

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use tauri::Manager;
use tauri::WindowEvent;

const BACKEND_URL: &str = "http://127.0.0.1:9178";
const BACKEND_PORT: u16 = 9178;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .on_window_event(|window, event| {
      // 增强：关闭窗口 = 最小化到任务栏（后台批量处理不被中断）；再次单击任务栏图标恢复
      if let WindowEvent::CloseRequested { api, .. } = event {
        if window.label() == "main" {
          let _ = window.minimize();
          api.prevent_close();
        }
      }
    })
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
/// - debug：直接跑 backend 的编译产物，静态目录 = workspace frontend/dist
/// - release：跑打包的 sidecar，静态目录 = exe 旁 resources/frontend（bundle.resources 打包）
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
  // 设置静态资源目录（后端托管前端 → 根路径 / 返回 index.html）
  if let Some(static_dir) = frontend_static_dir() {
    cmd.env("MOEVAULT_STATIC_DIR", &static_dir);
    eprintln!("[MoeVault] 前端静态目录: {}", static_dir.display());
  } else {
    eprintln!("[MoeVault] 警告: 未找到前端静态目录，根路径将 404");
  }
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

/// 定位前端静态资源目录。
/// - debug：workspace 根 frontend/dist
/// - release：exe 同目录 frontend/（Tauri bundle.resources map 目标键复制产物）
fn frontend_static_dir() -> Option<PathBuf> {
  if cfg!(debug_assertions) {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let p = PathBuf::from(manifest).join("..").join("frontend").join("dist");
    return p.is_dir().then_some(p);
  }
  let exe_dir = std::env::current_exe()
    .ok()
    .and_then(|p| p.parent().map(|p| p.to_path_buf()))
    .unwrap_or_else(|| PathBuf::from("."));
  // Tauri 2 资源复制到 exe 同级的 map 目标目录（frontend/）
  let p = exe_dir.join("frontend");
  if p.is_dir() {
    return Some(p);
  }
  // 备选：resources/frontend（安装包布局）
  let p2 = exe_dir.join("resources").join("frontend");
  p2.is_dir().then_some(p2)
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

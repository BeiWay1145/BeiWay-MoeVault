// BeiWay-MoeVault Tauri 桌面壳。
// 启动时拉起 Rust 后端（moevault-app），窗口加载 http://127.0.0.1:9178。
// - dev：spawn ../backend/target/debug/moevault-app.exe
// - prod：spawn sidecar（打包的 moevault-app.exe），需配置 externalBin

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};

const BACKEND_URL: &str = "http://127.0.0.1:9178";
const BACKEND_PORT: u16 = 9178;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_opener::init())
    .on_window_event(|window, event| {
      // 关闭窗口：根据后端设置 close_to_tray 决定 最小化到托盘 or 正常退出
      if let WindowEvent::CloseRequested { api, .. } = event {
        if window.label() == "main" {
          let close_to_tray = read_close_to_tray_setting();
          if close_to_tray {
            let _ = window.hide();
            api.prevent_close();
          }
          // 关闭=正常退出：不 prevent_close，窗口关闭后应用退出（后端子进程随之清理）
        }
      }
    })
    .setup(|app| {
      // 托盘图标：单击恢复窗口，右键菜单 显示/退出
      use tauri::menu::{Menu, MenuItem};
      let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
      let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
      let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;

      let _tray = TrayIconBuilder::new()
        .icon(
          app
            .default_window_icon()
            .cloned()
            .expect("默认窗口图标缺失"),
        )
        .tooltip("BeiWay-MoeVault")
        .on_tray_icon_event(|tray, event| {
          if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
          } = event
          {
            let app = tray.app_handle();
            if let Some(win) = app.get_webview_window("main") {
              let _ = win.show();
              let _ = win.unminimize();
              let _ = win.set_focus();
            }
          }
        })
        .menu(&tray_menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
          "show" => {
            if let Some(win) = app.get_webview_window("main") {
              let _ = win.show();
              let _ = win.unminimize();
              let _ = win.set_focus();
            }
          }
          "quit" => {
            app.exit(0);
          }
          _ => {}
        })
        .build(app)
        .expect("托盘图标创建失败");
      std::mem::forget(_tray); // 防止 drop 移除托盘图标

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

/// 读取后端设置 close_to_tray（同步 HTTP GET，localhost 延迟可忽略）。
/// 后端未就绪/读取失败时返回 false（正常退出，避免意外隐藏窗口）。
fn read_close_to_tray_setting() -> bool {
  use std::io::{Read, Write};
  if let Ok(mut stream) = std::net::TcpStream::connect_timeout(
    &format!("127.0.0.1:{BACKEND_PORT}").parse().unwrap(),
    std::time::Duration::from_millis(500),
  ) {
    let _ = stream.write_all(
      b"GET /api/v1/settings HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    let mut buf = Vec::new();
    let mut chunk = [0u8; 512];
    loop {
      match stream.read(&mut chunk) {
        Ok(n) if n > 0 => buf.extend_from_slice(&chunk[..n]),
        _ => break,
      }
    }
    // 提取 JSON body（最后一个 \r\n\r\n 之后）
    if let Some(idx) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
      let body = &buf[idx + 4..];
      if let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) {
        return v
          .get("close_to_tray")
          .and_then(|x| x.as_str())
          .map(|s| s == "true")
          .unwrap_or(false);
      }
    }
  }
  false
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

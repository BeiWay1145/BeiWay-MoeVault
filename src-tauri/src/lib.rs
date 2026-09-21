// BeiWay-MoeVault Tauri 桌面壳。
// 启动时拉起 Rust 后端（moevault-app），窗口加载 http://127.0.0.1:9178。
// - dev：spawn ../backend/target/debug/moevault-app.exe
// - prod：spawn sidecar（打包的 moevault-app.exe），需配置 externalBin

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

mod drag_drop;

use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent, RunEvent};

const BACKEND_URL: &str = "http://127.0.0.1:9178";
const BACKEND_PORT: u16 = 9178;

const INFER_URL: &str = "http://127.0.0.1:8001";
const INFER_PORT: u16 = 8001;

/// pip 安装源顺序：清华镜像优先（大陆网络友好），官方 PyPI 兜底。
/// 与 python/setup.bat、python/run_server.bat 的约定保持一致。
const INFER_PIP_INDEXES: &[&str] = &[
  "https://pypi.tuna.tsinghua.edu.cn/simple",
  "https://pypi.org/simple",
];

/// 全局持有后端子进程句柄：托盘退出/应用退出时确保杀掉后端。
static BACKEND_CHILD: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
/// 全局持有推理服务（Python uvicorn）子进程句柄：应用退出时一并杀掉。
static INFER_CHILD: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
/// 推理服务"依赖准备 + 启动"全流程互斥锁：
/// setup 后台线程自动启动与「启动服务」按钮/「一键安装依赖」可能并发触发
/// ensure_infer_venv（pip 安装），串行化避免同一 venv 并发写入损坏。
static INFER_START_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_opener::init())
    .invoke_handler(tauri::generate_handler![
      infer_start,
      infer_stop,
      infer_status,
      infer_install_deps,
      infer_install_gpu,
      infer_gpu_status
    ])
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
      // 托盘图标：单击恢复窗口，右键菜单 显示/开发者工具/退出
      use tauri::menu::{Menu, MenuItem};
      let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
      let devtools_item = MenuItem::with_id(
        app,
        "devtools",
        "开发者工具 (Ctrl+Shift+I)",
        true,
        Some("Ctrl+Shift+I"),
      )?;
      let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
      let tray_menu = Menu::with_items(app, &[&show_item, &devtools_item, &quit_item])?;

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
          "devtools" => {
            // release 构建默认关 DevTools：托盘菜单/快捷键显式打开（BUG4 排查用）
            if let Some(win) = app.get_webview_window("main") {
              let _ = win.open_devtools();
              let _ = win.set_focus();
            }
          }
          "quit" => {
            kill_backend();
            kill_infer();
            app.exit(0);
          }
          _ => {}
        })
        .build(app)
        .expect("托盘图标创建失败");
      std::mem::forget(_tray); // 防止 drop 移除托盘图标

      // Windows 原生拖放：注册 IDropTarget（详见 drag_drop.rs 顶部说明）。
      // wry 0.55.1 在 WebView2 上因 SetAllowExternalDrop(false) 后子窗口无既有 target，
      // 注册被条件短路跳过 → 拖入显示 STOP 拒绝光标且事件永不触发。
      if let Some(win) = app.get_webview_window("main") {
        // 守卫交给 drag_drop 模块自身持有（COM 对象非 Send，不能放进全局 static）
        drag_drop::install(&win);
      }

      // 启动后端
      match start_backend() {
        Ok(child) => {
          // 持有子进程句柄（全局）：托盘退出/应用退出时确保杀掉后端，避免残留占端口
          let slot = BACKEND_CHILD.get_or_init(|| Mutex::new(None));
          *slot.lock().unwrap() = Some(child);
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

      // 启动推理服务（Python uvicorn，端口 8001）：
      // 在后台线程执行——首次运行可能需自动创建 venv / 安装依赖（数分钟），不阻塞窗口加载；
      // 未就绪则降级，窗口照常加载，TopBar 轮询 /health 会在服务就绪后自动变绿。
      thread::spawn(|| {
        match start_infer() {
          Ok(child) => {
            if let Some(child) = child {
              let slot = INFER_CHILD.get_or_init(|| Mutex::new(None));
              *slot.lock().unwrap() = Some(child);
            }
            // 后台等待推理服务就绪（仅记录日志，不阻塞窗口）
            wait_for_infer();
          }
          Err(e) => {
            eprintln!("[MoeVault] 推理服务启动失败: {e}");
          }
        }
      });
      Ok(())
    })
    .build(tauri::generate_context!())
    .expect("error while building tauri application")
    .run(|_app_handle, event| {
      // 应用退出（任何路径：托盘退出/窗口关闭/系统关机）
      if let RunEvent::Exit = event {
        // 增强1：BUG追踪器——退出前请求后端转储日志（后端仍在运行）
        dump_logs_on_exit();
        kill_backend();
        kill_infer();
      }
    });
}

/// 退出前转储日志（BUG追踪器）：请求后端 /api/v1/logs/export 写 txt。
/// 尽力而为：失败不影响退出。
fn dump_logs_on_exit() {
  use std::io::{Read, Write};
  use std::net::TcpStream;
  if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", BACKEND_PORT)) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(3)));
    let req = format!(
      "GET /api/v1/logs/export HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
      BACKEND_PORT
    );
    let _ = stream.write_all(req.as_bytes());
    let mut buf = [0u8; 1024];
    let _ = stream.read(&mut buf);
  }
}

/// 杀掉后端子进程（托盘退出/应用退出时调用，避免残留占 9178 端口）。
fn kill_backend() {
    if let Some(slot) = BACKEND_CHILD.get() {
        if let Some(mut child) = slot.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// 杀掉推理服务子进程（应用退出时调用，避免残留占 8001 端口）。
fn kill_infer() {
    if let Some(slot) = INFER_CHILD.get() {
        if let Some(mut child) = slot.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
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
  // 运行时目录：后端据此定位是否已有迁移数据（不直接使用，仅作诊断透出）
  cmd.env("MOEVAULT_RUNTIME_DIR", runtime_root());
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

/// 检查推理服务 /health 是否返回 200（TCP 直连 8001）。
fn infer_healthy() -> bool {
  match std::net::TcpStream::connect_timeout(
    &format!("127.0.0.1:{INFER_PORT}").parse().unwrap(),
    Duration::from_millis(150),
  ) {
    Ok(mut stream) => {
      use std::io::Write;
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

/// 8001 端口是否被占用（任何进程）。
fn infer_port_occupied() -> bool {
  infer_port_owner_pid().is_some()
}

/// 8001 端口占用进程 PID。
fn infer_port_owner_pid() -> Option<u32> {
  #[cfg(target_os = "windows")]
  {
    let out = std::process::Command::new("netstat")
      .args(["-ano"])
      .output()
      .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
      // TCP    127.0.0.1:8001    0.0.0.0:0    LISTENING    12345
      let parts: Vec<&str> = line.split_whitespace().collect();
      if parts.len() >= 5 && parts[0] == "TCP" && parts[1].ends_with(&format!(":{INFER_PORT}")) && parts[3] == "LISTENING" {
        return parts[4].parse::<u32>().ok();
      }
    }
  }
  None
}

/// 8001 占用进程是否使用系统 Python（非 venv）。
fn infer_port_uses_system_python() -> bool {
  let Some(pid) = infer_port_owner_pid() else {
    return false;
  };
  // 用 PowerShell 拿进程路径（Rust 无 win32 API 依赖，走 powershell 查询）
  let script = format!("(Get-Process -Id {pid} -ErrorAction SilentlyContinue).Path");
  let out = std::process::Command::new("powershell")
    .args(["-NoProfile", "-Command", &script])
    .output()
    .ok();
  if let Some(o) = out {
    let path = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if path.is_empty() {
      return false; // 拿不到路径，不冒险杀
    }
    // venv python 路径含 .venv；系统 python 在 AppData\Local\Programs\Python 或 C:\Python*
    let lower = path.to_lowercase();
    if lower.contains(".venv") || lower.contains("venv") {
      return false;
    }
    if lower.contains("python") {
      return true;
    }
  }
  false
}

/// 杀掉 8001 占用进程。
fn kill_infer_port_owner() {
  if let Some(pid) = infer_port_owner_pid() {
    let _ = std::process::Command::new("taskkill")
      .args(["/PID", &pid.to_string(), "/F"])
      .output();
    eprintln!("[MoeVault] 已终止非 venv 推理实例 (PID {pid})");
  }
}

/// 运行时数据根目录（venv / 模型 / 日志等大体积文件的家）。
///
/// 优先级：
/// 1. 环境变量 `MOEVAULT_RUNTIME_DIR`（可显式指定，便于迁移与测试）
/// 2. 安装根目录下的 `runtime/`（便携式：所有数据随软件走，不占用 C 盘）
/// 3. `%LOCALAPPDATA%\BeiWay-MoeVault`（安装目录不可写时的回退，如只读安装位置）
///
/// 判定方式为**实际写入探测**：在候选目录创建并删除探针文件，成功即采用。
/// 结果缓存，避免每次调用都做磁盘探测。
fn runtime_root() -> PathBuf {
  use std::sync::OnceLock;
  static CACHED: OnceLock<PathBuf> = OnceLock::new();
  CACHED
    .get_or_init(|| {
      // 1) 显式覆盖
      if let Ok(v) = std::env::var("MOEVAULT_RUNTIME_DIR") {
        let p = PathBuf::from(v);
        if ensure_writable(&p) {
          eprintln!("[MoeVault] 运行时目录（环境变量）: {}", p.display());
          return p;
        }
        eprintln!("[MoeVault] 警告: MOEVAULT_RUNTIME_DIR 不可写，改用默认位置");
      }
      // 2) 安装根目录/runtime
      if let Some(root) = install_root() {
        let candidate = root.join("runtime");
        if ensure_writable(&candidate) {
          eprintln!("[MoeVault] 运行时目录（安装根目录）: {}", candidate.display());
          return candidate;
        }
        eprintln!(
          "[MoeVault] 安装目录不可写，回退用户数据目录: {}",
          candidate.display()
        );
      }
      // 3) 用户数据目录回退
      let base = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
      let fallback = base.join("BeiWay-MoeVault");
      let _ = std::fs::create_dir_all(&fallback);
      eprintln!("[MoeVault] 运行时目录（用户数据）: {}", fallback.display());
      fallback
    })
    .clone()
}

/// 安装根目录（exe 所在目录；debug 下为 workspace 根）。
fn install_root() -> Option<PathBuf> {
  if cfg!(debug_assertions) {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    return Some(PathBuf::from(manifest).join(".."));
  }
  std::env::current_exe()
    .ok()
    .and_then(|p| p.parent().map(|p| p.to_path_buf()))
}

/// 探测目录是否可写（创建即删的探针文件）。目录不存在时尝试创建。
fn ensure_writable(dir: &std::path::Path) -> bool {
  if std::fs::create_dir_all(dir).is_err() {
    return false;
  }
  let probe = dir.join(".moevault_write_probe");
  match std::fs::write(&probe, b"1") {
    Ok(()) => {
      let _ = std::fs::remove_file(&probe);
      true
    }
    Err(_) => false,
  }
}

/// 定位推理服务 python 目录（server 包所在目录）。
/// - debug：workspace 根 python/
/// - release：<运行时根目录>\python；每次启动从安装目录资源同步源码
///   （venv/日志/model 等大体积运行期产物留在运行时目录，不污染安装目录也不占 C 盘）
fn infer_python_dir() -> PathBuf {
  if cfg!(debug_assertions) {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    PathBuf::from(manifest).join("..").join("python")
  } else {
    let runtime = runtime_root().join("python");
    // 从安装包资源同步推理服务代码/脚本到运行时目录（幂等，每次启动覆盖旧文件：
    // 升级安装后 server 代码与 bat 随之更新，避免旧版本代码长期残留；
    // copy_dir_recursive 跳过 infer.log/__pycache__/.venv 等运行期生成物）
    if let Some(src) = bundled_python_dir() {
      eprintln!(
        "[MoeVault] 同步推理服务代码 {} → {}",
        src.display(),
        runtime.display()
      );
      if let Err(e) = copy_dir_recursive(&src, &runtime) {
        eprintln!("[MoeVault] 同步推理服务代码失败: {e}");
      }
    } else {
      eprintln!("[MoeVault] 警告: 安装目录未找到推理服务资源（python/server）");
    }
    runtime
  }
}

/// 安装包内置的 python 资源目录（exe 旁 python/ 或 resources/python/）。
fn bundled_python_dir() -> Option<PathBuf> {
  let exe_dir = std::env::current_exe()
    .ok()
    .and_then(|p| p.parent().map(|p| p.to_path_buf()))?;
  for p in [exe_dir.join("python"), exe_dir.join("resources").join("python")] {
    if p.join("server").is_dir() {
      return Some(p);
    }
  }
  None
}

/// 递归复制目录（src 存在性由调用方保证）。
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
  std::fs::create_dir_all(dst)?;
  for entry in std::fs::read_dir(src)? {
    let entry = entry?;
    let from = entry.path();
    let to = dst.join(entry.file_name());
    if entry.file_type()?.is_dir() {
      copy_dir_recursive(&from, &to)?;
    } else {
      // 跳过运行期生成物（日志/venv/缓存），只复制源码
      let name = entry.file_name().to_string_lossy().to_string();
      if name == "infer.log" || name == "__pycache__" || name.ends_with(".pyc") {
        continue;
      }
      std::fs::copy(&from, &to)?;
    }
  }
  Ok(())
}

/// 解析推理服务 python 候选启动器（按优先级）：
/// 1) python/.venv/Scripts/python.exe（Windows，setup.bat 创建）
/// 2) python/.venv/bin/python（类 unix）
/// 3) PATH 上的 py 启动器 / python
fn infer_python_candidates() -> Vec<(String, Vec<String>)> {
  let py_dir = infer_python_dir();
  let mut out = Vec::new();
  #[cfg(target_os = "windows")]
  {
    for p in [
      py_dir.join(".venv").join("Scripts").join("python.exe"),
      py_dir.join("venv").join("Scripts").join("python.exe"),
      py_dir.join(".venv").join("Scripts").join("pythonw.exe"),
    ] {
      if p.is_file() {
        out.push((p.to_string_lossy().into_owned(), Vec::new()));
      }
    }
    out.push(("py".into(), vec!["-3".into()]));
    out.push(("python".into(), Vec::new()));
  }
  #[cfg(not(target_os = "windows"))]
  {
    for p in [
      py_dir.join(".venv").join("bin").join("python3"),
      py_dir.join(".venv").join("bin").join("python"),
    ] {
      if p.is_file() {
        out.push((p.to_string_lossy().into_owned(), Vec::new()));
      }
    }
    out.push(("python3".into(), Vec::new()));
    out.push(("python".into(), Vec::new()));
  }
  out
}

/// 检查指定解释器中推理服务关键依赖缺失项（fastapi/uvicorn/transformers）。
/// 空 = 全部就绪；解释器不可执行时返回 ["python"]。
fn deps_missing_in(exe: &str, prefix: &[String]) -> Vec<String> {
  let mut cmd = Command::new(exe);
  cmd.args(prefix);
  cmd.args([
    "-c",
    "import importlib.util as u; missing=[m for m in ['fastapi','uvicorn','transformers','onnxruntime','PIL','numpy'] if u.find_spec(m) is None]; print(' '.join(missing))",
  ]);
  let out = match cmd.output() {
    Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
    _ => return vec!["python".into()],
  };
  if out.is_empty() {
    Vec::new()
  } else {
    out.split_whitespace().map(|s| s.to_string()).collect()
  }
}

/// 检查推理服务关键依赖是否齐全（fastapi/uvicorn/transformers）。
/// 返回首个候选解释器的缺失模块名列表（空 = 全部就绪）。找不到 python 时返回 ["python"]。
fn infer_deps_missing() -> Vec<String> {
  let candidates = infer_python_candidates();
  match candidates.first() {
    Some((exe, prefix)) => deps_missing_in(exe, prefix),
    None => vec!["python".into()],
  }
}

/// 确保 runtime python/.venv 存在且推理依赖齐全；缺什么装什么（幂等）。
/// - venv 不存在 → 用系统 Python（py -3 优先）创建 **纯净 venv**（不带 --system-site-packages）：
///   避免系统 site-packages 的旧 uvicorn/torch 干扰（GPU 部署后服务必须跑在 venv 环境）
/// - venv 存在但缺依赖 → 直接往 venv 里 pip 安装
/// pip 安装按 INFER_PIP_INDEXES 依次尝试（清华镜像优先）；过程写入 <runtime>/infer.log。
/// 返回 venv 的 python.exe 路径。
fn ensure_infer_venv() -> Result<PathBuf, String> {
  let py_dir = infer_python_dir();
  std::fs::create_dir_all(&py_dir)
    .map_err(|e| format!("创建推理环境目录失败: {e}"))?;
  let venv_exe = py_dir.join(".venv").join("Scripts").join("python.exe");
  let log_path = py_dir.join("infer.log");
  let venv_str = venv_exe.to_string_lossy().into_owned();

  let note = |msg: &str| {
    eprintln!("[MoeVault] {msg}");
    if let Ok(mut f) = std::fs::OpenOptions::new()
      .create(true)
      .append(true)
      .open(&log_path)
    {
      use std::io::Write;
      let _ = writeln!(f, "[install] {msg}");
    }
  };

  // 1) venv 已存在且依赖齐全 → 直接复用
  if Path::new(&venv_exe).is_file() && deps_missing_in(&venv_str, &[]).is_empty() {
    return Ok(venv_exe);
  }

  // 2) venv 不存在 → 用系统 Python 创建（py -3 优先；纯净 venv，不继承系统 site-packages）
  if !Path::new(&venv_exe).is_file() {
    note("未找到 python/.venv，正在创建纯净虚拟环境（依赖将完整装入 venv）…");
    // 基底解释器：候选列表中第一个不在 python 目录内的条目（跳过 venv 自身），兜底 py -3
    let base = infer_python_candidates()
      .into_iter()
      .find(|(exe, _)| !Path::new(exe).starts_with(&py_dir))
      .unwrap_or_else(|| ("py".into(), vec!["-3".into()]));
    let mut cmd = Command::new(&base.0);
    cmd.args(&base.1);
    // 纯净 venv：不继承系统 site-packages（避免旧 uvicorn/torch 干扰推理环境）
    cmd.args(["-m", "venv", &py_dir.join(".venv").to_string_lossy()]);
    #[cfg(target_os = "windows")]
    {
      use std::os::windows::process::CommandExt;
      const CREATE_NO_WINDOW: u32 = 0x0800_0000;
      cmd.creation_flags(CREATE_NO_WINDOW);
    }
    match cmd.output() {
      Ok(o) if o.status.success() => {}
      Ok(o) => {
        let stderr = String::from_utf8_lossy(&o.stderr);
        let tail: String = stderr.lines().rev().take(8).collect::<Vec<_>>().join("\n");
        return Err(format!("创建推理环境 .venv 失败:\n{tail}"));
      }
      Err(e) => return Err(format!("创建推理环境 .venv 失败: {e}")),
    }
    note("python/.venv 创建完成");
  }

  // 3) 往 venv 安装缺失依赖（清华镜像优先，官方 PyPI 兜底）
  let missing = deps_missing_in(&venv_str, &[]);
  if !missing.is_empty() {
    note(&format!(
      "安装缺失依赖: {}（清华镜像优先）…",
      missing.join(", ")
    ));
    let mut last_err: Option<String> = None;
    let mut installed = false;
    for index in INFER_PIP_INDEXES {
      let mut cmd = Command::new(&venv_exe);
      cmd.args([
        "-m",
        "pip",
        "install",
        "--disable-pip-version-check",
        "--no-warn-script-location",
        "-i",
        index,
      ]);
      cmd.args([
        "fastapi",
        "uvicorn",
        "transformers",
        "onnxruntime",
        "pillow",
        "numpy",
      ]);
      #[cfg(target_os = "windows")]
      {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
      }
      match cmd.output() {
        Ok(o) if o.status.success() => {
          note(&format!("依赖安装完成（源 {index}）"));
          installed = true;
          break;
        }
        Ok(o) => {
          let stderr = String::from_utf8_lossy(&o.stderr);
          let tail: String = stderr.lines().rev().take(8).collect::<Vec<_>>().join("\n");
          last_err = Some(tail);
          note(&format!("源 {index} 安装失败，尝试下一个…"));
        }
        Err(e) => {
          last_err = Some(format!("{e}"));
          note(&format!("源 {index} 无法访问: {e}"));
        }
      }
    }
    if !installed {
      return Err(format!(
        "推理依赖安装失败:\n{}",
        last_err.unwrap_or_else(|| "未知错误".into())
      ));
    }
  }

  Ok(venv_exe)
}

/// 启动推理服务（Python uvicorn，端口 8001）。
/// - 若 8001 已健康（外部已启动）→ 返回 Ok(None)，不重复拉起
/// - 关键依赖缺失（fastapi/uvicorn/transformers）→ 自动创建/修复 python/.venv 并安装依赖（幂等）
/// - 否则按候选列表尝试 spawn（venv python → py -3 → python）
/// 使用 CREATE_NO_WINDOW 隐藏控制台；日志写 <runtime>/infer.log
fn start_infer() -> std::io::Result<Option<Child>> {
  // 全流程互斥：避免 setup 后台线程与「启动服务」/「一键安装」并发安装依赖或重复 spawn
  let _guard = INFER_START_LOCK
    .get_or_init(|| Mutex::new(()))
    .lock()
    .unwrap();
  if infer_healthy() {
    // 已有一个实例在 8001：确认它是否用 venv python（GPU torch / 正确依赖）。
    // 若是系统 Python 的旧实例（如用户手动部署 GPU 前残留），杀掉后用 venv 重启，
    // 避免"GPU torch 装好但服务仍跑 CPU 旧实例"的失效问题。
    if infer_port_uses_system_python() {
      eprintln!("[MoeVault] 8001 被系统 Python 的推理实例占用（非 venv），杀掉并用 venv 重启");
      kill_infer_port_owner();
      // 等端口释放
      for _ in 0..50 {
        if !infer_port_occupied() {
          break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
      }
    } else {
      eprintln!("[MoeVault] 推理服务已在运行（venv 实例），跳过启动");
      return Ok(None);
    }
  }
  // 依赖预检：缺失 → 自动创建/修复 python/.venv（首次可能耗时数分钟；安装日志写 infer.log）
  let mut missing = infer_deps_missing();
  if !missing.is_empty() {
    match ensure_infer_venv() {
      Ok(venv) => {
        eprintln!("[MoeVault] 推理依赖已就绪（{}）", venv.display());
      }
      Err(e) => {
        return Err(std::io::Error::new(std::io::ErrorKind::Other, e));
      }
    }
    missing = infer_deps_missing();
    if !missing.is_empty() {
      return Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        format!(
          "推理服务依赖仍然缺失: {}（请检查网络后重试，或手动运行 python/setup.bat）",
          missing.join(", ")
        ),
      ));
    }
  }
  let cwd = infer_python_dir();
  let _ = std::fs::create_dir_all(&cwd);
  let log_path = cwd.join("infer.log");
  let log_file = std::fs::OpenOptions::new()
    .create(true)
    .append(true)
    .open(&log_path)?;
  let port = INFER_PORT.to_string();
  let mut last_err: Option<std::io::Error> = None;

  for (exe, prefix) in infer_python_candidates() {
    let mut cmd = Command::new(&exe);
    cmd
      .current_dir(&cwd)
      .stdout(Stdio::from(log_file.try_clone()?))
      .stderr(Stdio::from(log_file.try_clone()?));
    // HF_HOME：优先运行时目录内的 hf_cache（随软件走，不依赖 C 盘用户缓存）；
    // 若该目录不存在但系统缓存里有已下载模型，则沿用系统缓存（兼容旧安装迁移前的数据）。
    let runtime_hf = cwd.join("hf_cache");
    let system_hf = std::env::var("USERPROFILE")
      .ok()
      .map(|u| std::path::Path::new(&u).join(".cache").join("huggingface"));
    let hf_home = if runtime_hf.is_dir() {
      runtime_hf
    } else {
      system_hf.filter(|p| p.is_dir()).unwrap_or_else(|| cwd.join("hf_cache"))
    };
    cmd.env("HF_HOME", &hf_home);
    cmd.env("TRANSFORMERS_CACHE", hf_home.join("hub"));
    cmd.args(&prefix);
    cmd.args([
      "-m",
      "uvicorn",
      "server.main:app",
      "--host",
      "127.0.0.1",
      "--port",
      &port,
    ]);
    // Windows：隐藏推理服务控制台窗口
    #[cfg(target_os = "windows")]
    {
      use std::os::windows::process::CommandExt;
      const CREATE_NO_WINDOW: u32 = 0x0800_0000;
      cmd.creation_flags(CREATE_NO_WINDOW);
    }
    eprintln!(
      "[MoeVault] 启动推理服务: {} {} (cwd: {}, log: {})",
      exe,
      prefix.join(" "),
      cwd.display(),
      log_path.display()
    );
    match cmd.spawn() {
      Ok(child) => return Ok(Some(child)),
      Err(e) => {
        eprintln!("[MoeVault] 尝试推理服务启动器 {exe} 失败: {e}");
        last_err = Some(e);
      }
    }
  }
  Err(last_err.unwrap_or_else(|| {
    std::io::Error::new(
      std::io::ErrorKind::NotFound,
      "未找到可用的 Python 解释器（请先运行 python/setup.bat 或安装 Python）",
    )
  }))
}

/// 轮询推理服务 /health 直到就绪（最多 60 秒；模型首次加载可能较慢）。
fn wait_for_infer() {
  for _ in 0..300 {
    if infer_healthy() {
      eprintln!("[MoeVault] 推理服务就绪: {INFER_URL}");
      return;
    }
    thread::sleep(Duration::from_millis(200));
  }
  eprintln!("[MoeVault] 推理服务 60 秒内未就绪（可能缺依赖，见 python/infer.log）");
}

/// 桌面壳命令：手动启动推理服务（设置页「启动服务」按钮）。
#[tauri::command]
fn infer_start() -> Result<String, String> {
  // 已有子进程句柄（壳已拉起）或外部实例健康 → 已在运行
  if let Some(slot) = INFER_CHILD.get() {
    if slot.lock().unwrap().is_some() {
      return Ok("推理服务已在运行".into());
    }
  }
  if infer_healthy() {
    return Ok("推理服务已在运行（外部实例）".into());
  }
  match start_infer() {
    Ok(Some(child)) => {
      let slot = INFER_CHILD.get_or_init(|| Mutex::new(None));
      *slot.lock().unwrap() = Some(child);
      Ok("推理服务启动中…".into())
    }
    Ok(None) => Ok("推理服务已在运行（外部实例）".into()),
    Err(e) => Err(format!("推理服务启动失败: {e}")),
  }
}

/// 桌面壳命令：停止推理服务（设置页「停止服务」按钮）。
#[tauri::command]
fn infer_stop() -> Result<(), String> {
  kill_infer();
  Ok(())
}

/// 桌面壳命令：查询推理服务运行状态（供前端判断按钮可用性）。
/// 未运行时附带关键依赖缺失列表（供设置页显示"一键安装"入口）。
#[tauri::command]
fn infer_status() -> Result<serde_json::Value, String> {
  let owned = INFER_CHILD
    .get()
    .map(|s| s.lock().unwrap().is_some())
    .unwrap_or(false);
  let running = infer_healthy();
  let deps_missing: Vec<String> = if running {
    Vec::new()
  } else {
    infer_deps_missing()
  };
  Ok(serde_json::json!({
    "running": running,
    "owned": owned,
    "deps_missing": deps_missing,
  }))
}

/// 桌面壳命令：一键确保推理服务依赖就绪（fastapi/uvicorn/transformers）。
/// 复用 ensure_infer_venv：创建/修复 runtime python/.venv 并安装缺失依赖
/// （清华镜像优先，官方 PyPI 兜底；与 setup.bat 约定一致）。
#[tauri::command]
fn infer_install_deps() -> Result<String, String> {
  // 与 start_infer 共用同一把锁：避免与 setup 后台自动安装并发写 venv
  let _guard = INFER_START_LOCK
    .get_or_init(|| Mutex::new(()))
    .lock()
    .unwrap();
  let venv = ensure_infer_venv()?;
  Ok(format!("推理依赖已就绪（{}），可启动服务", venv.display()))
}

/// GPU 环境安装进行中标记（增强1：部署按钮防重复点击 + 状态轮询）。
static GPU_INSTALLING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 桌面壳命令：安装 GPU 推理环境（CUDA 版 torch/torchvision，替换 CPU 版）。
/// 立即返回，后台安装（数 GB 下载）；进度写 python/infer.log，用 infer_gpu_status 轮询。
#[tauri::command]
fn infer_install_gpu() -> Result<serde_json::Value, String> {
  use std::sync::atomic::Ordering;
  if GPU_INSTALLING.swap(true, Ordering::SeqCst) {
    return Ok(serde_json::json!({ "started": false, "message": "GPU 环境安装已在进行中" }));
  }
  // ensure_infer_venv 返回的是 venv python.exe 的完整路径（可直接用于 Command）
  let venv_exe = ensure_infer_venv()?;
  let py_dir = infer_python_dir();
  let log_path = py_dir.join("infer.log");

  std::thread::spawn(move || {
    let note = |msg: &str| {
      eprintln!("[MoeVault][gpu] {msg}");
      if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
        use std::io::Write;
        let _ = writeln!(f, "[gpu-install] {msg}");
      }
    };
    note("开始安装 GPU 推理环境（CUDA 12.8 版 torch 2.11.0 + torchvision 0.26.0，约 3GB，视网速需数分钟到数十分钟）…");
    // cu128 wheel：支持 RTX 50 系（Blackwell sm_120）及旧卡。
    // 关键：
    // 1) --index-url 只给 cu128 源、--extra-index-url 给 PyPI 镜像（torch 依赖 setuptools 等从 PyPI 拿）
    // 2) 锁定精确版本 +cu128（PEP 440 local version），避免 pip 选到 PyPI 上版本号更高的 CPU torch
    // 3) --only-binary :all: 防止源码编译
    // 4) retries 10：本机网络对 pytorch 官方源偶发 TLS 阻断，多次重试可穿透
    let combos = [
      ("https://download.pytorch.org/whl/cu128", "https://pypi.tuna.tsinghua.edu.cn/simple"),
      ("https://download.pytorch.org/whl/cu128", "https://pypi.org/simple"),
    ];
    let mut last_err: Option<String> = None;
    let mut installed = false;
    for (main, extra) in combos {
      // pip 输出实时写入 infer.log（进度可见；--progress-bar off 减少控制字符污染日志）
      let pip_log = log_path.clone();
      let mut cmd = Command::new(&venv_exe);
      cmd.args([
        "-m",
        "pip",
        "install",
        "--progress-bar",
        "off",
        "--disable-pip-version-check",
        "--no-warn-script-location",
        "--only-binary",
        ":all:",
        "--retries",
        "10",
        "--timeout",
        "60",
        "--force-reinstall",
        "--index-url",
        main,
        "--extra-index-url",
        extra,
        "torch==2.11.0+cu128",
        "torchvision==0.26.0+cu128",
      ]);
      #[cfg(target_os = "windows")]
      {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
      }
      let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&pip_log);
      if let Ok(f) = log_file {
        if let Ok(f2) = f.try_clone() {
          cmd.stdout(std::process::Stdio::from(f2));
        }
        cmd.stderr(std::process::Stdio::from(f));
      }
      // stdout/stderr 已重定向到文件 → 用 status() 等待退出码（进度实时进 infer.log）
      match cmd.status() {
        Ok(st) if st.success() => {
          note(&format!("GPU 推理环境安装完成（{main} + {extra}）；请重启推理服务使 CUDA 生效"));
          installed = true;
          break;
        }
        Ok(st) => {
          last_err = Some(format!("组合 {main} + {extra} 退出码 {:?}", st.code()));
          note(&format!("GPU 环境安装失败（退出码 {:?}），尝试下一组合…", st.code()));
        }
        Err(e) => {
          last_err = Some(format!("组合 {main} + {extra} 无法启动: {e}"));
          note(&format!("GPU 环境安装启动失败: {e}"));
        }
      }
    }
    if !installed {
      note(&format!(
        "GPU 环境安装失败（全部源组合均失败）:\n{}\n\n手动安装指引（网络被墙时的备选）：\n1) 浏览器打开 https://download.pytorch.org/whl/cu128/torch/ 下载 torch-2.11.0+cu128-cp310-cp310-win_amd64.whl\n2) 下载 torchvision-0.26.0+cu128-cp310-cp310-win_amd64.whl（同目录）\n3) 把两个文件放到本目录，运行：\n   .venv\\Scripts\\python.exe -m pip install torch-*.whl torchvision-*.whl --index-url https://pypi.tuna.tsinghua.edu.cn/simple\n4) 重启动应用，设置页查看 GPU 是否就绪",
        last_err.unwrap_or_else(|| "未知错误".into())
      ));
    }
    GPU_INSTALLING.store(false, Ordering::SeqCst);
  });

  Ok(serde_json::json!({ "started": true, "message": "GPU 环境安装已开始（后台进行，进度见 python/infer.log）" }))
}

/// 桌面壳命令：GPU 环境状态（安装中 + 当前 torch 是否可用 CUDA）。
#[tauri::command]
fn infer_gpu_status() -> Result<serde_json::Value, String> {
  use std::sync::atomic::Ordering;
  let installing = GPU_INSTALLING.load(Ordering::SeqCst);
  // 快速探测 venv python 的 CUDA 可用性（torch 已装才可能 true；未装 torch 会报错 → false）
  let py_dir = infer_python_dir();
  let venv_exe = py_dir.join(".venv").join("Scripts").join("python.exe");
  let mut cuda_ready = false;
  if Path::new(&venv_exe).is_file() {
    let mut cmd = Command::new(&venv_exe);
    cmd.args([
      "-c",
      "import torch;print(1 if torch.cuda.is_available() else 0)",
    ]);
    #[cfg(target_os = "windows")]
    {
      use std::os::windows::process::CommandExt;
      const CREATE_NO_WINDOW: u32 = 0x0800_0000;
      cmd.creation_flags(CREATE_NO_WINDOW);
    }
    if let Ok(o) = cmd.output() {
      cuda_ready = String::from_utf8_lossy(&o.stdout).trim() == "1";
    }
  }
  Ok(serde_json::json!({ "installing": installing, "cuda_ready": cuda_ready }))
}

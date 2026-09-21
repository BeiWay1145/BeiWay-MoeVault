//! Windows 原生拖放支持（替代 wry 在 WebView2 上的 IDropTarget 注册）。
//!
//! 仅在 Windows 下编译（其它平台无此问题）。
//!
//! 背景：wry 0.55.1 的 `DragDropController::new` 先调用 `SetAllowExternalDrop(false)`
//! 关闭 WebView2 内置拖放，再尝试 `RevokeDragDrop` + `RegisterDragDrop` 注册自己的 target。
//! 但注册被 `RevokeDragDrop(hwnd) != Err(DRAGDROP_E_INVALIDHWND)` 这个条件短路：
//! 关闭内置拖放后，WebView2 子窗口往往没有既有的 OLE drop target，
//! `RevokeDragDrop` 返回 INVALIDHWND → wry 的 target 也不会注册。
//! 结果是系统层面没有任何可接受拖放的目标窗口，拖入时显示 STOP 拒绝光标，
//! 且 DragDropEvent 永远不会触发。
//!
//! 本模块在窗口 HWND 及其子窗口上直接注册 `IDropTarget`，
//! 收到文件拖放后通过 Tauri 事件 `moevault://drag-drop` 转发给前端。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use tauri::{Emitter, Manager};
use windows::core::{implement, Result as WinResult, BOOL};
use windows::Win32::Foundation::{HWND, LPARAM, POINTL};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
use windows::Win32::System::Ole::{
  IDropTarget, IDropTarget_Impl, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
  CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::EnumChildWindows;

/// 前端监听的拖放事件名。
pub const DRAG_DROP_EVENT: &str = "moevault://drag-drop";

/// 转发给前端的载荷（与 Tauri 官方 DragDropEvent 形态保持一致）。
#[derive(Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum DragDropPayload {
  Enter { paths: Vec<String>, position: (i32, i32) },
  Over { position: (i32, i32) },
  Drop { paths: Vec<String>, position: (i32, i32) },
  Leave,
}

/// 持有已注册的 drop target，避免被提前释放（释放会注销拖放）。
pub struct DragDropGuard {
  _targets: Vec<IDropTarget>,
  hwnds: Vec<HWND>,
}

impl Drop for DragDropGuard {
  fn drop(&mut self) {
    for hwnd in &self.hwnds {
      unsafe {
        let _ = RevokeDragDrop(*hwnd);
      }
    }
  }
}

thread_local! {
  /// 当前线程持有的守卫。OLE COM 对象不是 Send，必须留在创建它的线程上；
  /// 一旦被释放就会调用 RevokeDragDrop 注销拖放，因此需长期持有。
  static GUARD: RefCell<Option<DragDropGuard>> = const { RefCell::new(None) };
}

/// 在窗口及其子窗口上注册拖放接收器，并把守卫存到线程局部（防提前释放）。
///
/// 与 wry 不同，这里**无条件**尝试注册：即使 `RevokeDragDrop` 返回 INVALIDHWND
/// （表示原本没有 target），也继续注册我们自己的 target。
pub fn install(window: &tauri::WebviewWindow) {
  let guard = match register(window) {
    Some(g) => g,
    None => return,
  };
  GUARD.with(|cell| {
    *cell.borrow_mut() = Some(guard);
  });
}

fn register(window: &tauri::WebviewWindow) -> Option<DragDropGuard> {
  let hwnd = window.hwnd().ok()?;
  let hwnd = HWND(hwnd.0 as _);

  // 事件发送器：把载荷广播给所有 webview
  let app = window.app_handle().clone();
  let emit = move |payload: DragDropPayload| {
    let _ = app.emit(DRAG_DROP_EVENT, payload);
  };
  let emit = Rc::new(emit);

  let mut targets: Vec<IDropTarget> = Vec::new();
  let mut hwnds: Vec<HWND> = Vec::new();

  // 顶层窗口本身 + 所有子窗口（WebView2 的内容窗口是子窗口，真正接收拖放）
  let mut candidates: Vec<HWND> = vec![hwnd];
  unsafe {
    let mut collected: Vec<HWND> = Vec::new();
    let ptr = &mut collected as *mut Vec<HWND>;
    unsafe extern "system" fn cb(child: HWND, lparam: LPARAM) -> BOOL {
      let list = &mut *(lparam.0 as *mut Vec<HWND>);
      list.push(child);
      BOOL(1) // 继续枚举
    }
    let _ = EnumChildWindows(Some(hwnd), Some(cb), LPARAM(ptr as isize));
    candidates.extend(collected);
  }

  for h in candidates {
    let target: IDropTarget = DropTarget::new(h, emit.clone()).into();
    unsafe {
      // 先撤销可能存在的旧 target（忽略失败：INVALIDHWND 表示本就没有）
      let _ = RevokeDragDrop(h);
      if RegisterDragDrop(h, &target).is_ok() {
        targets.push(target);
        hwnds.push(h);
      }
    }
  }

  if targets.is_empty() {
    eprintln!("[MoeVault] 拖放注册失败：没有可用的窗口句柄");
    return None;
  }
  eprintln!("[MoeVault] 原生拖放已注册（{} 个窗口）", targets.len());
  Some(DragDropGuard {
    _targets: targets,
    hwnds,
  })
}

#[implement(IDropTarget)]
struct DropTarget {
  hwnd: HWND,
  emit: Rc<dyn Fn(DragDropPayload)>,
  /// 当前悬停是否合法（用于只在有效时发 Leave）
  entered: RefCell<bool>,
}

impl DropTarget {
  fn new(hwnd: HWND, emit: Rc<dyn Fn(DragDropPayload)>) -> Self {
    Self {
      hwnd,
      emit,
      entered: RefCell::new(false),
    }
  }

  /// 把屏幕坐标转成客户区坐标。
  fn client_pos(&self, pt: &POINTL) -> (i32, i32) {
    let mut p = windows::Win32::Foundation::POINT { x: pt.x, y: pt.y };
    unsafe {
      let _ = ScreenToClient(self.hwnd, &mut p);
    }
    (p.x, p.y)
  }

  /// 从 IDataObject 中提取拖入的文件路径（CF_HDROP）。
  fn extract_paths(data: &IDataObject) -> Vec<String> {
    let mut out = Vec::new();
    let fmt = FORMATETC {
      cfFormat: CF_HDROP.0,
      ptd: std::ptr::null_mut(),
      dwAspect: DVASPECT_CONTENT.0,
      lindex: -1,
      tymed: TYMED_HGLOBAL.0 as u32,
    };
    unsafe {
      let Ok(mut medium) = data.GetData(&fmt) else {
        return out;
      };
      // medium 由我们负责释放
      let hdrop = HDROP(medium.u.hGlobal.0 as _);
      let count = DragQueryFileW(hdrop, 0xFFFF_FFFF, None);
      for i in 0..count {
        let len = DragQueryFileW(hdrop, i, None);
        if len == 0 {
          continue;
        }
        let mut buf = vec![0u16; (len + 1) as usize];
        let written = DragQueryFileW(hdrop, i, Some(&mut buf));
        if written > 0 {
          let s = String::from_utf16_lossy(&buf[..written as usize]);
          out.push(PathBuf::from(s).to_string_lossy().into_owned());
        }
      }
      DragFinish(hdrop);
      ReleaseStgMedium(&mut medium);
    }
    out
  }
}

impl IDropTarget_Impl for DropTarget_Impl {
  fn DragEnter(
    &self,
    data: windows::core::Ref<IDataObject>,
    _keys: MODIFIERKEYS_FLAGS,
    pt: &POINTL,
    effect: *mut DROPEFFECT,
  ) -> WinResult<()> {
    let paths = match data.as_ref() {
      Some(d) => DropTarget::extract_paths(d),
      None => Vec::new(),
    };
    let pos = (*self).client_pos(pt);
    if !paths.is_empty() {
      *self.entered.borrow_mut() = true;
      (self.emit)(DragDropPayload::Enter { paths, position: pos });
    }
    unsafe {
      *effect = DROPEFFECT_COPY;
    }
    Ok(())
  }

  fn DragOver(
    &self,
    _keys: MODIFIERKEYS_FLAGS,
    pt: &POINTL,
    effect: *mut DROPEFFECT,
  ) -> WinResult<()> {
    let pos = (*self).client_pos(pt);
    if *self.entered.borrow() {
      (self.emit)(DragDropPayload::Over { position: pos });
    }
    unsafe {
      *effect = DROPEFFECT_COPY;
    }
    Ok(())
  }

  fn DragLeave(&self) -> WinResult<()> {
    if *self.entered.borrow() {
      *self.entered.borrow_mut() = false;
      (self.emit)(DragDropPayload::Leave);
    }
    Ok(())
  }

  fn Drop(
    &self,
    data: windows::core::Ref<IDataObject>,
    _keys: MODIFIERKEYS_FLAGS,
    pt: &POINTL,
    effect: *mut DROPEFFECT,
  ) -> WinResult<()> {
    let paths = match data.as_ref() {
      Some(d) => DropTarget::extract_paths(d),
      None => Vec::new(),
    };
    let pos = (*self).client_pos(pt);
    *self.entered.borrow_mut() = false;
    if !paths.is_empty() {
      (self.emit)(DragDropPayload::Drop { paths, position: pos });
      unsafe {
        *effect = DROPEFFECT_COPY;
      }
    } else {
      unsafe {
        *effect = DROPEFFECT_NONE;
      }
    }
    Ok(())
  }
}


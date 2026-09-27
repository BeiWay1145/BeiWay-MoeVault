//! Windows 文件操作公共层（拖入 / 拖出 / 剪贴板共用）。
//!
//! 三者本质相同：都围绕 Shell 的 **CF_HDROP**（文件路径列表）格式交互。
//! 把 CF_HDROP 的构造与解析集中在此处，避免各处重复实现。
//!
//! - 拖入（drag_drop.rs）：解析 CF_HDROP → 得到路径 → 转发前端
//! - 拖出（本模块 do_drag_out）：构造 CF_HDROP → DoDragDrop → 资源管理器接收
//! - 剪贴板（本模块 copy_files）：构造 CF_HDROP → SetClipboardData
#![cfg(windows)]

use std::path::Path;

use windows::core::{implement, Result as WinResult, BOOL};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
  DVASPECT_CONTENT, FORMATETC, IDataObject, IDataObject_Impl, IEnumFORMATETC, STGMEDIUM, TYMED_HGLOBAL,
};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT};
use windows::Win32::System::Ole::{
  CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DoDragDrop, IDropSource, IDropSource_Impl, OleSetClipboard,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{DROPFILES, HDROP};

/// 把路径列表打包成 CF_HDROP 所需的全局内存块（DROPFILES 头 + 双 NUL 结尾的宽字符串）。
///
/// 返回的内存块所有权在调用成功后转移给系统（剪贴板或拖放数据对象），不应再释放。
/// 布局：
/// ```text
/// [DROPFILES 头][path1\0][path2\0]...[\0]
/// ```
unsafe fn build_hdrop(paths: &[String]) -> Option<HDROP> {
  if paths.is_empty() {
    return None;
  }
  // 宽字符内容：每个路径以 NUL 结尾，整体再以一个额外 NUL 收尾
  let mut wide: Vec<u16> = Vec::new();
  for p in paths {
    wide.extend(p.encode_utf16());
    wide.push(0);
  }
  wide.push(0); // 双 NUL 终止

  let header = std::mem::size_of::<DROPFILES>();
  let bytes = header + wide.len() * std::mem::size_of::<u16>();
  let hmem = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes).ok()?;
  let ptr = GlobalLock(hmem) as *mut u8;
  if ptr.is_null() {
    return None;
  }
  // DROPFILES 头：pFiles 指向偏移 header 处的文件列表，fWide=TRUE 表示宽字符
  let df = ptr as *mut DROPFILES;
  (*df).pFiles = header as u32;
  (*df).fWide = BOOL(1);
  // 拷贝宽字符内容到头部之后
  let dst = ptr.add(header) as *mut u16;
  std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
  let _ = GlobalUnlock(hmem);
  Some(HDROP(hmem.0 as _))
}

/// 从 CF_HDROP 内存块解析出文件路径列表。
///
/// # Safety
/// `hdrop` 必须指向有效的 DROPFILES 结构（由系统或 build_hdrop 提供）。
pub unsafe fn parse_hdrop(hdrop: HDROP) -> Vec<String> {
  use windows::Win32::UI::Shell::{DragFinish, DragQueryFileW};
  let mut out = Vec::new();
  let count = DragQueryFileW(hdrop, 0xFFFF_FFFF, None);
  for i in 0..count {
    let len = DragQueryFileW(hdrop, i, None);
    if len == 0 {
      continue;
    }
    let mut buf = vec![0u16; (len + 1) as usize];
    let written = DragQueryFileW(hdrop, i, Some(&mut buf));
    if written > 0 {
      out.push(String::from_utf16_lossy(&buf[..written as usize]));
    }
  }
  DragFinish(hdrop);
  out
}

/// 一个最小可用的 IDataObject 实现，仅支持 CF_HDROP（拖出/剪贴板用）。
#[implement(IDataObject)]
pub struct FileDataObject {
  paths: Vec<String>,
}

impl FileDataObject {
  pub fn new(paths: Vec<String>) -> Self {
    Self { paths }
  }

  /// 生成 CF_HDROP 的 FORMATETC 描述。
  fn hdrop_format() -> FORMATETC {
    FORMATETC {
      cfFormat: CF_HDROP.0,
      ptd: std::ptr::null_mut(),
      dwAspect: DVASPECT_CONTENT.0,
      lindex: -1,
      tymed: TYMED_HGLOBAL.0 as u32,
    }
  }
}

impl IDataObject_Impl for FileDataObject_Impl {
  fn GetData(&self, pformatetc: *const FORMATETC) -> WinResult<STGMEDIUM> {
    unsafe {
      let fmt = *pformatetc;
      if fmt.cfFormat == CF_HDROP.0 {
        let hdrop = build_hdrop(&self.paths).ok_or_else(windows::core::Error::from_win32)?;
        return Ok(STGMEDIUM {
          tymed: TYMED_HGLOBAL.0 as u32,
          u: windows::Win32::System::Com::STGMEDIUM_0 {
            hGlobal: windows::Win32::Foundation::HGLOBAL(hdrop.0 as _),
          },
          pUnkForRelease: std::mem::ManuallyDrop::new(None),
        });
      }
      Err(windows::core::Error::from_win32())
    }
  }

  fn QueryGetData(&self, pformatetc: *const FORMATETC) -> windows::core::HRESULT {
    unsafe {
      if (*pformatetc).cfFormat == CF_HDROP.0 {
        windows::Win32::Foundation::S_OK
      } else {
        windows::Win32::Foundation::DV_E_FORMATETC
      }
    }
  }

  fn EnumFormatEtc(&self, _dwdir: u32) -> WinResult<IEnumFORMATETC> {
    // 未实现枚举：拖放/剪贴板只需 GetData + QueryGetData
    Err(windows::core::Error::from_win32())
  }

  fn GetDataHere(&self, _pformatetc: *const FORMATETC, _pmedium: *mut STGMEDIUM) -> WinResult<()> {
    Err(windows::core::Error::from_win32())
  }

  fn GetCanonicalFormatEtc(
    &self,
    _pformatectin: *const FORMATETC,
    _pformatetcout: *mut FORMATETC,
  ) -> windows::core::HRESULT {
    // E_NOTIMPL：调用方应使用原始 FORMATETC
    windows::Win32::Foundation::E_NOTIMPL
  }

  fn SetData(
    &self,
    _pformatetc: *const FORMATETC,
    _pmedium: *const STGMEDIUM,
    _frelease: BOOL,
  ) -> WinResult<()> {
    Err(windows::core::Error::from_win32())
  }

  fn DAdvise(
    &self,
    _pformatetc: *const FORMATETC,
    _advf: u32,
    _padvsink: windows::core::Ref<'_, windows::Win32::System::Com::IAdviseSink>,
  ) -> WinResult<u32> {
    // OLE_E_ADVISENOTSUPPORTED
    Err(windows::core::Error::from(windows::Win32::Foundation::OLE_E_ADVISENOTSUPPORTED))
  }

  fn DUnadvise(&self, _dwconnection: u32) -> WinResult<()> {
    Err(windows::core::Error::from(windows::Win32::Foundation::OLE_E_ADVISENOTSUPPORTED))
  }

  fn EnumDAdvise(&self) -> WinResult<windows::Win32::System::Com::IEnumSTATDATA> {
    Err(windows::core::Error::from(windows::Win32::Foundation::OLE_E_ADVISENOTSUPPORTED))
  }
}

/// 默认拖放源：按需求"默认为复制操作（无确认步骤）"。
#[implement(IDropSource)]
pub struct CopyDropSource;

impl IDropSource_Impl for CopyDropSource_Impl {
  fn QueryContinueDrag(&self, _escape: BOOL, _key_state: MODIFIERKEYS_FLAGS) -> windows::core::HRESULT {
    windows::Win32::Foundation::S_OK
  }
  fn GiveFeedback(&self, _effect: DROPEFFECT) -> windows::core::HRESULT {
    windows::Win32::Foundation::S_OK
  }
}

/// 执行原生文件拖出（复制语义）。
///
/// 注意：`DoDragDrop` 会**阻塞当前线程**直到用户松开鼠标，
/// 因此必须在非 UI 线程调用（调用方负责 spawn）。
pub fn do_drag_out(hwnd: HWND, paths: Vec<String>) -> Result<(), String> {
  if paths.is_empty() {
    return Err("没有可拖出的文件".into());
  }
  // 校验文件存在，避免拖出无效路径
  let valid: Vec<String> = paths
    .into_iter()
    .filter(|p| Path::new(p).is_file())
    .collect();
  if valid.is_empty() {
    return Err("文件不存在".into());
  }
  unsafe {
    let data: IDataObject = FileDataObject::new(valid).into();
    let source: IDropSource = CopyDropSource.into();
    let mut effect = DROPEFFECT_COPY;
    let hr = DoDragDrop(&data, &source, DROPEFFECT_COPY, &mut effect);
    // DRAGDROP_S_DROP / DRAGDROP_S_CANCEL 都算正常结束
    if hr.is_err() && hr != windows::Win32::Foundation::DRAGDROP_S_CANCEL {
      return Err(format!("DoDragDrop 失败: {hr:?}"));
    }
    let _ = hwnd;
  }
  Ok(())
}

/// 复制文件到剪贴板（CF_HDROP）：可在资源管理器中直接"粘贴"。
pub fn copy_files_to_clipboard(paths: Vec<String>) -> Result<usize, String> {
  let valid: Vec<String> = paths
    .into_iter()
    .filter(|p| Path::new(p).is_file())
    .collect();
  if valid.is_empty() {
    return Err("没有可复制的文件".into());
  }
  let n = valid.len();
  unsafe {
    // 用 OleSetClipboard 包装成数据对象，兼容性优于裸 SetClipboardData
    let data: IDataObject = FileDataObject::new(valid).into();
    OleSetClipboard(&data).map_err(|e| format!("写入剪贴板失败: {e}"))?;
  }
  Ok(n)
}

/// 复制纯文本到剪贴板（复制路径用）。
pub fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
  unsafe {
    OpenClipboard(None).map_err(|e| format!("打开剪贴板失败: {e}"))?;
    let _ = EmptyClipboard();
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * std::mem::size_of::<u16>();
    let hmem = GlobalAlloc(GMEM_MOVEABLE, bytes).map_err(|e| format!("分配内存失败: {e}"))?;
    let ptr = GlobalLock(hmem) as *mut u16;
    if ptr.is_null() {
      let _ = CloseClipboard();
      return Err("锁定内存失败".into());
    }
    std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
    let _ = GlobalUnlock(hmem);
    // CF_UNICODETEXT = 13
    let _ = SetClipboardData(13, Some(windows::Win32::Foundation::HANDLE(hmem.0 as _)));
    let _ = CloseClipboard();
  }
  Ok(())
}

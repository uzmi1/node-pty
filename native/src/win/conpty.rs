
use lazy_static::lazy_static;
use std::{sync::{Mutex, Arc}, collections::HashMap};

use napi::JsFunction;
use widestring::{WideCString};
use winapi::{um::{winnt::HRESULT, handleapi::INVALID_HANDLE_VALUE, winbase::{PIPE_ACCESS_INBOUND, PIPE_ACCESS_OUTBOUND, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_TYPE_BYTE, PIPE_READMODE_BYTE, PIPE_WAIT}, minwinbase::{SECURITY_ATTRIBUTES, LPSECURITY_ATTRIBUTES}}, shared::winerror::HRESULT_FROM_WIN32};
use crate::err;

#[cfg(target_family = "windows")] use {
  std::ptr::null,
  std::ffi::CString,
  widestring::U16CString,
  winapi::{
    ctypes::wchar_t,
    shared::{
      winerror::SUCCEEDED,
      minwindef::DWORD
    },
    um::{
      processthreadsapi::GetExitCodeProcess,
      winbase::{INFINITE, RegisterWaitForSingleObject},
      winnt::{HANDLE, PVOID, WT_EXECUTEONLYONCE},
      errhandlingapi::GetLastError
    }
  },
  napi::threadsafe_function::{
    ThreadSafeCallContext, ThreadsafeFunctionCallMode, ErrorStrategy,
    ThreadsafeFunction, ErrorStrategy::Fatal
  },
  windows::Win32::System::{
    Console::{
      CreatePseudoConsole,
      ResizePseudoConsole,
      ClosePseudoConsole,
      
    },
    Pipes::CreateNamedPipeW
  }
};

lazy_static! {
  /// Static list of all pty handles protected by a mutex and atomic ref count 
  pub static ref PTY_HANDLES: Arc<Mutex<HashMap<i32, ConptyBaton>>> =  Arc::new(Mutex::new(HashMap::new()));
}

/// Returns a new server named pipe.  It has not yet been connected.
unsafe fn createDataServerPipe(
    write: bool,
    kind: String,
    hServer: HANDLE,
    name: String,
    pipeName: String
    ) -> bool {
  hServer = INVALID_HANDLE_VALUE;

  
  name = WideCString::from_str(format!("\\\\.\\pipe\\{}-{}", pipeName, kind)).unwrap();

  let winOpenMode: DWORD =  PIPE_ACCESS_INBOUND | PIPE_ACCESS_OUTBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE;

  let sa: SECURITY_ATTRIBUTES = {};
  sa.nLength = sizeof(sa);

  hServer = CreateNamedPipeW(
    name.c_str(), winOpenMode,
    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
    1, 0, 0, 30000,
    &sa
  );

  return hServer != INVALID_HANDLE_VALUE;
}

fn CreateNamedPipesAndPseudoConsole(
  cols: u32, rows: u32,
  flags: u32,
  pipe_name: u16,
  out_pty_id: &mut i32,
  out_h_in: &mut *const cty::c_void,
  out_in_name: &mut *const wchar_t,
  out_h_out: &mut *const cty::c_void,
  out_out_name: &mut *const wchar_t
) -> HRESULT {
  // If the kernel doesn't export these functions then their system is
  // too old and we cannot run.

  let success: bool = createDataServerPipe(true, "in", phInput, inName, pipeName);
  if !success {
    return unsafe { HRESULT_FROM_WIN32(GetLastError()) }
  }
  success = createDataServerPipe(false, "out", phOutput, outName, pipeName);
  if !success {
    return unsafe { HRESULT_FROM_WIN32(GetLastError()) }
  }
  unsafe { CreatePseudoConsole(size, *phInput, *phOutput, dwFlags, phPC) }
}

fn PtyConnect(
  id: cty::c_int, cmdline: *const cty::c_char,
  cwd: *const cty::c_char, env: *const cty::c_char,
  out_h_process: &mut HANDLE
) -> i32 {
  0
}


#[napi(object)]
#[derive(Serialize, Deserialize, Debug)]
struct IConptyProcess {
  pub fd: i32,
  pub pty: i32,
  pub conin: String,
  pub conout: String
}

#[napi(object)]
struct IConptyConnection {
  pub pid: i32
}

struct ConptyBaton {
  pub h_process: HANDLE,
  pub async_cb: ThreadsafeFunction<u32, Fatal>
}

unsafe impl Send for ConptyBaton {}

#[allow(dead_code)]
#[napi]
fn conpty_start_process(
  file: String,
  cols: u32, rows: u32,
  debug: bool, pipe_name: String,
  conpty_inherit_cursor: bool) -> napi::Result<IConptyProcess> {
    #[cfg(not(target_family = "windows"))]
    return err!("Platform not supported");
    let p = U16CString::from_str(pipe_name).unwrap();

    let mut pty_id: cty::c_int = 0;
    let mut h_in: *const cty::c_void = null();
    let mut in_name: *const wchar_t = null();
    let mut h_out: *const cty::c_void = null();
    let mut out_name: *const wchar_t = null();

    let hr = CreateNamedPipesAndPseudoConsole(
      cols, rows,
      if conpty_inherit_cursor { 1 } else { 0 },
      p.as_ptr(),
      &mut pty_id,
      &mut h_in, &mut in_name,
      &mut h_out, &mut out_name
    );

    if ! SUCCEEDED(hr) { return err!("conpty failed"); }

    // @todo SetConsoleCtrl

    Ok(IConptyProcess {
      fd: -1, pty: pty_id,
      conin: unsafe { from_wchar_ptr(in_name) },
      conout: unsafe { from_wchar_ptr(out_name) }
    })
}

#[allow(dead_code)]
#[napi]
fn conpty_connect(pty_id: i32, cmdline: String, cwd: String, env: Vec<String>, onexit: JsFunction) -> napi::Result<IConptyConnection> {
  #[cfg(not(target_family = "windows"))]
  return err!("Platform not supported");
  let senv = (env.join("\0") + "\0\0").into_bytes();

  let mut h_process: HANDLE = unsafe { std::mem::zeroed() };
  let pid = unsafe { PtyConnect(pty_id, CString::new(cmdline)?.as_ptr(),
      CString::new(cwd)?.as_ptr(), senv.as_ptr() as *const cty::c_char, &mut h_process) };

  let async_cb = onexit.create_threadsafe_function::<_, _, _, ErrorStrategy::Fatal>(0,
    |ctx: ThreadSafeCallContext<u32>| {
      ctx.env.create_uint32(ctx.value).map(|v0| { vec![v0] })
    })?;

  let baton = Box::new( ConptyBaton { h_process, async_cb } );

  unsafe {
    let mut h_wait: HANDLE = std::mem::zeroed();
    RegisterWaitForSingleObject(&mut h_wait, h_process,
                                Some(on_exit_process_eh),
                                Box::into_raw(baton) as *mut _,
                                INFINITE, WT_EXECUTEONLYONCE);
  }

  /* @todo check `pid > 0` and report errors */
  Ok(IConptyConnection { pid })
}

#[allow(dead_code)]
#[napi]
fn conpty_resize(pty_id: i32, cols: i32, rows: i32) -> napi::Result<()>{
  #[cfg(not(target_family = "windows"))]
  return err!("Platform not supported");
  return Ok(());
}

#[allow(dead_code)]
#[napi]
fn conpty_kill(pty_id: i32) -> napi::Result<()> {
  #[cfg(not(target_family = "windows"))]
  return err!("Platform not supported");
  return Ok(());
}

// #[napi(js_name = "startProcess")]
// #[allow(dead_code)]
// fn start_process(
//   _file: String,
//   cols: u32, rows: u32,
//   _debug: bool, pipe_name: String,
//   conpty_inherit_cursor: bool) -> napi::Result<IConptyProcess> {

//   let p = U16CString::from_str(pipe_name).unwrap();

//   let mut pty_id: cty::c_int = 0;
//   let mut h_in: *const cty::c_void = null();
//   let mut in_name: *const wchar_t = null();
//   let mut h_out: *const cty::c_void = null();
//   let mut out_name: *const wchar_t = null();

//   let hr = unsafe {
//     CreateNamedPipesAndPseudoConsole(cols, rows,
//                                      if conpty_inherit_cursor { 1 } else { 0 },
//                                      p.as_ptr(),
//                                      &mut pty_id,
//                                      &mut h_in, &mut in_name,
//                                      &mut h_out, &mut out_name)
//   };

//   if ! SUCCEEDED(hr) {
//     panic!("conpty failed");
//   }

//   // @todo SetConsoleCtrl

//   let result = IConptyProcess {
//     fd: -1, pty: pty_id,
//     conin: unsafe { from_wchar_ptr(in_name) },
//     conout: unsafe { from_wchar_ptr(out_name) }
//   };
//   return Ok(result);
// }

// #[napi(js_name = "connect")]
// #[allow(dead_code)]
// fn connect(pty_id: i32, cmdline: String, cwd: String, env: Vec<String>, onexit: JsFunction) -> napi::Result<IConptyConnection> {
//   let senv = (env.join("\0") + "\0\0").into_bytes();

//   let mut h_process: HANDLE = unsafe { std::mem::zeroed() };
//   let pid = unsafe { PtyConnect(pty_id, CString::new(cmdline)?.as_ptr(),
//       CString::new(cwd)?.as_ptr(), senv.as_ptr() as *const cty::c_char, &mut h_process) };

//   let async_cb = onexit.create_threadsafe_function::<_, _, _, ErrorStrategy::Fatal>(0,
//     |ctx: ThreadSafeCallContext<u32>| {
//       ctx.env.create_uint32(ctx.value).map(|v0| { vec![v0] })
//     })?;

//   let baton = Box::new( ConptyBaton { h_process, async_cb } );

//   unsafe {
//     let mut h_wait: HANDLE = std::mem::zeroed();
//     RegisterWaitForSingleObject(&mut h_wait, h_process,
//                                 Some(on_exit_process_eh),
//                                 Box::into_raw(baton) as *mut _,
//                                 INFINITE, WT_EXECUTEONLYONCE);
//   }

//   /* @todo check `pid > 0` and report errors */
//   Ok(IConptyConnection { pid })
// }

// unsafe extern "system" fn on_exit_process_eh(ctx: PVOID, _b: u8) -> () {
//   let baton = Box::from_raw(ctx as *mut ConptyBaton);
//   println!("hProcess = {:?}", baton.h_process);

//   let mut exit_code: u32 = 0;
//   GetExitCodeProcess(baton.h_process, &mut exit_code);
//   println!("exit code = {}", exit_code);

//   baton.async_cb.call(exit_code, ThreadsafeFunctionCallMode::Blocking);
//   /* @todo free the baton */
// }

unsafe fn from_wchar_ptr(s: *const wchar_t) -> String {
  U16CString::from_ptr_str(s).to_string_lossy().into()
}

/// This is marked as "extern" so that the callback isn't mangled
/// by the compiler. This allows for the other process to call a
/// predictable function when it exits. The exit code can then
/// be sent to the async callback in javascript
unsafe extern "system" fn on_exit_process_eh(ctx: PVOID, _b: u8) -> () {
  let baton = Box::from_raw(ctx as *mut ConptyBaton);
  println!("hProcess = {:?}", baton.h_process);

  let mut exit_code: u32 = 0;
  GetExitCodeProcess(baton.h_process, &mut exit_code);
  println!("exit code = {}", exit_code);

  baton.async_cb.call(exit_code, ThreadsafeFunctionCallMode::Blocking);
  /* @todo free the baton */
}


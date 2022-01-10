use std::ptr::null;
use std::ffi::CString;
use widestring::U16CString;
use winapi::ctypes::wchar_t;
use winapi::shared::winerror::SUCCEEDED;
use napi::JsFunction;

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

#[link(name = "conpty")]
extern "C" {
  fn CreateNamedPipesAndPseudoConsole(cols: cty::uint32_t, rows: cty::uint32_t,
                                      flags: cty::uint32_t,
                                      pipe_name: *const u16,
                                      out_pty_id: &mut cty::c_int,
                                      out_h_in: &mut *const cty::c_void,
                                      out_in_name: &mut *const wchar_t,
                                      out_h_out: &mut *const cty::c_void,
                                      out_out_name: &mut *const wchar_t) -> i32;

  fn PtyConnect(id: cty::c_int, cmdline: *const cty::c_char, cwd: *const cty::c_char, env: *const cty::c_char) -> i32;
}

#[napi(js_name = "startProcess")]
#[allow(dead_code)]
fn start_process(
  _file: String,
  cols: u32, rows: u32,
  _debug: bool, pipe_name: String,
  conpty_inherit_cursor: bool) -> napi::Result<IConptyProcess> {

  let p = U16CString::from_str(pipe_name).unwrap();

  let mut pty_id: cty::c_int = 0;
  let mut h_in: *const cty::c_void = null();
  let mut in_name: *const wchar_t = null();
  let mut h_out: *const cty::c_void = null();
  let mut out_name: *const wchar_t = null();

  let hr = unsafe {
    CreateNamedPipesAndPseudoConsole(cols, rows,
                                     if conpty_inherit_cursor { 1 } else { 0 },
                                     p.as_ptr(),
                                     &mut pty_id,
                                     &mut h_in, &mut in_name,
                                     &mut h_out, &mut out_name)
  };

  if ! SUCCEEDED(hr) {
    panic!("conpty failed");
  }

  let result = IConptyProcess {
    fd: -1, pty: pty_id,
    conin: unsafe { from_wchar_ptr(in_name) },
    conout: unsafe { from_wchar_ptr(out_name) }
  };
  return Ok(result);
}

#[napi]
#[allow(dead_code)]
fn connect(pty_id: i32, cmdline: String, cwd: String, env: Vec<String>, onexit: JsFunction) -> napi::Result<IConptyConnection> {
  let senv = (env.join("\0") + "\0\0").into_bytes();

  let pid = unsafe { PtyConnect(pty_id, CString::new(cmdline)?.as_ptr(),
      CString::new(cwd)?.as_ptr(), senv.as_ptr() as *const cty::c_char) };

  /** @todo check `pid > 0` and report errors */
  Ok(IConptyConnection { pid })
}

unsafe fn from_wchar_ptr(s: *const wchar_t) -> String {
  U16CString::from_ptr_str(s).to_string_lossy().into()
}

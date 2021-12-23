use std::ptr::null;
use widestring::U16CString;
use winapi::ctypes::wchar_t;
use winapi::shared::winerror::SUCCEEDED;

#[napi(object)]
#[derive(Serialize, Deserialize, Debug)]
struct IConptyProcess {
  pub fd: i32,
  pub pty: i32,
  pub conin: String,
  pub conout: String
}

#[link(name = "conpty")]
extern "C" {
  fn CreateNamedPipesAndPseudoConsole(cols: cty::uint32_t, rows: cty::uint32_t,
                                      flags: cty::uint32_t,
                                      pipe_name: *const u16,
                                      out_h_in: &mut *const cty::c_void,
                                      out_in_name: &mut *const wchar_t,
                                      out_h_out: &mut *const cty::c_void,
                                      out_out_name: &mut *const wchar_t) -> i32;
}

static mut pty_id: i32 = 1;

#[napi(js_name = "startProcess")]
fn start_process(
  file: String,
  cols: u32, rows: u32,
  debug: bool, pipe_name: String,
  conpty_inherit_cursor: bool) -> napi::Result<IConptyProcess> {

  let p = U16CString::from_str(pipe_name).unwrap();

  let mut h_in: *const cty::c_void = null();
  let mut in_name: *const wchar_t = null();
  let mut h_out: *const cty::c_void = null();
  let mut out_name: *const wchar_t = null();

  let hr = unsafe {
    CreateNamedPipesAndPseudoConsole(cols, rows,
                                     if conpty_inherit_cursor { 1 } else { 0 },
                                     p.as_ptr(),
                                     &mut h_in, &mut in_name,
                                     &mut h_out, &mut out_name)
  };

  if ! SUCCEEDED(hr) {
    panic!("conpty failed");
  }

  let result = IConptyProcess {
    fd: -1, pty: unsafe { pty_id += 1; pty_id },
    conin: unsafe { from_wchar_ptr(in_name) },
    conout: unsafe { from_wchar_ptr(out_name) }
  };
  return Ok(result);
}

unsafe fn from_wchar_ptr(s: *const wchar_t) -> String {
  U16CString::from_ptr_str(s).to_string_lossy().into()
}

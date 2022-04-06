
#[napi(object)]
pub struct IWinptyProcess {
    pub pty: i32,
    pub fd: i32,
    pub conin: String,
    pub conout: String,
    pub pid: i32,
    pub inner_pid: i32,
    pub inner_pid_handle: i32
}

#[allow(dead_code)]
#[napi]
fn winpty_start_process(file: String, commandLine: String, env: Vec<String>, cwd: String, cols: i32, rows: i32, debug: bool) -> napi::Result<IWinptyProcess> {
    #[cfg(not(target_family = "windows"))]
    return err!("Unsupported architecture");
    return Ok(IWinptyProcess{ pty: todo!(), fd: todo!(), conin: todo!(), conout: todo!(), pid: todo!(), inner_pid: todo!(), inner_pid_handle: todo!() });
}

#[allow(dead_code)]
#[napi]
fn winpty_resize(processHandle: i32, cols: i32, rows: i32) -> napi::Result<()> {
    #[cfg(not(target_family = "windows"))]
    return err!("Unsupported architecture");
    return Ok(());
}

#[allow(dead_code)]
#[napi]
fn winpty_kill(pid: i32, innerPidHandle: i32) -> napi::Result<()> {
    #[cfg(not(target_family = "windows"))]
    return err!("Unsupported architecture");
    return Ok(());
}

#[allow(dead_code)]
#[napi]
fn winpty_get_process_list(pid: i32) -> napi::Result<Vec<i32>> {
    #[cfg(not(target_family = "windows"))]
    return err!("Unsupported architecture");
    return Ok(vec![1]);
}

#[allow(dead_code)]
#[napi]
fn winpty_get_exit_code(innerPidHandle: i32) -> napi::Result<i32> {
    #[cfg(not(target_family = "windows"))]
    return err!("Unsupported architecture");
    return Ok(0);
}
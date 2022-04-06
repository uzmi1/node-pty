
#[allow(dead_code)]
#[napi]
fn get_console_process_list(pid: i32) -> napi::Result<Vec<i32>> {
    #[cfg(not(target_family = "windows"))]
    return err!("Unsupported architecture");
    return Ok(vec![]);
}
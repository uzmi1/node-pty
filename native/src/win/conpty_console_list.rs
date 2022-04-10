#![deny(clippy::all)]
/// Copyright (c) 2019, Microsoft Corporation (MIT License).
/// Copyright (c) 2022, Daniel Brenot (MIT License)
/// 
/// This file is responsible for getting process lists
/// on the windows platform

use windows::Win32::System::Console::{FreeConsole, AttachConsole, GetConsoleProcessList};
use std::os::raw::{c_int, c_uint};

use crate::err;

#[allow(dead_code)]
#[napi]
unsafe fn get_console_process_list(pid: i32) -> napi::Result<Vec<i32>> {
    #[cfg(not(target_family = "windows"))]
    return err!("Unsupported architecture");

    if !FreeConsole().as_bool() { return err!("FreeConsole failed"); }

    if !AttachConsole(pid as _).as_bool() { return err!("AttachConsole failed"); }

    // The count of how many 
    const PROCESS_COUNT: c_int = 64;
    // Create an array to be filled by the native system call
    let mut process_list = [0 as c_uint; PROCESS_COUNT as usize];
    let actual_count = GetConsoleProcessList(&mut process_list as _);
    let mut process_list = Vec::<i32>::with_capacity(actual_count as usize);
    for i in 0..actual_count {
        process_list.push(*process_list.get_unchecked(i as usize));
    }
    FreeConsole();
    return Ok(process_list);
}
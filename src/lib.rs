#[macro_use]
extern crate napi;
#[macro_use]
extern crate napi_derive;
#[macro_use]
extern crate serde_derive;
//#[macro_use]
//extern crate napi_derive;
use std::fs;
use std::os::unix::prelude::AsRawFd;
use std::os::unix::prelude::RawFd;
use std::ptr::null;
use std::ffi::c_void;
use std::convert::TryFrom;


// import the preludes
use napi::bindgen_prelude::*;
use napi::Result;


use nix::libc::*;

use nix::pty::PtyMaster;
use nix::pty::openpty;
use nix::sys::termios::Termios;
use nix::unistd::execvp;
use nix::libc::{O_NONBLOCK, TIOCSWINSZ, CTL_KERN, KERN_PROC, KERN_PROC_PID, winsize};
use nix::libc::{EBADF, EFAULT, EINVAL, ENOTTY};
use nix::unistd::tcgetpgrp;
use nix::pty::ptsname;
use nix::errno::errno;

use nix::libc::{ioctl, sysctl, FILE, fcntl, termios};

use std::ffi::CStr;

use serde_json::{Map, Value};
use serde::{Deserialize, Serialize};

use napi_derive::napi;


// use nix::libc::{execvp, ioctl, ptsname, winsize};

mod conpty_console_list;


// Structs

#[napi(object)]
#[derive(Serialize, Deserialize, Debug)]
struct IUnixProcess {
    pub fd: i32,
    pub pid: i32,
    pub pty: String
}

#[napi(object)]
#[derive(Serialize, Deserialize, Debug)]
struct IUnixOpenProcess {
    pub master: i32,
    pub slave: i32,
    pub pty: String
}

// Exposed functions for NAPI


#[napi]
fn fork<T: Fn(i32,i32) -> Result<()>>(
    file: String, args: Vec<String>,
    env: Vec<String>, cwd: String,
    cols: i32, rows: i32,
    uid: i32, gid: i32,
    utf8: bool, onexit: T) -> napi::Result<IUnixProcess> {

    let term = termios {
        c_iflag: ICRNL | IXON | IXANY | IMAXBEL | BRKINT,
        c_oflag: OPOST | ONLCR,
        c_cflag: CREAD | CS8 | HUPCL,
        c_lflag: ICANON | ISIG | IEXTEN | ECHO | ECHOE | ECHOK | ECHOKE | ECHOCTL,
        c_cc: val,
        c_ispeed: val,
        c_ospeed: val
    };

    if utf8 { term.c_iflag |= IUTF8; }

    term.c_cc[VEOF] = 4;
    term.c_cc[VEOL] = -1;
    term.c_cc[VEOL2] = -1;
    term.c_cc[VERASE] = 0x7f;
    term.c_cc[VWERASE] = 23;
    term.c_cc[VKILL] = 21;
    term.c_cc[VREPRINT] = 18;
    term.c_cc[VINTR] = 3;
    term.c_cc[VQUIT] = 0x1c;
    term.c_cc[VSUSP] = 26;
    term.c_cc[VSTART] = 17;
    term.c_cc[VSTOP] = 19;
    term.c_cc[VLNEXT] = 22;
    term.c_cc[VDISCARD] = 15;
    term.c_cc[VMIN] = 1;
    term.c_cc[VTIME] = 0;

    #[cfg(target_os = "macos")]
    {
        term.c_cc[VDSUSP] = 25;
        term.c_cc[VSTATUS] = 20;
    }

    cfsetispeed(&term, B38400);
    cfsetospeed(&term, B38400);

    let fd = -1;
    let newmask;
    let oldmask;


    // TODO fill in remaining implementation
    let pty = unsafe { ptsname(fd)? };
    return Ok(IUnixProcess {fd, pid, pty});

}

#[napi]
fn open(cols: u32, rows: u32) -> napi::Result<IUnixOpenProcess> {
    let ws_col = u16::try_from(cols);
    let ws_row = u16::try_from(rows);
    if ws_col.is_err() { return Err(napi::Error::new(napi::Status::InvalidArg, String::from("Failed to convert cols to a u16"))) }
    if ws_row.is_err() { return Err(napi::Error::new(napi::Status::InvalidArg, String::from("Failed to convert rows to a u16"))) }
    let winp = winsize {
        ws_col: ws_col.unwrap(), ws_row: ws_row.unwrap(),
        ws_xpixel: 0, ws_ypixel: 0
    };
    let (master, slave) = pty_openpty(winp, None, None)?;

    pty_nonblock(master.as_raw_fd())?;
    pty_nonblock(slave.as_raw_fd())?;

    // Takes the result from ptsname and converts it to a string for easy serialization
    let pty = unsafe { ptsname(&master) };
    if pty.is_err() { return Err(napi::Error::new(napi::Status::GenericFailure, "Failed to get the name of pty".to_string())); }
    return Ok(IUnixOpenProcess {master: master.as_raw_fd(), slave: slave.as_raw_fd(), pty: pty.unwrap()});
}

#[napi]
fn process(fd: i32, tty: String) -> Option<String> {
    // TODO do we want to replace this with a result and throw an error instead?
    if tty.is_empty() {return None; }
    let name = pty_getproc(fd, tty);
    match name {
        Ok(name) => return Some(name),
        Err(err) => return None,
    }
}

#[napi]
fn resize(fd: i32, cols: i32, rows: i32) -> Result<()>{
    let ws_col = u16::try_from(cols);
    let ws_row = u16::try_from(rows);
    if ws_col.is_err() { return Err(napi::Error::new(napi::Status::InvalidArg, String::from("Failed to convert cols to a u16"))) }
    if ws_row.is_err() { return Err(napi::Error::new(napi::Status::InvalidArg, String::from("Failed to convert rows to a u16"))) }
    let winp = winsize {
        ws_col: ws_col.unwrap(), ws_row: ws_row.unwrap(),
        ws_xpixel: 0, ws_ypixel: 0
    };
    if (unsafe { ioctl(fd, TIOCSWINSZ, &winp) } == -1) {
        Err(napi::Error::new(napi::Status::GenericFailure,
    String::from(match errno() {
                EBADF => "ioctl(2) failed, EBADF",
                EFAULT => "ioctl(2) failed, EFAULT",
                EINVAL => "ioctl(2) failed, EINVAL",
                ENOTTY => "ioctl(2) failed, ENOTTY",
                _ => "ioctl(2) failed"
            })
        ))
    } else {
        Ok(())
    }

}

// Helper functions to be used internally

/// execvpe(3) is not portable.
/// http://www.gnu.org/software/gnulib/manual/html_node/execvpe.html
fn pty_execvpe(file: String, argv: *const *const i8, envp: *const *const i8) -> i32 {
    // TODO implementation here
}

/// Nonblocking FD
fn pty_nonblock(fd: RawFd) -> Result<i32> {
    let flags = unsafe { fcntl(fd, F_GETFL, 0) };
    if flags == -1 { return Err(napi::Error::new(napi::Status::GenericFailure, "Failed at fcntl F_GETFL".to_string())); }
    flags = unsafe { fcntl(fd, F_SETFL, flags | O_NONBLOCK) };
    if flags == -1 { return Err(napi::Error::new(napi::Status::GenericFailure, "Failed at fcntl F_SETFL".to_string())); }
    Ok(flags)
}

/// pty_waitpid
/// Wait for SIGCHLD to read exit status.
fn pty_waitpid() {
    // TODO implementation here
}

/// pty_after_waitpid
/// Callback after exit status has been read.
fn pty_after_waitpid() {
    // TODO implementation here
}

/// pty_after_close
/// uv_close() callback - free handle data
fn pty_after_close() {
    // TODO implementation here
}

/// Taken from: tmux (http://tmux.sourceforge.net/)
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn pty_getproc(fd: i32, tty: String) -> Result<String> {
    return Err(());
}

/// Taken from: tmux (http://tmux.sourceforge.net/)
#[cfg(target_os = "linux")]
fn pty_getproc(fd: i32, tty: String) -> Result<String> {
    let f: *mut FILE;
    let mut path =  Vec::new();
    let pgrp = tcgetpgrp(fd)?;
    if !pgrp { return None; }
    // TODO check if this produces correct string
    write!(&path, "/proc/{}/cmdline", pgrp);
    if path.is_empty() { return None; }
    return fs::read_to_string(path)?;
}

#[cfg(target_os = "macos")]
fn pty_getproc(fd: i32, tty: String) -> Result<String> {

    let mib = [CTL_KERN, KERN_PROC, KERN_PROC_PID, 0];
    let kp: *mut ::c_void;
    let size = size_of(kp);
    mib[3] = tcgetpgrp(fd)?;
    if mib[3] == -1 { return Err(()); }
    let ctlRes = unsafe { sysctl(mib, 4, &kp, &size, null(), 0) };
    if ctlRes == -1 { return  Err(()); }
    //if ((size != sizeof(kp)) || kp);
    // TODO complete implementation
    return Ok();
}

/// Returns the master and slave fd's in a tuple
fn pty_openpty(winp: winsize, name: Option<String>, termp: Option<&Termios>) -> Result<(PtyMaster, PtyMaster)> {
    // TODO implementation here
    return Ok((0,0));
}

fn pty_forkpty(name: String, termp: Termios, winsize: winsize) -> Result<i32> {
    // TODO implementation here
    return Ok(0);
}





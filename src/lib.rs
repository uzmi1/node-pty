#[macro_use]
extern crate napi;
#[macro_use]
extern crate napi_derive;
#[macro_use]
extern crate serde_derive;
//#[macro_use]
//extern crate napi_derive;

use std::fmt::Result;
use std::fs;
use std::ptr::null;
use std::ffi::c_void;



// import the preludes
use napi::bindgen_prelude::*;

use nix::fcntl::FcntlArg::{F_GETFL, F_SETFL};

use nix::libc::tcgetattr;

use nix::unistd::execvp;
use nix::libc::{O_NONBLOCK, TIOCSWINSZ, CTL_KERN, KERN_PROC, KERN_PROC_PID, winsize};
use nix::unistd::tcgetpgrp;
use nix::pty::ptsname;
use nix::errno::errno;
use nix::errno::Errno::{EBADF, EFAULT, EINVAL, ENOTTY};

use nix::libc::{ioctl, sysctl, FILE };
use nix::fcntl::{fcntl};

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

//#[napi(object)]
#[derive(Serialize, Deserialize, Debug)]
struct IUnixOpenProcess {
    pub master: i32,
    pub slave: i32,
    pub pty: String
}

// Exposed functions for NAPI

/// Fork
#[napi]
fn fork<T: Fn(i32,i32) -> Result<()>>(
    file: String, args: Vec<String>,
    env: Vec<String>, cwd: String,
    cols: i32, rows: i32,
    uid: i32, gid: i32,
    utf8: bool, onexit: T) -> Result<IUnixProcess> {

    let mut term = termios::;
    term.c_iflag = ICRNL | IXON | IXANY | IMAXBEL | BRKINT;
    term.c_oflag = OPOST | ONLCR;
    term.c_cflag = CREAD | CS8 | HUPCL;
    term.c_lflag = ICANON | ISIG | IEXTEN | ECHO | ECHOE | ECHOK | ECHOKE | ECHOCTL;
    
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

//#[napi]
fn open(cols: u16, rows: u16) -> Result<IUnixOpenProcess> {
    let winp = winsize {
        ws_col: cols, ws_row: rows,
        ws_xpixel: 0, ws_ypixel: 0
    };
    let (master, slave) = pty_openpty(winp, None, None)?;

    if pty_nonblock(master) == -1 { Err("Could not set master fd to nonblocking.") }
    if pty_nonblock(slave) == -1 { Err("Could not set slave fd to nonblocking.") }
    
    // Takes the result from ptsname and converts it to a string for easy serialization
    let pty = unsafe { ptsname(master)? };
    return Ok(IUnixOpenProcess {master, slave, pty});
}

//#[napi]
fn process(fd: i32, tty: String) -> Option<String> {
    // TODO do we want to replace this with a result and throw an error instead?
    if tty.is_empty() {return None; }
    let name = pty_getproc(fd, tty);
    match name {
        Ok(name) => todo!(),
        Err(err) => return None,
    }
}

//#[napi]
fn resize(fd: i32, cols: i32, rows: i32) -> Result<()>{
    let winp = winsize {
        ws_col: cols, ws_row: rows,
        ws_xpixel: 0, ws_ypixel: 0
    };
    if (unsafe { ioctl(fd, TIOCSWINSZ, &winp) } == -1) {
        match errno() {
            EBADF => Err("ioctl(2) failed, EBADF"),
            EFAULT => Err("ioctl(2) failed, EFAULT"),
            EINVAL => Err("ioctl(2) failed, EINVAL"),
            ENOTTY => Err("ioctl(2) failed, ENOTTY"),
            _ => Err("ioctl(2) failed")
        }
    }
}

// Helper functions to be used internally

/// execvpe(3) is not portable.
/// http://www.gnu.org/software/gnulib/manual/html_node/execvpe.html
fn pty_execvpe(file: String, argv: *const *const i8, envp: *const *const i8) -> i32 {
    // TODO implementation here
}   

/// Nonblocking FD
fn pty_nonblock(fd: i32) -> Result<()> {
    let flags = fcntl(fd, F_GETFL, 0)?;
    if flags == -1 { return -1; }
    return fcntl(fd, F_SETFL, flags | O_NONBLOCK);
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
#[cfg(not(target_os = "macos"))]
#[cfg(not(target_os = "linux"))]
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
fn pty_openpty(winp: winsize, name: Option<String>, termp: Option<&Termios>) -> Result<(i32, i32)> {
    // TODO implementation here
    return Ok((0,0));
}

fn pty_forkpty(name: String, termp: Termios, winsize: winsize) -> Result<i32> {
    // TODO implementation here
    return Ok(0);
}





///
/// Copyright (c) 2012-2015, Christopher Jeffrey (MIT License)
/// Copyright (c) 2017, Daniel Imms (MIT License)
///
/// pty.rs:
///  This file is responsible for starting processes
///  with pseudo-terminal file descriptors.
///
/// See:
///  man pty
///  man tty_ioctl
///  man termios
///  man forkpty
///

// macros
#[macro_use]
extern crate napi;
#[macro_use]
extern crate napi_derive;
#[macro_use]
extern crate serde_derive;

// std imports
use std::borrow::Borrow;
use std::borrow::BorrowMut;
use std::ffi::CString;
use std::os::unix::prelude::AsRawFd;
use std::os::unix::prelude::RawFd;
use std::ptr::null;
use std::convert::TryFrom;
use napi::bindgen_prelude::*;
use napi::Result;

// nix imports
use nix::env;
use nix::pty::PtyMaster;
use nix::sys::signal::NSIG;
use nix::sys::termios::Termios;
use nix::pty::ptsname;
use nix::errno::errno;
use nix::unistd::chdir;

// nix libc
use nix::libc::{O_NONBLOCK, TIOCSWINSZ, CTL_KERN, KERN_PROC, KERN_PROC_PID, winsize};
use nix::libc::{B38400};
use nix::libc::{EBADF, EFAULT, EINVAL, ENOTTY};
use nix::libc::{sigfillset, ioctl, sysctl, forkpty, fcntl, termios};
use nix::libc::{cfsetispeed, cfsetospeed};
use nix::libc::*;

// Serde
use serde_json::{Map, Value};
use serde::{Deserialize, Serialize};


// use nix::libc::{execvp, ioctl, ptsname, winsize};

mod conpty_console_list;


// Structs for returned data

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

/// Creates a forked process to run the requested file.
#[napi]
fn fork<T: Fn(i32,i32) -> Result<()>>(
    file: String, args: Vec<String>,
    env: Vec<String>, cwd: String,
    cols: i32, rows: i32,
    uid: i32, gid: i32,
    utf8: bool, onexit: T) -> napi::Result<IUnixProcess> {

    // fd of the new forked process
    let mut master = -1;
    //
    let mut newmask: sigset_t;
    let mut oldmask: sigset_t;
    //
    let sig_action: sigaction;

    // Terminal window size
    let winp = winsize {
        ws_col: cols as u16, ws_row: rows as u16,
        ws_xpixel: 0, ws_ypixel: 0
    };

    // Create a new termios with default flags.
    // For more info on termios settings:
    // https://man7.org/linux/man-pages/man3/termios.3.html
    let term = termios {
        c_iflag: ICRNL | IXON | IXANY | IMAXBEL | BRKINT,
        c_oflag: OPOST | ONLCR,
        c_cflag: CREAD | CS8 | HUPCL,
        c_lflag: ICANON | ISIG | IEXTEN | ECHO | ECHOE | ECHOK | ECHOKE | ECHOCTL,
        c_cc: Default::default(),
        c_ispeed: Default::default(),
        c_ospeed: Default::default()
    };

    // Enable utf8 support if requested
    if utf8 { term.c_iflag |= IUTF8; }

    // Set supported terminal characters
    term.c_cc[VEOF] = 4;
    term.c_cc[VEOL] = 255;
    term.c_cc[VEOL2] = 255;
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

    // Specific character support for macos
    #[cfg(target_os = "macos")]
    {
        term.c_cc[VDSUSP] = 25;
        term.c_cc[VSTATUS] = 20;
    }

    unsafe {
        // Set terminal input and output baud rate
        cfsetispeed(term.borrow_mut(), B38400);
        cfsetospeed(term.borrow_mut(), B38400);

        // temporarily block all signals
        // this is needed due to a race condition in openpty
        // and to avoid running signal handlers in the child
        // before exec* happened
        sigfillset(newmask.borrow_mut());
        pthread_sigmask(SIG_SETMASK, newmask.borrow_mut(), oldmask.borrow_mut());
    }

    // Forks and then assigns a pointer to the fork file descriptor to master
    let pid = pty_forkpty(master, term, winp);

    if pid == 0 {
        // remove all signal handler from child
        sig_action.sa_sigaction = SIG_DFL;
        sig_action.sa_flags = 0;
        unsafe {
            sigemptyset(sig_action.sa_mask.borrow_mut());
            for i in 0..NSIG {
                sigaction(i, &sig_action, null() as *mut nix::libc::sigaction);
            }
        }
    }

    // Reenable signals
    unsafe { pthread_sigmask(SIG_SETMASK, oldmask.borrow_mut(), null() as *mut u32); }

    match pid {
        -1 => { return Err(napi::Error::new(napi::Status::GenericFailure, "forkpty(3) failed.".to_string())) },
        0 => {
            if !cwd.is_empty() {
                unsafe {
                    if chdir(cwd.borrow()).is_err() { panic!("chdir(2) failed."); }

                    if uid != -1 && gid != -1 {
                        if setgid(gid as u32) == -1 { panic!("setgid(2) failed."); }
                        if setuid(uid as u32) == -1 { panic!("setuid(2) failed."); }
                    }
                    // Allocate a vector the size of the args, with space for a file and null terminator
                    let argv = Vec::<*const *const i8>::with_capacity(args.len() + 2);
                    // Set the file as the first argument
                    argv[0] = CString::new(file)?.as_ptr() as *const *const i8;
                    // Terminate the argument array with null to designate the end
                    argv[argv.len()-1] = null();
                    // Fill the existing arguments into the middle of the array
                    for i in 0..argv.len() - 2 {
                        argv[i+1] = CString::new(args[i])?.as_ptr() as *const *const i8;
                    }
                    pty_execvpe(file, argv.as_slice()[0], env);

                    panic!("execvp(3) failed.")

                };
            }
        },
        _ => {}
    };

    let pty = pty_ptsname(master)?;
    return Ok(IUnixProcess {fd: master, pid, pty});

}

#[napi]
fn open(cols: u32, rows: u32) -> napi::Result<IUnixOpenProcess> {
    let winp = winsize {
        ws_col: cols as u16, ws_row: rows as u16,
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
    unsafe {
        let environ: *mut *mut *mut c_char;
        #[cfg(target_os = "macos")] { environ = _NSGetEnviron(); }
        #[cfg(not(target_os = "macos"))] { environ = environ; }
    }
    return 0;
}

/// Nonblocking FD
fn pty_nonblock(fd: RawFd) -> Result<i32> {
    let flags = unsafe { fcntl(fd, F_GETFL, 0) };
    if flags == -1 { return Err(napi::Error::new(napi::Status::GenericFailure, "Failed at fcntl F_GETFL".to_string())); }
    flags = unsafe { fcntl(fd, F_SETFL, flags | O_NONBLOCK) };
    if flags == -1 { return Err(napi::Error::new(napi::Status::GenericFailure, "Failed at fcntl F_SETFL".to_string())); }
    Ok(flags)
}

/// Wait for SIGCHLD to read exit status.
fn pty_waitpid() {
    // TODO implementation here
}

/// Callback after exit status has been read.
fn pty_after_waitpid() {
    // TODO implementation here
}

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

/// Passes the call to the unsafe function forkpty
#[cfg(target_os = "macos")]
fn pty_forkpty(mut master: i32, mut termp: termios, mut winp: winsize) -> i32 {
    unsafe {
        forkpty(
    master.borrow_mut(),
        *null(),
        termp.borrow_mut() as *mut nix::libc::termios,
        winp.borrow_mut() as *mut nix::libc::winsize
        )
    }
}

/// Get's the name of the terminal pointed to by the given file descriptor
fn pty_ptsname(master: &PtyMaster) -> Result<String> {
    let name = unsafe { ptsname(master) };
    match name {
        Ok(name) => return Ok(name),
        Err(err) => return Err(napi::Error::new(napi::Status::GenericFailure, "Failed to get slave name".to_string())),
    }
}




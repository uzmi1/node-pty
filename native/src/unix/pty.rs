#![deny(clippy::all)]

use napi::JsFunction;

#[cfg(target_family = "windows")]
use crate::err;

#[cfg(not(target_family = "windows"))] use {
  std::ffi::{CStr, CString},
  std::io::Write,
  std::process::Command,
  std::ptr::{null, null_mut},
  nix::libc::{O_NONBLOCK, TIOCSWINSZ, winsize},
  nix::libc::{B38400},
  nix::libc::{sigfillset, ioctl, sysctl, forkpty, fcntl, termios},
  nix::libc::{cfsetispeed, cfsetospeed},
  nix::sys::signal::Signal,
  nix::libc::*,
  nix::libc::openpty,
  nix::errno::Errno,
  nix::unistd::chdir,
  napi::{Result, JsFunction},
  napi::threadsafe_function::{ThreadSafeCallContext, ThreadsafeFunctionCallMode, ErrorStrategy}
};



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

#[allow(dead_code)]
#[napi]
fn pty_fork(
  file: String, args: Vec<String>,
  env: Vec<String>, cwd: String,
  cols: i32, rows: i32,
  uid: i32, gid: i32,
  utf8: bool, onexit: JsFunction) -> napi::Result<IUnixProcess> {
  #[cfg(target_family = "windows")]
  return err!("Unsupported architecture");
  #[cfg(not(target_family = "windows"))] {
    //
    let mut newmask: sigset_t = unsafe { std::mem::zeroed() };
    let mut oldmask: sigset_t = unsafe { std::mem::zeroed() };
    //
    let mut sig_action = mk_sigaction();

    // Terminal window size
    let mut winp = winsize {
      ws_col: cols as u16, ws_row: rows as u16,
      ws_xpixel: 0, ws_ypixel: 0
    };

    // Create a new termios with default flags.
    // For more info on termios settings:
    // https://man7.org/linux/man-pages/man3/termios.3.html
    let mut term: termios = unsafe { std::mem::zeroed() };
    // (Cannot use struct initializer because different *nixes have different fields)
    term.c_iflag = ICRNL | IXON | IXANY | IMAXBEL | BRKINT;
    term.c_oflag = OPOST | ONLCR;
    term.c_cflag = CREAD | CS8 | HUPCL;
    term.c_lflag = ICANON | ISIG | IEXTEN | ECHO | ECHOE | ECHOK | ECHOKE | ECHOCTL;

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
      cfsetispeed(&mut term, B38400);
      cfsetospeed(&mut term, B38400);

      // temporarily block all signals
      // this is needed due to a race condition in openpty
      // and to avoid running signal handlers in the child
      // before exec* happened
      sigfillset(&mut newmask);
      pthread_sigmask(SIG_SETMASK, &mut newmask, &mut oldmask);
    }

    // Forks and then assigns a pointer to the fork file descriptor to master
    let mut master: i32 = -1;
    let pid = unsafe { forkpty(&mut master, null_mut(), &mut term, &mut winp) };

    if pid == 0 {
      // remove all signal handlers from child
      sig_action.sa_sigaction = SIG_DFL;
      sig_action.sa_flags = 0;
      unsafe {
        sigemptyset(&mut sig_action.sa_mask);
        for i in Signal::iterator() {
          sigaction(i as c_int, &sig_action, null_mut());
        }
      }
    }

    // Reenable signals
    unsafe { pthread_sigmask(SIG_SETMASK, &mut oldmask, null_mut()); }

    match pid {
      -1 => { return err!("forkpty(3) failed.") },
      0 => {
        unsafe {
          if !cwd.is_empty() {
            if chdir(cwd.as_str()).is_err() { child_panic("chdir(2) failed"); }
          }

          if uid != -1 && gid != -1 {
            if setgid(gid as u32) == -1 { child_panic("setgid(2) failed"); }
            if setuid(uid as u32) == -1 { child_panic("setuid(2) failed"); }
          }
          // Prepare char *argv[]: [file, ...args, null]
          let cargs = vec![&file].into_iter().chain(args.iter())
              .map(|s| { cstr_unsafe(s.clone()) })
              .collect::<Vec<_>>();
          let argv = nul_terminated(&cargs);

          // Prepare char *envv[]: [...env, null]
          let cenv = env.iter().map(|s| { cstr_unsafe(s.clone()) })
              .collect::<Vec<_>>();
          let envv = nul_terminated(&cenv);

          pty_execvpe(CString::new(file)?.as_ptr(), argv.as_ptr(), envv.as_ptr());

          child_panic("execvp(3) failed");
        }
      },
      _ => {
        unsafe { pty_nonblock(master)?; }

        let tsfn = onexit.create_threadsafe_function::<_, _, _, ErrorStrategy::Fatal>(0,
          |ctx: ThreadSafeCallContext<(u32,u32)>| {
            // convert tuple to vec of size 2. @todo better way via serde?
            ctx.env.create_uint32(ctx.value.0).and_then(|v0| {
              ctx.env.create_uint32(ctx.value.1).map(|v1| { vec![v0, v1] })
            })
          })?;

        std::thread::spawn(move || {
          let rc = unsafe { pty_waitpid(pid) };
          //std::thread::sleep(std::time::Duration::from_millis(1000));
          tsfn.call(rc, ThreadsafeFunctionCallMode::Blocking);
        });
      }
    };

    let pty = unsafe { pty_ptsname(master).expect("ptsname failed") };
    return Ok(IUnixProcess {fd: master, pid, pty});
  }
}

#[allow(dead_code)]
#[napi]
fn pty_open(cols: u32, rows: u32) -> napi::Result<IUnixOpenProcess> {
  #[cfg(target_family = "windows")]
  return err!("Unsupported architecture");
  #[cfg(not(target_family = "windows"))] {
    // Terminal window size
    let mut winp = winsize {
      ws_col: cols as u16, ws_row: rows as u16,
      ws_xpixel: 0, ws_ypixel: 0
    };

    let mut amaster: i32 = 0;
    let mut aslave: i32 = 0;
    unsafe {
      openpty(&mut amaster, &mut aslave, null::<i8>() as *mut i8, null::<i8>() as *mut termios,
        &mut winp);
    }

    return Ok(IUnixOpenProcess {master: amaster, slave: aslave, pty: String::new()});
  }
}

// Gets the name of the process with the given fie descriptor
#[allow(dead_code)]
#[napi]
fn pty_process(fd: i32, tty: String) -> Option<String> {
  #[cfg(target_family = "windows")]
  return None;
  #[cfg(not(target_family = "windows"))] {
    // TODO do we want to replace this with a result and throw an error instead?
    if tty.is_empty() {return None; }
    let name = pty_getproc(fd, tty);
    match name {
        Ok(name) => return Some(name),
        Err(err) => return None,
    }
  }
}

/// Resizes the terminal pointed to by the provided file descriptor
/// to the preferred
#[allow(dead_code)]
#[napi]
fn pty_resize(fd: i32, cols: i32, rows: i32) -> napi::Result<()>{
  #[cfg(target_family = "windows")]
  return err!("Unsupported architecture");
  #[cfg(not(target_family = "windows"))] {
    let winp = winsize {
        ws_col: cols as u16, ws_row: rows as u16,
        ws_xpixel: 0, ws_ypixel: 0
    };
    if (unsafe { ioctl(fd, TIOCSWINSZ, &winp) } == -1) {
        match Errno::last() {
            EBADF => err!("ioctl(2) failed, EBADF"),
            EFAULT =>err!( "ioctl(2) failed, EFAULT"),
            EINVAL =>err!( "ioctl(2) failed, EINVAL"),
            ENOTTY =>err!( "ioctl(2) failed, ENOTTY"),
            _ => err!("ioctl(2) failed")
        }
    } else {
        Ok(())
    }
  }
}

#[cfg(not(target_family = "windows"))]
fn cstr_unsafe(s: String) -> CString {
  CString::new(s).expect("CString::new failed")
}

#[cfg(not(target_family = "windows"))]
fn cstr_unsafe_(s: &str) -> CString {
  CString::new(s).expect("CString::new failed")
}

#[cfg(not(target_family = "windows"))]
fn nul_terminated(arr: &Vec<CString>) -> Vec<*const c_char> {
  arr.iter().map(|s| { s.as_ptr() })
      .chain(vec![null()].into_iter()).collect::<Vec<_>>()
}

#[cfg(not(target_family = "windows"))]
fn child_panic(s: &str) {
  unsafe {
    perror(cstr_unsafe_(s).as_ptr());
    exit(1);
  }
}

/// Get's the name of the terminal pointed to by the given file descriptor
#[cfg(not(target_family = "windows"))]
unsafe fn pty_ptsname(master: c_int) -> nix::Result<String> {
  let name_ptr = ptsname(master);
  if name_ptr.is_null() {
    return Err(Errno::last());
  }

  let name = CStr::from_ptr(name_ptr);
  Ok(name.to_string_lossy().into_owned())
}

/// execvpe(3) is not portable.
/// http://www.gnu.org/software/gnulib/manual/html_node/execvpe.html
#[cfg(not(target_family = "windows"))]
unsafe fn pty_execvpe(file: *const i8, argv: *const *const i8, envp: *const *const i8) -> i32 {
  /* this is the hackiest, but that's what used to be in the C++ implementation */
  extern "C" {
    static mut environ: *const *const i8;
  }
  environ = envp;
  /* suggestion: pass envp as Vec<String> and use
   *   nix::env::clearenv();
   *   std::env::setenv(...);
   * also, optimization: change `unixTerminal.ts` to pass `undefined` in case
   * an `env` option is not set. And then, in this case, skip this charade
   * altogether.
   */
  return execvp(file, argv);
}

#[cfg(not(target_family = "windows"))]
unsafe fn pty_nonblock(fd: c_int) -> Result<c_int> {
  match fcntl(fd, F_GETFL, 0) {
    -1 => return err!("failed to set nonblocking mode (fcntl(F_GETFL) failed)"),
    flags => match fcntl(fd, F_SETFL, flags | O_NONBLOCK) {
      -1 => return err!("failed to set nonblocking mode (fcntl(F_SETFL) failed)"),
      rc => Ok(rc)
    }
  }
}

#[cfg(not(target_family = "windows"))]
unsafe fn pty_waitpid(pid: pid_t) -> (u32, u32) {
  let mut stat_loc: c_int = 0;
  let ret = waitpid(pid, &mut stat_loc, 0);
  match ret {
    -1 => {
      match Errno::last() {
        Errno::EINTR => pty_waitpid(pid),
        Errno::ECHILD => (0, 0),
        _ => panic!("waitpid(3): unexpected error")
      }
    },
    _ => {
      (if WIFEXITED(stat_loc) { WEXITSTATUS(stat_loc) as u32 } else { 0 },
       if WIFSIGNALED(stat_loc) { WTERMSIG(stat_loc) as u32 } else { 0 })
    }
  }
}


#[cfg(target_os = "macos")]
fn mk_sigaction() -> sigaction {
  sigaction {
    sa_sigaction: SIG_DFL,
    sa_mask: unsafe { std::mem::zeroed() },
    sa_flags: 0
  }
}

#[cfg(target_os = "linux")]
fn mk_sigaction() -> sigaction {
  sigaction {
    sa_sigaction: SIG_DFL,
    sa_mask: unsafe { std::mem::zeroed() },
    sa_flags: 0,
    sa_restorer: None
  }
}

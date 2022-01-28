extern crate napi_build;
#[cfg(target_family = "windows")]
extern crate cc;

fn main() {
  napi_build::setup();
  #[cfg(target_family = "windows")]
  {
    cc::Build::new()
        .file("src/win/conpty.cc")
        .compile("conpty");
  }
}

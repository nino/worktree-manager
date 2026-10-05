//! Embeds `res/app.manifest` (Common Controls 6, per-monitor DPI awareness)
//! as the executable's manifest resource.
//!
//! The resource is compiled into a one-object static library linked with
//! `+whole-archive`: a linker drops archive members nothing refers to, and
//! nothing refers to a resource. A library's link-lib, unlike a link-arg,
//! reaches the final link of every executable that depends on this crate,
//! so `wtm-app` gets the manifest without a build script of its own.
//!
//! Where the resource compiler cannot be found the build goes on with a
//! warning, and `lib.rs` activates an equivalent manifest at run time.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=res/app.rc");
    println!("cargo:rerun-if-changed=res/app.manifest");
    println!("cargo:rerun-if-env-changed=WINDRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let res = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir")).join("res");
    let built = match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
        Ok("msvc") => msvc(&res, &out),
        _ => gnu(&res, &out),
    };
    match built {
        Ok(()) => {
            println!("cargo:rustc-link-search=native={}", out.display());
            println!("cargo:rustc-link-lib=static:+whole-archive,-bundle=wtm_manifest");
        }
        Err(e) => println!(
            "cargo:warning=the application manifest was not embedded ({e}); it is activated at run time instead"
        ),
    }
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let shown = format!("{cmd:?}");
    match cmd.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("{shown} exited with {s}")),
        Err(e) => Err(format!("{shown}: {e}")),
    }
}

/// The mingw tools, by their target-prefixed names first (a cross build),
/// then plain (a build on Windows with mingw on the PATH).
fn tool(name: &str) -> String {
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "x86_64".into());
    let prefixed = format!("{arch}-w64-mingw32-{name}");
    if Command::new(&prefixed).arg("--version").output().is_ok() {
        prefixed
    } else {
        name.to_string()
    }
}

fn gnu(res: &Path, out: &Path) -> Result<(), String> {
    let windres = std::env::var("WINDRES").unwrap_or_else(|_| tool("windres"));
    let object = out.join("app_manifest.o");
    run(Command::new(&windres)
        .current_dir(res)
        .args(["-O", "coff", "-i", "app.rc", "-o"])
        .arg(&object))?;
    let archive = out.join("libwtm_manifest.a");
    let _ = std::fs::remove_file(&archive);
    run(Command::new(tool("ar"))
        .arg("crs")
        .arg(&archive)
        .arg(&object))
}

fn msvc(res: &Path, out: &Path) -> Result<(), String> {
    let compiled = out.join("app.res");
    run(Command::new("rc.exe")
        .current_dir(res)
        .arg("/nologo")
        .arg("/fo")
        .arg(&compiled)
        .arg("app.rc"))?;
    let object = out.join("app_manifest.obj");
    run(Command::new("cvtres.exe")
        .arg("/nologo")
        .arg("/machine:x64")
        .arg(format!("/out:{}", object.display()))
        .arg(&compiled))?;
    run(Command::new("lib.exe")
        .arg("/nologo")
        .arg(format!("/out:{}", out.join("wtm_manifest.lib").display()))
        .arg(&object))
}

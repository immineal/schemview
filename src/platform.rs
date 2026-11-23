//! The little bit that differs between desktop and Android.
//!
//! On Android the Java side (`android/java/.../MainActivity.java`) does what needs the
//! Android framework: file pickers, "Open with", copying content:// files, downloading the
//! texture jar, window insets. It reports back through the JNI functions in
//! `android/native/src/lib.rs`, which call [`post`]. The viewer drains the inbox once per frame.

use eframe::egui;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Phone-style UI: touch gestures, bigger controls, on-screen buttons instead of key hints.
pub const TOUCH: bool = cfg!(target_os = "android");

pub enum Msg {
    /// Open this local file (already copied out of whatever content:// URI it came from).
    Open(PathBuf),
    /// A line of status text; `None` clears it. `sticky` keeps it until replaced.
    Status(Option<String>, bool),
    /// A texture jar was installed or removed: reload the open file.
    TexturesChanged,
    /// Safe-area insets in physical pixels: left, top, right, bottom.
    Insets([f32; 4]),
    /// The system Back gesture or button.
    Back,
}

static INBOX: Mutex<Vec<Msg>> = Mutex::new(Vec::new());
static CTX: OnceLock<egui::Context> = OnceLock::new();

pub fn post(m: Msg) {
    if let Ok(mut q) = INBOX.lock() {
        q.push(m);
    }
    if let Some(c) = CTX.get() {
        c.request_repaint();
    }
}

pub(crate) fn take() -> Vec<Msg> {
    INBOX.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
}

pub(crate) fn set_ctx(ctx: &egui::Context) {
    let _ = CTX.set(ctx.clone());
}

#[cfg(target_os = "android")]
mod imp {
    use jni::objects::JObject;
    use std::sync::OnceLock;

    /// JavaVM and the NativeActivity (a JNI global reference, valid for the process lifetime:
    /// the process exits when the activity goes away).
    static JAVA: OnceLock<(usize, usize)> = OnceLock::new();

    pub fn init(vm: *mut std::ffi::c_void, activity: *mut std::ffi::c_void) {
        let _ = JAVA.set((vm as usize, activity as usize));
    }

    /// Call a `void name()` method on the activity. Errors are logged, never fatal.
    pub fn call(name: &str) {
        let Some(&(vm, act)) = JAVA.get() else { return };
        let r = (|| -> jni::errors::Result<()> {
            let vm = unsafe { jni::JavaVM::from_raw(vm as *mut jni::sys::JavaVM)? };
            let mut env = vm.attach_current_thread()?;
            let obj = unsafe { JObject::from_raw(act as jni::sys::jobject) };
            let res = env.call_method(&obj, name, "()V", &[]);
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
            res.map(|_| ())
        })();
        if let Err(e) = r {
            log::error!("schemview: JNI call {name} failed: {e}");
        }
    }
}

#[cfg(target_os = "android")]
pub use imp::init as android_init;

fn call(_name: &str) {
    #[cfg(target_os = "android")]
    imp::call(_name);
}

/// Diagnostics: logcat on Android (tag "schemview"), stderr with SCHEMVIEW_DEBUG on desktop.
pub fn log(msg: &str) {
    #[cfg(target_os = "android")]
    log::info!("{msg}");
    #[cfg(not(target_os = "android"))]
    if std::env::var_os("SCHEMVIEW_DEBUG").is_some() {
        eprintln!("{msg}");
    }
}

/// Show the system file picker for a schematic.
pub fn pick_file() {
    call("pickSchematic");
}
/// Show the system file picker for a Minecraft client jar.
pub fn pick_jar() {
    call("pickJar");
}
/// Download the latest release client jar from Mojang (progress arrives as `Msg::Status`).
pub fn download_textures() {
    call("downloadTextures");
}
/// Back button with nothing left to close: leave the app, keeping its state.
pub fn go_back() {
    call("goBack");
}

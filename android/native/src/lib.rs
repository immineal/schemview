//! Android entry points: `android_main` (started by the NativeActivity glue) and the JNI
//! callbacks that `MainActivity.java` uses to talk to the viewer.

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jint};
use jni::JNIEnv;
use schemview::platform::{post, Msg};
use winit::platform::android::activity::AndroidApp;

static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[no_mangle]
fn android_main(app: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("schemview"));
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        // A second activity in this process; MainActivity forwards its file and finishes it.
        log::warn!("second activity instance: not starting another viewer");
        return;
    }
    std::panic::set_hook(Box::new(|p| log::error!("panic: {p}")));
    schemview::android_main(app);
    // Android allows one event loop per process. Exit, so the next launch gets a fresh process
    // instead of a crash when the activity is created again.
    std::process::exit(0);
}

fn string(env: &mut JNIEnv, s: &JString) -> Option<String> {
    if s.is_null() {
        return None;
    }
    env.get_string(s).ok().map(|s| s.into())
}

#[no_mangle]
extern "system" fn Java_dev_immineal_schemview_MainActivity_nativeOpen(mut env: JNIEnv, _c: JClass, path: JString) {
    if let Some(p) = string(&mut env, &path) {
        post(Msg::Open(p.into()));
    }
}

#[no_mangle]
extern "system" fn Java_dev_immineal_schemview_MainActivity_nativeStatus(mut env: JNIEnv, _c: JClass, text: JString, sticky: jboolean) {
    post(Msg::Status(string(&mut env, &text), sticky != 0));
}

#[no_mangle]
extern "system" fn Java_dev_immineal_schemview_MainActivity_nativeTexturesChanged(_env: JNIEnv, _c: JClass) {
    post(Msg::TexturesChanged);
}

#[no_mangle]
extern "system" fn Java_dev_immineal_schemview_MainActivity_nativeBack(_env: JNIEnv, _c: JClass) {
    post(Msg::Back);
}

#[no_mangle]
extern "system" fn Java_dev_immineal_schemview_MainActivity_nativeInsets(_env: JNIEnv, _c: JClass, l: jint, t: jint, r: jint, b: jint) {
    post(Msg::Insets([l as f32, t as f32, r as f32, b as f32]));
}

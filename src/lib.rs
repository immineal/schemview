//! schemview - a small, fast viewer for Minecraft schematics.

mod app;
mod assets;
mod blocks;
mod formats;
mod legacy;
mod mesh;
mod nbt;
pub mod platform;
mod render;

pub use app::desktop_main;
#[cfg(target_os = "android")]
pub use app::android_main;

//! Usage: schemview [FILE]        open a viewer window
//!        schemview --stats FILE  parse + mesh without a window, print statistics

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    schemview::desktop_main();
}

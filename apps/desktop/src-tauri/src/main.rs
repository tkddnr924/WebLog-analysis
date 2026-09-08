//! 얇은 진입점. 모든 조립은 lib.rs에 있다.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    weblog_desktop_lib::run();
}

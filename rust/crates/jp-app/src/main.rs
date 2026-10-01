// Windows 上 release 构建不要弹控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    jp_app_lib::run()
}

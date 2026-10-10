#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    let data_dir = nova::paths::take_data_dir_arg(&mut args);
    nova::paths::init_data_dir(data_dir);
    if args.iter().any(|a| a == "--version") {
        println!("{} {}", nova::APP_NAME, nova::VERSION);
    }
}

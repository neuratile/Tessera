#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

fn main() {
    // Loads `apps/desktop/.env` when present (`dotenvy`) before `AppConfig::from_env`.
    testing_ide_lib::config::load_dotenv_optional();
    testing_ide_lib::run();
}

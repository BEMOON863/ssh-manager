//! Библиотечная часть Ssh-manager: модель данных, хранение, SSH-утилиты и GUI.
//!
//! Бинарный крейт (`src/main.rs`) содержит только запуск приложения.

pub mod app;
pub mod model;
pub mod probe;
pub mod remote_files;
pub mod ssh;
pub mod store;
pub mod terminal;
pub mod theme;
pub mod ui;

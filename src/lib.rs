//! Pebbly: Samsung One UI's Now Bar, living inside the Windows taskbar.

pub mod activities;
pub mod app;
pub mod claude_hooks;
pub mod i18n;
pub mod legacy;
pub mod platform;
pub mod render;
pub mod settings;
pub mod settings_ui;
pub mod slint_shot;
pub mod update;

/// Pebbly's folder under %APPDATA% (settings) and %LOCALAPPDATA% (log, caches, update state).
pub const DIR: &str = "Pebbly";

#[cfg(target_os = "windows")]
#[path = "desktop/windows.rs"]
mod win_desktop;

#[cfg(target_os = "macos")]
#[path = "desktop/macos.rs"]
mod mac_desktop;

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
#[path = "desktop/unix.rs"]
mod unix_desktop;

#[cfg(target_os = "windows")]
pub use win_desktop::*;

#[cfg(target_os = "macos")]
pub use mac_desktop::*;

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
pub use unix_desktop::*;

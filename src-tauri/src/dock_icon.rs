//! Dock icon follows the effective appearance: the dark Amara icon in dark mode, the light
//! one otherwise. Applied at startup, on `WindowEvent::ThemeChanged`, and when the frontend
//! calls `sync_dock_icon` after changing the window theme (Settings → Appearance).
//! Both PNGs are compiled into the binary, so no runtime path lookup can fail or leak.

use tauri::{AppHandle, Manager, Runtime, Theme, WebviewWindow};

pub const LIGHT_ICON: &[u8] = include_bytes!("../icons/1024x1024.png");
pub const DARK_ICON: &[u8] = include_bytes!("../icons/dark/icon-1024.png");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockIcon {
    Light,
    Dark,
}

/// Unknown/unspecified themes fall back to the light (bundle) icon.
pub fn icon_for(theme: Option<Theme>) -> DockIcon {
    match theme {
        Some(Theme::Dark) => DockIcon::Dark,
        _ => DockIcon::Light,
    }
}

pub fn icon_bytes(icon: DockIcon) -> &'static [u8] {
    match icon {
        DockIcon::Light => LIGHT_ICON,
        DockIcon::Dark => DARK_ICON,
    }
}

/// Read the window's effective theme and set the Dock icon to match. Never fails loudly.
pub fn sync<R: Runtime>(window: &WebviewWindow<R>) {
    let icon = icon_for(window.theme().ok());
    let _ = window.run_on_main_thread(move || set_dock_icon(icon));
}

#[cfg(target_os = "macos")]
fn set_dock_icon(icon: DockIcon) {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(icon_bytes(icon));
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: called on the main thread (checked above) with a valid, owned NSImage.
    unsafe { app.setApplicationIconImage(Some(&image)) };
}

#[cfg(not(target_os = "macos"))]
fn set_dock_icon(_icon: DockIcon) {}

/// Called by the frontend after `setTheme` so an app-pinned theme updates the Dock too.
#[tauri::command]
pub fn sync_dock_icon<R: Runtime>(app: AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        sync(&window);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_theme_uses_dark_icon_everything_else_light() {
        assert_eq!(icon_for(Some(Theme::Dark)), DockIcon::Dark);
        assert_eq!(icon_for(Some(Theme::Light)), DockIcon::Light);
        assert_eq!(icon_for(None), DockIcon::Light);
    }

    #[test]
    fn embedded_icons_are_distinct_pngs() {
        const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
        assert!(LIGHT_ICON.starts_with(PNG));
        assert!(DARK_ICON.starts_with(PNG));
        assert_ne!(icon_bytes(DockIcon::Light), icon_bytes(DockIcon::Dark));
    }
}

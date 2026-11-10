//! Native modal dialogs run on their own UI thread; preparation and the window
//! keep drawing. Paths come from the chooser, never from environment substitutes.
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
};

pub const TITLE: &str = "Choose Star Wars Battlefront or prepared game data";
pub const INVALID: &str =
    "That folder holds neither Star Wars Battlefront (2004) nor prepared game data.";

type Reply = mpsc::Sender<Result<Option<PathBuf>, String>>;
enum Request {
    Choose(bool, Reply),
    Warning(PathBuf, Reply),
}

fn request(make: impl FnOnce(Reply) -> Request) -> Receiver<Result<Option<PathBuf>, String>> {
    static SERVICE: std::sync::OnceLock<mpsc::Sender<Request>> = std::sync::OnceLock::new();
    let sender = SERVICE.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        // A single thread owns every GTK object across repeated dialogs.
        std::thread::spawn(move || {
            while let Ok(request) = rx.recv() {
                match request {
                    Request::Choose(import, reply) => {
                        let _ = reply.send(choose_native(import));
                    }
                    Request::Warning(path, reply) => {
                        let _ = reply.send(warning_native(&path).map(|_| None));
                    }
                }
            }
        });
        tx
    });
    let (tx, rx) = mpsc::channel();
    let _ = sender.send(make(tx));
    rx
}
pub fn choose() -> Receiver<Result<Option<PathBuf>, String>> {
    request(|reply| Request::Choose(false, reply))
}
pub fn warning(path: PathBuf) -> Receiver<Result<Option<PathBuf>, String>> {
    request(|reply| Request::Warning(path, reply))
}

#[cfg(target_os = "linux")]
fn gtk_library() -> Result<&'static libloading::Library, String> {
    static GTK: std::sync::OnceLock<Result<libloading::Library, String>> =
        std::sync::OnceLock::new();
    // SAFETY: symbols are requested with GTK's C signatures below, and the
    // library stays loaded while GTK's registered types retain callbacks.
    GTK.get_or_init(|| unsafe {
        libloading::Library::new("libgtk-3.so.0").map_err(|e| e.to_string())
    })
    .as_ref()
    .map_err(Clone::clone)
}

#[cfg(target_os = "linux")]
fn flush_dialog_changes(gtk: &libloading::Library) -> Result<(), String> {
    // The service sleeps between requests instead of running gtk_main. Flush
    // destruction now so the closed dialog cannot remain painted over the game.
    unsafe {
        let display: libloading::Symbol<unsafe extern "C" fn() -> *mut std::ffi::c_void> = gtk
            .get(b"gdk_display_get_default\0")
            .map_err(|e| e.to_string())?;
        let flush: libloading::Symbol<unsafe extern "C" fn(*mut std::ffi::c_void)> =
            gtk.get(b"gdk_display_flush\0").map_err(|e| e.to_string())?;
        flush(display());
    }
    Ok(())
}

pub fn choose_export() -> Receiver<Result<Option<PathBuf>, String>> {
    request(|reply| Request::Choose(true, reply))
}

#[cfg(target_os = "linux")]
fn choose_native(import: bool) -> Result<Option<PathBuf>, String> {
    use std::ffi::{c_char, c_void, CStr, CString};
    use std::os::unix::ffi::OsStrExt;
    // SAFETY: the GTK service thread owns the nested event loop and every C
    // string until it returns. The library and its registered callbacks stay loaded.
    unsafe {
        let gtk = gtk_library()?;
        let init: libloading::Symbol<unsafe extern "C" fn(*mut i32, *mut *mut *mut c_char) -> i32> =
            gtk.get(b"gtk_init_check\0").map_err(|e| e.to_string())?;
        if init(std::ptr::null_mut(), std::ptr::null_mut()) == 0 {
            return Err("Cannot open the folder chooser on this display.".into());
        }
        let new: libloading::Symbol<
            unsafe extern "C" fn(
                *const c_char,
                *mut c_void,
                i32,
                *const c_char,
                *const c_char,
            ) -> *mut c_void,
        > = gtk
            .get(b"gtk_file_chooser_native_new\0")
            .map_err(|e| e.to_string())?;
        let run: libloading::Symbol<unsafe extern "C" fn(*mut c_void) -> i32> = gtk
            .get(b"gtk_native_dialog_run\0")
            .map_err(|e| e.to_string())?;
        let filename: libloading::Symbol<unsafe extern "C" fn(*mut c_void) -> *mut c_char> = gtk
            .get(b"gtk_file_chooser_get_filename\0")
            .map_err(|e| e.to_string())?;
        let folder: libloading::Symbol<unsafe extern "C" fn(*mut c_void, *const c_char) -> i32> = gtk
            .get(b"gtk_file_chooser_set_current_folder\0")
            .map_err(|e| e.to_string())?;
        let modal: libloading::Symbol<unsafe extern "C" fn(*mut c_void, i32)> = gtk
            .get(b"gtk_native_dialog_set_modal\0")
            .map_err(|e| e.to_string())?;
        let destroy: libloading::Symbol<unsafe extern "C" fn(*mut c_void)> = gtk
            .get(b"gtk_native_dialog_destroy\0")
            .map_err(|e| e.to_string())?;
        let free: libloading::Symbol<unsafe extern "C" fn(*mut c_void)> =
            gtk.get(b"g_free\0").map_err(|e| e.to_string())?;
        let unref: libloading::Symbol<unsafe extern "C" fn(*mut c_void)> =
            gtk.get(b"g_object_unref\0").map_err(|e| e.to_string())?;
        let title = CString::new(if import {
            "Import prepared game content"
        } else {
            TITLE
        })
        .unwrap();
        let dialog = new(
            title.as_ptr(),
            std::ptr::null_mut(),
            if import { 0 } else { 2 },
            c"Open".as_ptr(),
            c"Cancel".as_ptr(),
        );
        if dialog.is_null() {
            return Err("Cannot create the folder chooser.".into());
        }
        modal(dialog, 1);
        if let Some(home) = std::env::var_os("HOME") {
            if let Ok(home) = CString::new(home.as_bytes()) {
                folder(dialog, home.as_ptr());
            }
        }
        let result = if run(dialog) == -3 {
            let path = filename(dialog);
            if path.is_null() {
                None
            } else {
                let selected =
                    PathBuf::from(std::ffi::OsStr::from_bytes(CStr::from_ptr(path).to_bytes()));
                free(path.cast());
                Some(selected)
            }
        } else {
            None
        };
        destroy(dialog);
        unref(dialog);
        flush_dialog_changes(gtk)?;
        Ok(result)
    }
}

#[cfg(target_os = "windows")]
fn choose_native(import: bool) -> Result<Option<PathBuf>, String> {
    let script = r#"Add-Type -AssemblyName System.Windows.Forms; [Console]::OutputEncoding=[System.Text.UTF8Encoding]::new(); $d=New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description='Choose Star Wars Battlefront or prepared game data'; $d.ShowNewFolderButton=$false; if($d.ShowDialog() -eq 'OK'){[Console]::Write($d.SelectedPath)}"#;
    let script = if import {
        r#"Add-Type -AssemblyName System.Windows.Forms; [Console]::OutputEncoding=[System.Text.UTF8Encoding]::new(); $d=New-Object System.Windows.Forms.OpenFileDialog; $d.Title='Import prepared game content'; $d.Filter='Battlefront Eternal content export|*.bfepack;*.zip'; if($d.ShowDialog() -eq 'OK'){[Console]::Write($d.FileName)}"#
    } else {
        script
    };
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-STA", "-Command", script])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("Cannot open the folder chooser.".into());
    }
    let path = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    Ok((!path.is_empty()).then(|| PathBuf::from(path)))
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn choose_native(_: bool) -> Result<Option<PathBuf>, String> {
    Err("Folder selection is unavailable on this platform.".into())
}

#[cfg(target_os = "linux")]
fn warning_native(path: &std::path::Path) -> Result<(), String> {
    use std::ffi::{c_char, c_void, CString};
    // SAFETY: GTK owns the dialog for this nested event loop; both strings
    // remain alive until it returns and the widget is destroyed on this thread.
    unsafe {
        let gtk = gtk_library()?;
        let new: libloading::Symbol<
            unsafe extern "C" fn(*mut c_void, i32, i32, i32, *const c_char, ...) -> *mut c_void,
        > = gtk
            .get(b"gtk_message_dialog_new\0")
            .map_err(|e| e.to_string())?;
        let detail: libloading::Symbol<unsafe extern "C" fn(*mut c_void, *const c_char, ...)> = gtk
            .get(b"gtk_message_dialog_format_secondary_text\0")
            .map_err(|e| e.to_string())?;
        let run: libloading::Symbol<unsafe extern "C" fn(*mut c_void) -> i32> =
            gtk.get(b"gtk_dialog_run\0").map_err(|e| e.to_string())?;
        let destroy: libloading::Symbol<unsafe extern "C" fn(*mut c_void)> = gtk
            .get(b"gtk_widget_destroy\0")
            .map_err(|e| e.to_string())?;
        let message = CString::new(INVALID).unwrap();
        let folder = CString::new(path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        let dialog = new(
            std::ptr::null_mut(),
            1,
            1,
            1,
            c"%s".as_ptr(),
            message.as_ptr(),
        );
        if dialog.is_null() {
            return Err("Cannot create the folder warning.".into());
        }
        detail(dialog, c"%s".as_ptr(), folder.as_ptr());
        run(dialog);
        destroy(dialog);
        flush_dialog_changes(gtk)?;
        Ok(())
    }
}
#[cfg(target_os = "windows")]
fn warning_native(path: &std::path::Path) -> Result<(), String> {
    let detail = path.to_string_lossy().replace('\'', "''");
    let script=format!("Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.MessageBox]::Show('{}' + [Environment]::NewLine + '{}','Battlefront Eternal','OK','Warning')|Out-Null",INVALID,detail);
    std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-STA", "-Command", &script])
        .status()
        .map_err(|e| e.to_string())?;
    Ok(())
}
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn warning_native(_: &std::path::Path) -> Result<(), String> {
    Err(INVALID.into())
}

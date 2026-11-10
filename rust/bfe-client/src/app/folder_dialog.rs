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
/// What a chooser picks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A game install or prepared content folder.
    Folder,
    /// The archive to import (`showOpenDialog` in `runContentTransfer`).
    ImportFile,
    /// Where to save the export (`showSaveDialog` in `runContentTransfer`).
    ExportFile,
}

/// The archive name the browser build proposes for an export.
pub const EXPORT_NAME: &str = "Battlefront-Eternal-content.bfepack";

enum Request {
    Choose(Kind, Reply),
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
                    Request::Choose(kind, reply) => {
                        let _ = reply.send(choose_native(kind));
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
    request(|reply| Request::Choose(Kind::Folder, reply))
}
/// The open dialog for an archive to import: filtered to `.bfepack` and `.zip`.
pub fn import_file() -> Receiver<Result<Option<PathBuf>, String>> {
    request(|reply| Request::Choose(Kind::ImportFile, reply))
}
/// The save dialog for an export: proposes [`EXPORT_NAME`] in the downloads folder.
pub fn export_file() -> Receiver<Result<Option<PathBuf>, String>> {
    request(|reply| Request::Choose(Kind::ExportFile, reply))
}

/// The user's downloads folder (`app.getPath("downloads")`): `XDG_DOWNLOAD_DIR`, the
/// `user-dirs.dirs` entry, or `~/Downloads`.
fn downloads_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_DOWNLOAD_DIR") {
        return Some(PathBuf::from(dir));
    }
    let home = PathBuf::from(std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?);
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    if let Ok(text) = std::fs::read_to_string(config.join("user-dirs.dirs")) {
        for line in text.lines() {
            if let Some(value) = line.trim().strip_prefix("XDG_DOWNLOAD_DIR=") {
                let value = value
                    .trim()
                    .trim_matches('"')
                    .replace("$HOME", &home.to_string_lossy());
                return Some(PathBuf::from(value));
            }
        }
    }
    Some(home.join("Downloads"))
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

#[cfg(target_os = "linux")]
fn choose_native(kind: Kind) -> Result<Option<PathBuf>, String> {
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
        let folder: libloading::Symbol<unsafe extern "C" fn(*mut c_void, *const c_char) -> i32> =
            gtk.get(b"gtk_file_chooser_set_current_folder\0")
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
        let title = CString::new(match kind {
            Kind::Folder => TITLE,
            Kind::ImportFile => "Import prepared game content",
            Kind::ExportFile => "Export prepared game content",
        })
        .unwrap();
        // GTK_FILE_CHOOSER_ACTION_OPEN = 0, SAVE = 1, SELECT_FOLDER = 2.
        let dialog = new(
            title.as_ptr(),
            std::ptr::null_mut(),
            match kind {
                Kind::ImportFile => 0,
                Kind::ExportFile => 1,
                Kind::Folder => 2,
            },
            if kind == Kind::ExportFile {
                c"_Save"
            } else {
                c"_Open"
            }
            .as_ptr(),
            c"_Cancel".as_ptr(),
        );
        if dialog.is_null() {
            return Err("Cannot create the folder chooser.".into());
        }
        modal(dialog, 1);
        // The browser's filters: `.bfepack` for an export, `.bfepack` and `.zip` for an import.
        if kind != Kind::Folder {
            let new_filter: libloading::Symbol<unsafe extern "C" fn() -> *mut c_void> = gtk
                .get(b"gtk_file_filter_new\0")
                .map_err(|e| e.to_string())?;
            let set_name: libloading::Symbol<unsafe extern "C" fn(*mut c_void, *const c_char)> =
                gtk.get(b"gtk_file_filter_set_name\0")
                    .map_err(|e| e.to_string())?;
            let add_pattern: libloading::Symbol<unsafe extern "C" fn(*mut c_void, *const c_char)> =
                gtk.get(b"gtk_file_filter_add_pattern\0")
                    .map_err(|e| e.to_string())?;
            let add_filter: libloading::Symbol<unsafe extern "C" fn(*mut c_void, *mut c_void)> =
                gtk.get(b"gtk_file_chooser_add_filter\0")
                    .map_err(|e| e.to_string())?;
            let filter = new_filter();
            if kind == Kind::ExportFile {
                set_name(filter, c"Battlefront Eternal content".as_ptr());
                add_pattern(filter, c"*.bfepack".as_ptr());
            } else {
                set_name(filter, c"Battlefront Eternal content export".as_ptr());
                add_pattern(filter, c"*.bfepack".as_ptr());
                add_pattern(filter, c"*.zip".as_ptr());
            }
            add_filter(dialog, filter);
        }
        let start = if kind == Kind::ExportFile {
            downloads_dir().map(PathBuf::into_os_string)
        } else {
            std::env::var_os("HOME")
        };
        if let Some(start) = start {
            if let Ok(start) = CString::new(start.as_bytes()) {
                folder(dialog, start.as_ptr());
            }
        }
        if kind == Kind::ExportFile {
            let set_name: libloading::Symbol<unsafe extern "C" fn(*mut c_void, *const c_char)> =
                gtk.get(b"gtk_file_chooser_set_current_name\0")
                    .map_err(|e| e.to_string())?;
            let overwrite: libloading::Symbol<unsafe extern "C" fn(*mut c_void, i32)> = gtk
                .get(b"gtk_file_chooser_set_do_overwrite_confirmation\0")
                .map_err(|e| e.to_string())?;
            set_name(dialog, CString::new(EXPORT_NAME).unwrap().as_ptr());
            overwrite(dialog, 1);
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
fn choose_native(kind: Kind) -> Result<Option<PathBuf>, String> {
    let prelude = "Add-Type -AssemblyName System.Windows.Forms; [Console]::OutputEncoding=[System.Text.UTF8Encoding]::new(); ";
    let script = match kind {
        Kind::Folder => format!("{prelude}$d=New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description='Choose Star Wars Battlefront or prepared game data'; $d.ShowNewFolderButton=$false; if($d.ShowDialog() -eq 'OK'){{[Console]::Write($d.SelectedPath)}}"),
        Kind::ImportFile => format!("{prelude}$d=New-Object System.Windows.Forms.OpenFileDialog; $d.Title='Import prepared game content'; $d.Filter='Battlefront Eternal content export|*.bfepack;*.zip'; if($d.ShowDialog() -eq 'OK'){{[Console]::Write($d.FileName)}}"),
        Kind::ExportFile => {
            let start = downloads_dir().map(|d| d.to_string_lossy().replace('\'', "''")).unwrap_or_default();
            format!("{prelude}$d=New-Object System.Windows.Forms.SaveFileDialog; $d.Title='Export prepared game content'; $d.Filter='Battlefront Eternal content|*.bfepack'; $d.FileName='{EXPORT_NAME}'; $d.InitialDirectory='{start}'; $d.OverwritePrompt=$true; if($d.ShowDialog() -eq 'OK'){{[Console]::Write($d.FileName)}}")
        }
    };
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-STA", "-Command", &script])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("Cannot open the folder chooser.".into());
    }
    let path = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    Ok((!path.is_empty()).then(|| PathBuf::from(path)))
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn choose_native(_: Kind) -> Result<Option<PathBuf>, String> {
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

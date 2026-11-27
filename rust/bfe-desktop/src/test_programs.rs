//! Test programs that a test writes and then starts.
//!
//! A child process that another test starts holds a copy of every descriptor this process has open until it
//! runs its own program, and starting a file that some process holds open for writing fails with "text file
//! busy". So a test program is not written by this process: a shell that this function starts and waits for
//! writes it and makes it executable, and nothing here ever holds it open for writing.

use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

/// Writes `text` to `path` as an executable program (mode 755) through `/bin/sh`.
pub(crate) fn write(path: &Path, text: &[u8]) {
    let mut shell = Command::new("/bin/sh")
        .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .expect("start /bin/sh to write a test program");
    shell
        .stdin
        .take()
        .expect("the shell's input")
        .write_all(text)
        .expect("write the test program");
    let status = shell.wait().expect("wait for /bin/sh");
    assert!(
        status.success(),
        "writing {} failed: {status}",
        path.display()
    );
}

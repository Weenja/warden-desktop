//! Where the app listens for native hosts, and how a host connects. Local only, per user:
//!
//! - macOS: a Unix socket in the app's data folder (`~/Library/Application Support/<app id>`),
//!   readable and writable by this user only (mode 0600).
//! - Windows: a named pipe whose name holds the user's name. Remote clients are refused, and
//!   each side checks that the other end is this same Warden exe (see `server.rs`, `host.rs`).

use std::io;

/// The socket file's name inside the app data folder.
#[cfg(unix)]
const SOCKET_FILE: &str = "ext.sock";

/// The app data folder, the same one Tauri's `app_data_dir()` gives the app.
#[cfg(target_os = "macos")]
pub fn data_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(std::path::PathBuf::from(home).join("Library/Application Support").join(super::APP_ID))
}

#[cfg(target_os = "macos")]
pub fn socket_path() -> Option<std::path::PathBuf> {
    Some(data_dir()?.join(SOCKET_FILE))
}

/// `\\.\pipe\warden-staging-ext-<domain>-<user>`. Pipe names are machine-wide, so the user's
/// name keeps two people signed in on one PC apart.
#[cfg(windows)]
pub fn pipe_name() -> String {
    let part = |key: &str| std::env::var(key).unwrap_or_default().replace(['\\', '/'], "_");
    format!(r"\\.\pipe\warden-staging-ext-{}-{}", part("USERDOMAIN"), part("USERNAME"))
}

#[cfg(target_os = "macos")]
pub type Stream = tokio::net::UnixStream;
#[cfg(windows)]
pub type Stream = tokio::net::windows::named_pipe::NamedPipeClient;

/// Connect to the running app. Fails when the app is not running.
#[cfg(target_os = "macos")]
pub async fn connect() -> io::Result<Stream> {
    let path = socket_path().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no HOME"))?;
    tokio::net::UnixStream::connect(path).await
}

#[cfg(windows)]
pub async fn connect() -> io::Result<Stream> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Pipes::GetNamedPipeServerProcessId;

    let client = tokio::net::windows::named_pipe::ClientOptions::new().open(pipe_name())?;
    // Only talk to Warden itself: another program could have created a pipe with this name
    // first to read which sites are open.
    let mut server_pid = 0u32;
    unsafe { GetNamedPipeServerProcessId(HANDLE(client.as_raw_handle()), &mut server_pid) }
        .map_err(|e| io::Error::other(e.to_string()))?;
    if !crate::platform::is_this_exe(server_pid) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "the pipe is not Warden's"));
    }
    Ok(client)
}

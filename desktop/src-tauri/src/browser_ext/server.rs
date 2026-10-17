//! The app listens on the local endpoint for native hosts (see `endpoint.rs`) and turns each
//! connection into a [`Link`](super::link::Links). Anti-tamper: a connection is accepted only
//! from this same Warden exe (the host the browser started), and the browser it serves is read
//! from the host's parent processes here, not taken from anything the host says. A script that
//! connects by itself is refused; Warden started by hand from a terminal links with no browser,
//! which never changes what drains.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

use super::frame;
use super::link::Links;
use crate::platform;

/// After the endpoint fails (e.g. another program holds the pipe name), try again this often.
const RETRY_AFTER: Duration = Duration::from_secs(30);

/// Accept native hosts for as long as the app runs.
pub async fn serve(links: Arc<Links>) {
    loop {
        if let Err(e) = listen(&links).await {
            log::warn!("browser link endpoint: {e}");
        }
        tokio::time::sleep(RETRY_AFTER).await;
    }
}

#[cfg(target_os = "macos")]
async fn listen(links: &Arc<Links>) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let path = super::endpoint::socket_path().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no HOME"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // A socket file left by a crash would block bind. Only one Warden runs (single instance),
    // so an existing file is never another live app's.
    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path)?;
    // This user only: other accounts on the Mac cannot connect or read which sites are open.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    loop {
        let (stream, _) = listener.accept().await?;
        let Ok(cred) = stream.peer_cred() else { continue };
        let same_user = cred.uid() == unsafe { libc::getuid() };
        match cred.pid() {
            Some(pid) if same_user => accept(links, stream, pid as u32),
            _ => log::warn!("browser link: refused a connection from another user"),
        }
    }
}

#[cfg(windows)]
async fn listen(links: &Arc<Links>) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use tokio::net::windows::named_pipe::ServerOptions;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;

    let name = super::endpoint::pipe_name();
    // first_pipe_instance: fail if another program already made a pipe with this name, instead
    // of sharing it. Remote (network) clients are refused.
    let mut server = ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(&name)?;
    loop {
        server.connect().await?;
        let connected = server;
        server = ServerOptions::new().reject_remote_clients(true).create(&name)?;
        let mut pid = 0u32;
        match unsafe { GetNamedPipeClientProcessId(HANDLE(connected.as_raw_handle()), &mut pid) } {
            Ok(()) => accept(links, connected, pid),
            Err(e) => log::warn!("browser link: no client process id: {e}"),
        }
    }
}

/// Check who connected, then serve the link in the background.
fn accept<S>(links: &Arc<Links>, stream: S, pid: u32)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    if !platform::is_this_exe(pid) {
        log::warn!("browser link: refused a connection that is not Warden's native host");
        return;
    }
    let browser = platform::browser_ancestor(pid);
    log::info!("browser link: host connected for {}", browser.as_deref().unwrap_or("no known browser"));
    tokio::spawn(serve_link(links.clone(), stream, browser));
}

/// Relay frames: host -> [`Links::receive`], link outbox -> host. Ends when the host goes.
async fn serve_link<S>(links: Arc<Links>, stream: S, browser: Option<String>)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    let (id, mut outbox) = links.open(browser);
    let sender = tokio::spawn(async move {
        while let Some(message) = outbox.recv().await {
            let Ok(frame) = frame::encode(message.to_string().as_bytes()) else { continue };
            if writer.write_all(&frame).await.is_err() {
                break;
            }
        }
    });
    while let Ok(Some(body)) = frame::read(&mut reader).await {
        links.receive(id, &body);
    }
    links.close(id);
    sender.abort();
}

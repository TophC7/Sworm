//! Real HTTP/WS daemon regressions. The test binary relaunches itself so HOME/XDG
//! are scratch paths before Tokio, Host, or global settings are initialized.
use anyhow::{bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    net::{SocketAddr, TcpListener},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc,
    time::Duration,
};
use sworm_protocol::{
    lsp::LspEvent,
    rpc::{
        FileReadDown, LspDown, Open, PtyCursor, PtyDown, PtyUp, Reply, Request, Response,
        WireError, WorkbenchInfo, WorkbenchRun, MAX_FILE_CHUNK_BYTES, MAX_REQUEST_FRAME_BYTES,
    },
};
use sworm_server::{serve, ServeOptions, ServerHandle};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{sleep, timeout},
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};

const WAIT: Duration = Duration::from_secs(8);
const LSP_ID: &str = "dev.sworm.nix::nil";
type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Fixture {
    home: PathBuf,
    repo: PathBuf,
    handle: ServerHandle,
}

impl Fixture {
    async fn start(home: &Path) -> Result<Self> {
        let config = home.join("server-config");
        let data = home.join("server-data");
        let assets = home.join("assets");
        let repo = home.join("repo");
        for dir in [&config, &data, &assets, &repo, &repo.join(".sworm")] {
            fs::create_dir_all(dir)?;
        }
        fs::write(
            assets.join("index.html"),
            "<html>SWORM-WEB-INDEX-7751</html>",
        )?;
        fs::write(assets.join("app.js"), "window.swormAsset = 7751;")?;
        fs::write(
            config.join("server.toml"),
            format!(
                "[web]\nbind = \"127.0.0.1:0\"\nassets_dir = {:?}\n",
                assets.to_string_lossy().as_ref()
            ),
        )?;
        let status = Command::new("git")
            .args(["-c", "init.defaultBranch=main", "init"])
            .current_dir(&repo)
            .status()?;
        if !status.success() {
            bail!("git init failed: {status}");
        }
        fs::write(repo.join(".git/info/exclude"), ".sworm/\n")?;
        fs::write(
            repo.join(".sworm/settings.jsonc"),
            r#"{"providers":{"terminal":{"enabled":true,"binary_path_override":"sh","extra_args":[]}}}"#,
        )?;
        let handle = serve(ServeOptions {
            config_dir: config.clone(),
            data_dir: data,
            listen: Some("127.0.0.1:0".parse()?),
        })
        .await?;
        Ok(Self {
            home: home.into(),
            repo,
            handle,
        })
    }
    fn addr(&self) -> SocketAddr {
        self.handle.web_addr.expect("enabled web listener")
    }
    fn folder(&self) -> String {
        self.repo.to_string_lossy().into_owned()
    }
    fn origin(&self) -> String {
        format!("http://{}", self.addr())
    }
    fn db(&self) -> Result<rusqlite::Connection> {
        let db = rusqlite::Connection::open(self.home.join("server-data/server.db"))?;
        db.busy_timeout(Duration::from_secs(5))?;
        Ok(db)
    }
    /// Sends the exact first control frame and returns the daemon's answer.
    async fn hello(
        &self,
        workbench: &str,
        takeover: bool,
        token: Option<&str>,
    ) -> Result<(Socket, Value)> {
        let mut ws = connect(self.addr(), "/ws", Some(&self.origin())).await?;
        ws.send(Message::Text(
            json!({"hello":{"workbench_id":workbench,"takeover":takeover,"controller_token":token}})
                .to_string()
                .into(),
        ))
        .await?;
        let first = next_text(&mut ws).await.with_context(|| {
            format!("hello workbench={workbench:?} takeover={takeover} awaiting first frame")
        })?;
        Ok((ws, first))
    }
    async fn attach(
        &self,
        workbench: &str,
        takeover: bool,
        token: Option<&str>,
    ) -> Result<Attached> {
        let (ws, ready) = self.hello(workbench, takeover, token).await?;
        let connection = ready["ready"]["connection_id"]
            .as_str()
            .with_context(|| format!("workbench was not ready: {ready}"))?
            .to_owned();
        let token = ready["ready"]["controller_token"]
            .as_str()
            .with_context(|| format!("workbench ready lacked a controller token: {ready}"))?
            .to_owned();
        Ok(Attached {
            ws,
            connection,
            token,
        })
    }
    /// A fresh, empty workbench that only reaches the registry. Its page
    /// leaving prunes it; `release` waits for that so lists never count it.
    async fn observer(&self) -> Result<(Socket, String)> {
        let id = new_workbench();
        Ok((self.attach(&id, false, None).await?.ws, id))
    }
    async fn release(&self, mut observer: Socket, id: &str) -> Result<()> {
        observer.close(None).await?;
        self.wait_pruned(id).await
    }
    async fn wait_pruned(&self, id: &str) -> Result<()> {
        let db = self.db()?;
        timeout(WAIT, async {
            while manifest_ids(&db)?.iter().any(|stored| stored == id) {
                sleep(Duration::from_millis(20)).await;
            }
            Ok::<(), anyhow::Error>(())
        })
        .await
        .with_context(|| format!("workbench {id} was never pruned"))?
    }
    /// Other workbenches, as the Home screen of an empty one lists them.
    async fn list(&self) -> Result<Vec<WorkbenchInfo>> {
        let (mut observer, id) = self.observer().await?;
        let list = success(&mut observer, 1, Request::WorkbenchList {})
            .await?
            .workbench_list()
            .checked()?;
        self.release(observer, &id).await?;
        Ok(list.into_iter().filter(|info| info.id != id).collect())
    }
    async fn listed(&self, id: &str) -> Result<Option<WorkbenchInfo>> {
        Ok(self.list().await?.into_iter().find(|info| info.id == id))
    }
    async fn wait_listed(
        &self,
        id: &str,
        what: &str,
        done: impl Fn(Option<&WorkbenchInfo>) -> bool,
    ) -> Result<()> {
        timeout(WAIT, async {
            loop {
                if done(self.listed(id).await?.as_ref()) {
                    return Ok::<(), anyhow::Error>(());
                }
                sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .with_context(|| format!("workbench {id} never became {what}"))?
    }
    async fn wait_detached(&self, id: &str, running: usize) -> Result<()> {
        self.wait_listed(id, "detached", |info| {
            info.is_some_and(|info| !info.connected && info.running.len() == running)
        })
        .await
    }
    /// A fresh workbench with a fresh controller, for tests that only need
    /// ordinary shared-Host access.
    async fn control(&self) -> Result<(Socket, String)> {
        let attached = self.attach(&new_workbench(), false, None).await?;
        Ok((attached.ws, attached.connection))
    }
    async fn stream(&self, id: &str, open: Open) -> Result<Socket> {
        let mut ws = connect(
            self.addr(),
            &format!("/ws/stream?connection_id={id}"),
            Some(&format!("http://{}", self.addr())),
        )
        .await?;
        ws.send(Message::Binary(serde_json::to_vec(&open)?.into()))
            .await?;
        Ok(ws)
    }
}

async fn connect(addr: SocketAddr, route: &str, origin: Option<&str>) -> Result<Socket> {
    let mut request = format!("ws://{addr}{route}").into_client_request()?;
    if let Some(origin) = origin {
        request.headers_mut().insert("Origin", origin.parse()?);
    }
    Ok(timeout(WAIT, connect_async(request))
        .await
        .context("websocket handshake timed out")??
        .0)
}
async fn rejected(addr: SocketAddr, route: &str, origin: Option<&str>, code: u16) -> Result<()> {
    let mut request = format!("ws://{addr}{route}").into_client_request()?;
    if let Some(origin) = origin {
        request.headers_mut().insert("Origin", origin.parse()?);
    }
    let err = timeout(WAIT, connect_async(request))
        .await
        .context("websocket rejection timed out")?
        .expect_err("unexpected websocket upgrade");
    match err {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            assert_eq!(response.status().as_u16(), code)
        }
        other => bail!("expected HTTP {code}, got {other}"),
    }
    Ok(())
}
async fn wait_revoked(addr: SocketAddr, id: &str) -> Result<()> {
    let route = format!("/ws/stream?connection_id={id}");
    timeout(WAIT, async {
        loop {
            let mut request = format!("ws://{addr}{route}").into_client_request()?;
            request
                .headers_mut()
                .insert("Origin", format!("http://{addr}").parse()?);
            match connect_async(request).await {
                Err(tokio_tungstenite::tungstenite::Error::Http(response))
                    if response.status().as_u16() == 403 =>
                {
                    return Ok::<(), anyhow::Error>(())
                }
                Ok((mut socket, _)) => {
                    socket.close(None).await?;
                }
                Err(other) => bail!("revoked stream handshake failed unexpectedly: {other}"),
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .context("disconnected control retained stream admission")?
}
async fn next_message(ws: &mut Socket) -> Result<Message> {
    timeout(WAIT, async {
        loop {
            match ws.next().await.context("websocket ended without frame")?? {
                Message::Ping(_) | Message::Pong(_) => continue,
                message => return Ok(message),
            }
        }
    })
    .await
    .context("websocket frame timed out")?
}
async fn next_text(ws: &mut Socket) -> Result<Value> {
    loop {
        match next_message(ws).await? {
            Message::Text(text) => {
                let value: Value = serde_json::from_str(&text)?;
                if let Some(ping) = value.get("ping") {
                    ws.send(Message::Text(json!({"pong": ping}).to_string().into()))
                        .await?;
                } else {
                    return Ok(value);
                }
            }
            other => bail!("expected control text: {other:?}"),
        }
    }
}
async fn next_tagged<T: serde::de::DeserializeOwned>(ws: &mut Socket) -> Result<Tagged<T>> {
    match next_message(ws).await? {
        Message::Binary(bytes) if bytes.first() == Some(&0) => {
            Ok(Tagged::Json(serde_json::from_slice(&bytes[1..])?))
        }
        Message::Binary(bytes) if bytes.first() == Some(&1) => Ok(Tagged::Raw(bytes[1..].to_vec())),
        other => bail!("unexpected stream frame: {other:?}"),
    }
}
enum Tagged<T> {
    Json(T),
    Raw(Vec<u8>),
}
trait CheckedWire<T> {
    fn checked(self) -> Result<T>;
}
impl<T> CheckedWire<T> for std::result::Result<T, WireError> {
    fn checked(self) -> Result<T> {
        self.map_err(|error| anyhow::anyhow!("{error:?}"))
    }
}
async fn raw(ws: &mut Socket, bytes: &[u8]) -> Result<()> {
    let mut body = Vec::with_capacity(bytes.len() + 1);
    body.push(1);
    body.extend_from_slice(bytes);
    ws.send(Message::Binary(body.into())).await?;
    Ok(())
}
async fn next_reply(ws: &mut Socket) -> Result<Value> {
    loop {
        let value = next_text(ws).await?;
        if value.get("event").is_none() {
            return Ok(value);
        }
    }
}
async fn rpc(ws: &mut Socket, id: u64, request: Request) -> Result<Response> {
    ws.send(Message::Text(
        json!({"id":id,"request":request}).to_string().into(),
    ))
    .await?;
    let value = timeout(WAIT, next_reply(ws))
        .await
        .context("RPC timed out")??;
    assert_eq!(value["id"], id, "response correlation failed: {value}");
    Ok(serde_json::from_value(value["response"].clone())?)
}
async fn success(ws: &mut Socket, id: u64, request: Request) -> Result<Reply> {
    rpc(ws, id, request)
        .await?
        .map_err(|e| anyhow::anyhow!("RPC failed: {e:?}"))
}
async fn http(addr: SocketAddr, route: &str) -> Result<String> {
    let mut socket = TcpStream::connect(addr).await?;
    socket
        .write_all(
            format!("GET {route} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .await?;
    let mut response = Vec::new();
    timeout(WAIT, socket.read_to_end(&mut response)).await??;
    Ok(String::from_utf8(response)?)
}

/// A workbench controller admitted by `ready`.
struct Attached {
    ws: Socket,
    connection: String,
    token: String,
}

const MANIFEST_KEY: &str = "web_workbench_manifest";

/// Reads a retired control until the socket ends. Pings, events, and failed
/// replies may precede its terminal frame; nothing may follow it, no RPC
/// issued to it may succeed, and its close is a policy close.
async fn terminal(ws: &mut Socket) -> Result<Option<Value>> {
    timeout(WAIT, async {
        let mut terminal = None;
        loop {
            let message = match ws.next().await {
                Some(Ok(message)) => message,
                None | Some(Err(_)) => return Ok(terminal),
            };
            match message {
                Message::Text(text) => {
                    let value: Value = serde_json::from_str(&text)?;
                    if value.get("ping").is_some() {
                        continue;
                    }
                    if let Some(terminal) = &terminal {
                        bail!("control frame followed terminal {terminal}: {value}");
                    }
                    if ["busy", "revoked", "closed", "error"]
                        .iter()
                        .any(|key| value.get(key).is_some())
                    {
                        terminal = Some(value);
                    } else if value.get("response").is_some()
                        && serde_json::from_value::<Response>(value["response"].clone())?.is_ok()
                    {
                        bail!("retired control completed an RPC: {value}");
                    }
                }
                Message::Close(frame) => {
                    if let Some(frame) = frame {
                        assert_eq!(u16::from(frame.code), 1008, "terminal close code");
                    }
                    return Ok(terminal);
                }
                _ => {}
            }
        }
    })
    .await
    .context("retired control never closed")?
}
/// Revocation may be observed as its frame or as bare closure.
async fn retired(ws: &mut Socket, reason: &str) -> Result<()> {
    if let Some(frame) = terminal(ws).await? {
        assert_eq!(frame, json!({ reason: true }), "wrong retirement frame");
    }
    Ok(())
}
/// A hello answered by a terminal frame, followed only by a policy close.
async fn denied(
    f: &Fixture,
    workbench: &str,
    takeover: bool,
    token: Option<&str>,
) -> Result<Value> {
    let (mut ws, first) = f.hello(workbench, takeover, token).await?;
    assert!(first.get("ready").is_none(), "hello was admitted: {first}");
    assert_eq!(terminal(&mut ws).await?, None, "second terminal frame");
    Ok(first)
}
async fn invalid_id(f: &Fixture, workbench: &str) -> Result<()> {
    let frame = denied(f, workbench, false, None).await?;
    assert_eq!(frame["error"]["kind"], "invalid_argument", "{frame}");
    assert_eq!(frame["error"]["message"], "Invalid workbench id", "{frame}");
    Ok(())
}
/// The daemon must refuse this first control frame with a bare close.
async fn rejects_first_frame(f: &Fixture, first: Message, code: u16) -> Result<()> {
    let mut ws = connect(f.addr(), "/ws", Some(&f.origin())).await?;
    ws.send(first).await?;
    match next_message(&mut ws).await? {
        Message::Close(Some(frame)) => assert_eq!(u16::from(frame.code), code),
        other => bail!("invalid first control frame was answered with {other:?}"),
    }
    Ok(())
}
fn hello_text(value: Value) -> Message {
    Message::Text(value.to_string().into())
}
fn terminal_start(f: &Fixture, run: &str) -> Request {
    Request::SessionStart {
        run_id: run.into(),
        folder_path: f.folder(),
        provider_id: "terminal".into(),
        resume_token: None,
        cols: 80,
        rows: 24,
    }
}
fn task_start(f: &Fixture, run: &str, attach_only: bool) -> Request {
    Request::TasksStart {
        run_id: run.into(),
        folder_path: f.folder(),
        task_id: "once".into(),
        active_file_path: None,
        cols: 80,
        rows: 24,
        attach_only,
    }
}
fn new_run() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn new_workbench() -> String {
    uuid::Uuid::new_v4().to_string()
}
/// Silences echo, then reads the shell's PID from its own output.
async fn shell_pid(pty: &mut Socket, cursor: &mut PtyCursor) -> Result<u32> {
    raw(pty, b"stty -echo; printf '%s%s\\n' 'ECHO-' 'OFF-3310'\n").await?;
    pty_until(pty, cursor, b"ECHO-OFF-3310").await?;
    raw(pty, b"printf 'SHELL-PID-%s-END\\n' \"$$\"\n").await?;
    let output = pty_until(pty, cursor, b"-END").await?;
    Ok(String::from_utf8_lossy(&output)
        .split_once("SHELL-PID-")
        .context("PID marker missing")?
        .1
        .split_once("-END")
        .context("PID suffix missing")?
        .0
        .parse()?)
}
fn alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}
async fn wait_exit(pid: u32, what: &str) -> Result<()> {
    timeout(WAIT, async {
        while alive(pid) {
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .with_context(|| format!("{what} PID {pid} survived"))
}
async fn wait_file(path: &Path, expected: &[u8]) -> Result<()> {
    timeout(WAIT, async {
        while !fs::read(path).is_ok_and(|bytes| bytes == expected) {
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .with_context(|| format!("{} never held {:?}", path.display(), expected))
}
async fn event(ws: &mut Socket, kind: &str) -> Result<Value> {
    timeout(WAIT, async {
        loop {
            let frame = next_text(ws).await?;
            if frame["event"]["kind"] == kind {
                return Ok(frame["event"]["payload"].clone());
            }
        }
    })
    .await
    .context("event absent")?
}
async fn put_state(ws: &mut Socket, id: u64, key: &str, value: &str) -> Result<()> {
    success(
        ws,
        id,
        Request::AppStatePut {
            key: key.into(),
            value_json: value.into(),
        },
    )
    .await?
    .app_state_put()
    .checked()
}
async fn get_state(ws: &mut Socket, id: u64, key: &str) -> Result<Option<String>> {
    success(ws, id, Request::AppStateGet { key: key.into() })
        .await?
        .app_state_get()
        .checked()
}
fn stored(db: &rusqlite::Connection, key: &str) -> Result<Option<String>> {
    use rusqlite::OptionalExtension;
    Ok(db
        .query_row(
            "SELECT value_json FROM app_state WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()?)
}
fn manifest_ids(db: &rusqlite::Connection) -> Result<Vec<String>> {
    let Some(manifest) = stored(db, MANIFEST_KEY)? else {
        return Ok(Vec::new());
    };
    let records: Vec<Value> = serde_json::from_str(&manifest)?;
    records
        .iter()
        .map(|record| {
            record["id"]
                .as_str()
                .map(str::to_owned)
                .context("manifest record without id")
        })
        .collect()
}
fn v4(folders: &[&str], active: i64) -> String {
    json!({
        "version": 4,
        "activeTabIndex": active,
        "tabs": folders
            .iter()
            .map(|folder| json!({"folderPath": folder, "layout": {"kept": true}}))
            .collect::<Vec<_>>(),
    })
    .to_string()
}
/// A foreign or unavailable PTY attachment must end without output.
async fn pty_denied(ws: &mut Socket) -> Result<()> {
    match timeout(WAIT, ws.next())
        .await
        .context("denied PTY stream stayed open")?
    {
        Some(Ok(Message::Binary(bytes))) if bytes.first() == Some(&0) => {
            match serde_json::from_slice::<PtyDown>(&bytes[1..])? {
                PtyDown::Closed { .. } => Ok(()),
                other => bail!("foreign PTY stream delivered {other:?}"),
            }
        }
        None | Some(Ok(Message::Close(_))) | Some(Err(_)) => Ok(()),
        Some(Ok(other)) => bail!("foreign PTY stream delivered {other:?}"),
    }
}

async fn startup_and_policy(f: &Fixture) -> Result<()> {
    assert!(
        f.addr().ip().is_loopback(),
        "default web bind exposed a non-loopback address"
    );
    assert!(http(f.addr(), "/").await?.contains("SWORM-WEB-INDEX-7751"));
    assert!(http(f.addr(), "/app.js")
        .await?
        .contains("window.swormAsset = 7751"));
    assert!(http(f.addr(), "/workspace/deep/link")
        .await?
        .contains("SWORM-WEB-INDEX-7751"));
    for origin in [
        None,
        Some("null"),
        Some("https://foreign.example"),
        Some("not-an-origin"),
    ] {
        rejected(f.addr(), "/ws", origin, 403).await?;
        rejected(f.addr(), "/ws/stream?connection_id=unknown", origin, 403).await?;
    }
    let (mut owner, id) = f.control().await?;
    assert!(
        matches!(rpc(&mut owner, 30, Request::Pair { token:"unused".into(), name:"browser".into() }).await?, Err(WireError::InvalidArgument { message }) if message == "pairing is unavailable over web")
    );
    rejected(
        f.addr(),
        "/ws/stream?connection_id=unknown",
        Some(&format!("http://{}", f.addr())),
        403,
    )
    .await?;
    let mut control = f.control().await?.0;
    control
        .send(Message::Binary(b"binary-control".to_vec().into()))
        .await?;
    match next_message(&mut control).await? {
        Message::Close(Some(frame)) => assert_eq!(u16::from(frame.code), 1008),
        other => bail!("binary control did not close with 1008: {other:?}"),
    }
    for open in [Open::Events, Open::Rpc(Request::AppRuntimeInfo {})] {
        let mut ws = f.stream(&id, open).await?;
        match next_message(&mut ws).await? {
            Message::Close(Some(frame)) => assert_eq!(u16::from(frame.code), 1008),
            other => bail!("unsupported Open did not close with 1008: {other:?}"),
        }
    }
    let mut ws = connect(
        f.addr(),
        &format!("/ws/stream?connection_id={id}"),
        Some(&format!("http://{}", f.addr())),
    )
    .await?;
    ws.send(Message::Binary(vec![b'x'; 70 * 1024].into()))
        .await?;
    match next_message(&mut ws).await? {
        Message::Close(Some(frame)) => assert_eq!(u16::from(frame.code), 1009),
        other => bail!("oversized Open did not close with 1009: {other:?}"),
    }
    owner.close(None).await?;
    wait_revoked(f.addr(), &id).await?;
    Ok(())
}
struct StalledTask {
    path: PathBuf,
    release: Option<mpsc::Sender<()>>,
    writer: Option<std::thread::JoinHandle<std::io::Result<()>>>,
}

impl StalledTask {
    fn new(path: PathBuf) -> Result<(Self, tokio::sync::oneshot::Receiver<()>)> {
        let result = Command::new("mkfifo").arg(&path).status()?;
        if !result.success() {
            bail!("mkfifo failed: {result}");
        }
        let (opened, reached) = tokio::sync::oneshot::channel();
        let (release, resume) = mpsc::channel();
        let writer_path = path.clone();
        let writer = std::thread::spawn(move || {
            let mut fifo = OpenOptions::new().write(true).open(writer_path)?;
            let _ = opened.send(());
            let _ = resume.recv();
            fifo.write_all(br#"{"version":1,"tasks":[{"id":"once","label":"Once","command":"exec sh -c 'echo $$ >> delayed-pids; printf x >> delayed-counter; exec sleep 30'","singleton":true}]}"#)
        });
        Ok((
            Self {
                path,
                release: Some(release),
                writer: Some(writer),
            },
            reached,
        ))
    }
    fn unblock(&mut self) -> Result<()> {
        self.release.take();
        self.writer
            .take()
            .expect("task writer already joined")
            .join()
            .expect("task writer panicked")?;
        Ok(())
    }
}
impl Drop for StalledTask {
    fn drop(&mut self) {
        self.release.take();
        if let Some(writer) = self.writer.take() {
            let _ = OpenOptions::new().read(true).write(true).open(&self.path);
            let _ = writer.join();
        }
    }
}

async fn delayed_start_and_disconnect(f: &Fixture) -> Result<()> {
    let tasks = f.repo.join(".sworm/tasks.jsonc");
    let (mut stalled, reached) = StalledTask::new(tasks.clone())?;
    let workbench = new_workbench();
    let Attached {
        ws: mut control,
        token,
        ..
    } = f.attach(&workbench, false, None).await?;
    let request = |run: &str| Request::TasksStart {
        run_id: run.into(),
        folder_path: f.folder(),
        task_id: "once".into(),
        active_file_path: None,
        cols: 80,
        rows: 24,
        attach_only: false,
    };
    control
        .send(Message::Text(
            json!({"id":100,"request":request("web-delayed-a")})
                .to_string()
                .into(),
        ))
        .await?;
    timeout(WAIT, reached)
        .await
        .context("task start did not reach gated config read")??;
    control
        .send(Message::Text(
            json!({"id":101,"request":Request::AppStateGet { key:"web:opaque".into() }})
                .to_string()
                .into(),
        ))
        .await?;
    let quick = timeout(Duration::from_secs(2), next_reply(&mut control))
        .await
        .context("independent request blocked by start")??;
    assert_eq!(
        quick["id"], 101,
        "delayed start stole unrelated reply correlation: {quick}"
    );
    assert_eq!(
        serde_json::from_value::<Response>(quick["response"].clone())?
            .unwrap()
            .app_state_get()
            .checked()?
            .as_deref(),
        Some("{  \"opaque\": [1,  2], \"string\": \"keep spaces\"  }")
    );
    stalled.unblock()?;
    let started = next_reply(&mut control).await?;
    assert_eq!(started["id"], 100);
    serde_json::from_value::<Response>(started["response"].clone())?
        .unwrap()
        .tasks_start()
        .checked()?;
    timeout(WAIT, async {
        while !fs::read(f.repo.join("delayed-counter")).is_ok_and(|bytes| bytes == b"x") {
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("first delayed task did not execute exactly once")?;
    success(
        &mut control,
        102,
        Request::TasksStop {
            run_id: "web-delayed-a".into(),
        },
    )
    .await?
    .tasks_stop()
    .checked()?;
    fs::remove_file(&tasks)?;

    let (mut stalled, reached) = StalledTask::new(tasks.clone())?;
    control
        .send(Message::Text(
            json!({"id":103,"request":request("web-delayed-b")})
                .to_string()
                .into(),
        ))
        .await?;
    timeout(WAIT, reached)
        .await
        .context("disconnected start did not reach gated config read")??;
    control.close(None).await?;
    stalled.unblock()?;
    // Automatic reconnect of the same lease must own the start the old socket admitted.
    let mut replacement = f.attach(&workbench, false, Some(&token)).await?.ws;
    timeout(WAIT, async {
        loop {
            if run_status(&mut replacement, "web-delayed-b").await?
                && fs::read(f.repo.join("delayed-counter")).is_ok_and(|bytes| bytes.len() == 2)
            {
                return Ok::<(), anyhow::Error>(());
            }
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("disconnected start never published a retained run")??;
    success(
        &mut replacement,
        104,
        Request::TasksStop {
            run_id: "web-delayed-b".into(),
        },
    )
    .await?
    .tasks_stop()
    .checked()?;
    fs::remove_file(tasks)?;
    Ok(())
}

async fn events_and_correlation(f: &Fixture) -> Result<()> {
    let (mut first, _) = f.control().await?;
    let (mut second, _) = f.control().await?;
    let value = "{  \"opaque\": [1,  2], \"string\": \"keep spaces\"  }";
    success(
        &mut first,
        1,
        Request::AppStatePut {
            key: "web:opaque".into(),
            value_json: value.into(),
        },
    )
    .await?
    .app_state_put()
    .checked()?;
    assert_eq!(
        success(
            &mut second,
            2,
            Request::AppStateGet {
                key: "web:opaque".into()
            }
        )
        .await?
        .app_state_get()
        .checked()?
        .as_deref(),
        Some(value)
    );
    first
        .send(Message::Text(
            json!({"id":10,"request":Request::RecentFoldersList {}})
                .to_string()
                .into(),
        ))
        .await?;
    first
        .send(Message::Text(
            json!({"id":11,"request":Request::AppStateGet { key:"web:opaque".into() }})
                .to_string()
                .into(),
        ))
        .await?;
    let mut replies = std::collections::HashMap::new();
    while replies.len() < 2 {
        let response = next_text(&mut first).await?;
        if let Some(id) = response["id"].as_u64() {
            replies.insert(id, response["response"].clone());
        }
    }
    assert!(serde_json::from_value::<Response>(
        replies.remove(&10).context("missing recent reply")?
    )?
    .unwrap()
    .recent_folders_list()
    .checked()?
    .is_empty());
    assert_eq!(
        serde_json::from_value::<Response>(replies.remove(&11).context("missing state reply")?)?
            .unwrap()
            .app_state_get()
            .checked()?
            .as_deref(),
        Some(value)
    );
    let folder = f.folder();
    let other = f.home.join("other-repo");
    fs::create_dir_all(&other)?;
    success(
        &mut first,
        12,
        Request::FolderClaim {
            folder_path: folder.clone(),
        },
    )
    .await?
    .folder_claim()
    .checked()?;
    success(
        &mut second,
        13,
        Request::FolderClaim {
            folder_path: other.to_string_lossy().into_owned(),
        },
    )
    .await?
    .folder_claim()
    .checked()?;
    let (mut emitter, _) = f.control().await?;
    success(
        &mut emitter,
        14,
        Request::RecentFoldersTouch {
            path: folder.clone(),
        },
    )
    .await?
    .recent_folders_touch()
    .checked()?;
    assert!(event(&mut first, "recent_folders_changed")
        .await?
        .as_array()
        .context("recent event payload")?
        .iter()
        .any(|v| v["path"] == folder.as_str() && v["opened_at"].is_string()));
    assert!(event(&mut second, "recent_folders_changed")
        .await?
        .as_array()
        .context("recent event payload")?
        .iter()
        .any(|v| v["path"] == folder.as_str() && v["opened_at"].is_string()));
    // Folder-scoped notifications must not leak across independent claims.
    success(
        &mut first,
        15,
        Request::FilesWatchDirs {
            project_path: folder.clone(),
            dirs: vec![String::new()],
        },
    )
    .await?
    .files_watch_dirs()
    .checked()?;
    success(
        &mut second,
        16,
        Request::FilesWatchDirs {
            project_path: other.to_string_lossy().into_owned(),
            dirs: vec![String::new()],
        },
    )
    .await?
    .files_watch_dirs()
    .checked()?;
    fs::write(f.repo.join("web-watch.txt"), "changed")?;
    let changed = event(&mut first, "files_changed").await?;
    assert_eq!(changed["folder_path"], folder);
    let foreign = timeout(Duration::from_millis(500), async {
        loop {
            let frame = next_text(&mut second).await?;
            if frame["event"]["kind"] == "files_changed"
                && frame["event"]["payload"]["folder_path"] == folder
            {
                return Ok::<(), anyhow::Error>(());
            }
        }
    })
    .await;
    assert!(foreign.is_err(), "foreign control received folder event");
    fs::write(other.join("other-watch.txt"), "changed")?;
    let changed = event(&mut second, "files_changed").await?;
    assert_eq!(changed["folder_path"], other.to_string_lossy().as_ref());
    let foreign = timeout(Duration::from_millis(500), async {
        loop {
            let frame = next_text(&mut first).await?;
            if frame["event"]["kind"] == "files_changed"
                && frame["event"]["payload"]["folder_path"] == other.to_string_lossy().as_ref()
            {
                return Ok::<(), anyhow::Error>(());
            }
        }
    })
    .await;
    assert!(
        foreign.is_err(),
        "first control received second folder's event"
    );
    Ok(())
}

async fn pty_frame(ws: &mut Socket, cursor: &mut PtyCursor) -> Result<Vec<u8>> {
    match next_tagged::<PtyDown>(ws).await? {
        Tagged::Raw(bytes) => {
            if bytes.len() < 8 {
                bail!("short PTY offset");
            }
            let offset = u64::from_be_bytes(bytes[..8].try_into()?);
            assert_eq!(offset, cursor.output_offset, "PTY cursor discontinuity");
            cursor.output_offset += (bytes.len() - 8) as u64;
            Ok(bytes[8..].to_vec())
        }
        Tagged::Json(PtyDown::Event { sequence, .. }) => {
            cursor.event_sequence = cursor.event_sequence.max(sequence);
            Ok(Vec::new())
        }
        Tagged::Json(PtyDown::Gap { lost_bytes }) => {
            bail!("unexpected PTY history gap of {lost_bytes} bytes")
        }
        Tagged::Json(PtyDown::Closed { error }) => bail!("PTY attachment rejected: {error:?}"),
    }
}
async fn pty_until(ws: &mut Socket, cursor: &mut PtyCursor, marker: &[u8]) -> Result<Vec<u8>> {
    timeout(WAIT, async {
        let mut bytes = Vec::new();
        loop {
            bytes.extend(pty_frame(ws, cursor).await?);
            if bytes.windows(marker.len()).any(|part| part == marker) {
                return Ok(bytes);
            }
        }
    })
    .await
    .context("PTY output timed out")?
}
async fn run_status(ws: &mut Socket, run_id: &str) -> Result<bool> {
    Ok(success(
        ws,
        70,
        Request::RunStatus {
            run_id: run_id.into(),
        },
    )
    .await?
    .run_status()
    .checked()?
    .live)
}
async fn retained_terminal_and_task(f: &Fixture) -> Result<()> {
    let run = "web-terminal-retained";
    let workbench = new_workbench();
    let Attached {
        ws: mut control,
        connection: id,
        ..
    } = f.attach(&workbench, false, None).await?;
    success(
        &mut control,
        1,
        Request::SessionStart {
            run_id: run.into(),
            folder_path: f.folder(),
            provider_id: "terminal".into(),
            resume_token: None,
            cols: 80,
            rows: 24,
        },
    )
    .await?
    .session_start()
    .checked()?;
    let mut pty = f
        .stream(
            &id,
            Open::Pty {
                run_id: run.into(),
                cursor: PtyCursor::default(),
            },
        )
        .await?;
    let mut cursor = PtyCursor::default();
    raw(
        &mut pty,
        b"stty -echo; printf '%s%s\\n' 'WEB-ECHO-' 'OFF-9943'\n",
    )
    .await?;
    pty_until(&mut pty, &mut cursor, b"WEB-ECHO-OFF-9943").await?;
    raw(&mut pty, b"printf 'WEB-PID-%s-END\\n' \"$$\"\n").await?;
    let output = pty_until(&mut pty, &mut cursor, b"-END").await?;
    let text = String::from_utf8_lossy(&output);
    let pid: u32 = text
        .split_once("WEB-PID-")
        .context("PID marker missing")?
        .1
        .split_once("-END")
        .context("PID suffix missing")?
        .0
        .parse()?;
    control.close(None).await?;
    wait_revoked(f.addr(), &id).await?;
    let _ = timeout(WAIT, async {
        loop {
            if next_message(&mut pty).await.is_err() {
                break;
            }
        }
    })
    .await
    .context("old attachment was not detached")?;
    // A detached workbench keeps its live run and admits a fresh document.
    f.wait_detached(&workbench, 1).await?;
    let Attached {
        ws: mut replacement,
        connection: new_id,
        ..
    } = f.attach(&workbench, false, None).await?;
    assert!(
        run_status(&mut replacement, run).await?,
        "control loss killed retained PTY"
    );
    let mut resumed = f
        .stream(
            &new_id,
            Open::Pty {
                run_id: run.into(),
                cursor,
            },
        )
        .await?;
    raw(
        &mut resumed,
        b"printf 'WEB-RESUMED-%s-MARK-2188\\n' \"$$\"\n",
    )
    .await?;
    let output = pty_until(&mut resumed, &mut cursor, b"MARK-2188").await?;
    let text = String::from_utf8_lossy(&output);
    assert!(
        text.contains(&format!("WEB-RESUMED-{pid}-MARK-2188")),
        "PTY PID changed: {text}"
    );
    assert!(
        !text.contains("WEB-PID-"),
        "cursor replay duplicated consumed output"
    );
    success(
        &mut replacement,
        2,
        Request::SessionStop { run_id: run.into() },
    )
    .await?
    .session_stop()
    .checked()?;
    assert!(!run_status(&mut replacement, run).await?);
    timeout(WAIT, async {
        while PathBuf::from(format!("/proc/{pid}")).exists() {
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("explicit stop left terminal PID alive")?;

    fs::write(
        f.repo.join(".sworm/tasks.jsonc"),
        r#"{"version":1,"tasks":[{"id":"once","label":"Once","command":"printf x >> task-counter; sleep 30","singleton":true}]}"#,
    )?;
    let task = "web-task-once";
    let request = |run: &str, attach_only| Request::TasksStart {
        run_id: run.into(),
        folder_path: f.folder(),
        task_id: "once".into(),
        active_file_path: None,
        cols: 80,
        rows: 24,
        attach_only,
    };
    success(&mut replacement, 3, request(task, false))
        .await?
        .tasks_start()
        .checked()?;
    timeout(WAIT, async {
        loop {
            if fs::read(f.repo.join("task-counter")).is_ok_and(|bytes| bytes == b"x") {
                break;
            }
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await?;
    let mut task_stream = f
        .stream(
            &new_id,
            Open::Pty {
                run_id: task.into(),
                cursor: PtyCursor::default(),
            },
        )
        .await?;
    assert!(
        matches!(
            next_tagged::<PtyDown>(&mut task_stream).await?,
            Tagged::Json(PtyDown::Event { .. })
        ),
        "initial task attachment had no process event"
    );
    task_stream.close(None).await?;
    success(&mut replacement, 4, request(task, true))
        .await?
        .tasks_start()
        .checked()?;
    let mut reattached = f
        .stream(
            &new_id,
            Open::Pty {
                run_id: task.into(),
                cursor: PtyCursor::default(),
            },
        )
        .await?;
    assert!(
        matches!(
            next_tagged::<PtyDown>(&mut reattached).await?,
            Tagged::Json(PtyDown::Event { .. })
        ),
        "attach-only task lost its retained stream"
    );
    assert!(
        run_status(&mut replacement, task).await?,
        "attach-only replaced or lost live run"
    );
    assert_eq!(fs::read(f.repo.join("task-counter"))?, b"x");
    success(
        &mut replacement,
        5,
        Request::TasksStop {
            run_id: task.into(),
        },
    )
    .await?
    .tasks_stop()
    .checked()?;
    assert!(matches!(
        rpc(
            &mut replacement,
            6,
            request(&uuid::Uuid::new_v4().to_string(), true)
        )
        .await?,
        Err(WireError::NotFound { .. })
    ));
    assert_eq!(fs::read(f.repo.join("task-counter"))?, b"x");
    Ok(())
}

fn enable_lsp(repo: &Path) -> Result<()> {
    let script = repo.join("fake-lsp.sh");
    fs::write(
        &script,
        r#"#!/bin/sh
say() { printf 'Content-Length: %s\r\n\r\n%s' "${#1}" "$1"; }
say '{"jsonrpc":"2.0","method":"window/logMessage","params":{"type":3,"message":"fake-lsp-started"}}'
while IFS= read -r line; do
  case "$line" in *initialize*) say '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}' ;; esac
done
"#,
    )?;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
    fs::write(
        repo.join(".sworm/settings.jsonc"),
        format!(
            r#"{{"providers":{{"terminal":{{"enabled":true,"binary_path_override":"sh","extra_args":[]}}}},"lsp":{{"servers":{{"{LSP_ID}":{{"enabled":true,"binary_path_override":"{}"}}}}}}}}"#,
            script.display()
        ),
    )?;
    Ok(())
}
async fn lsp_event(ws: &mut Socket) -> Result<LspEvent> {
    match next_tagged::<LspDown>(ws).await? {
        Tagged::Json(LspDown::Event { event }) => Ok(event),
        _ => bail!("expected LSP event"),
    }
}
async fn lsp_ownership(f: &Fixture) -> Result<()> {
    enable_lsp(&f.repo)?;
    let workbench = new_workbench();
    let Attached {
        ws: mut owner,
        connection: id,
        token,
    } = f.attach(&workbench, false, None).await?;
    let (mut other, _) = f.control().await?;
    let pty_run = "web-lsp-surviving-pty";
    success(
        &mut owner,
        1,
        Request::SessionStart {
            run_id: pty_run.into(),
            folder_path: f.folder(),
            provider_id: "terminal".into(),
            resume_token: None,
            cols: 80,
            rows: 24,
        },
    )
    .await?
    .session_start()
    .checked()?;
    let session = "web-lsp-owned";
    let mut stream = f
        .stream(
            &id,
            Open::Lsp {
                session_id: session.into(),
            },
        )
        .await?;
    assert!(matches!(
        next_tagged::<LspDown>(&mut stream).await?,
        Tagged::Json(LspDown::Ready)
    ));
    let start = |session: &str| Request::LspStart {
        session_id: session.into(),
        folder_path: f.folder(),
        server_definition_id: LSP_ID.into(),
        root_path: f.folder(),
    };
    assert!(matches!(
        rpc(&mut other, 1, start(session)).await?,
        Err(WireError::InvalidArgument { .. })
    ));
    success(&mut owner, 2, start(session))
        .await?
        .lsp_start()
        .checked()?;
    let pid = match lsp_event(&mut stream).await? {
        LspEvent::Started { pid: Some(pid), .. } => pid,
        other => bail!("missing LSP PID: {other:?}"),
    };
    assert!(PathBuf::from(format!("/proc/{pid}")).exists());
    owner.close(None).await?;
    timeout(WAIT, async {
        while PathBuf::from(format!("/proc/{pid}")).exists() {
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("LSP survived control loss")?;
    let Attached {
        ws: mut replacement,
        connection: new_id,
        ..
    } = f.attach(&workbench, false, Some(&token)).await?;
    assert!(
        run_status(&mut replacement, pty_run).await?,
        "LSP teardown killed simultaneously running PTY"
    );
    let mut fresh = f
        .stream(
            &new_id,
            Open::Lsp {
                session_id: session.into(),
            },
        )
        .await?;
    assert!(matches!(
        next_tagged::<LspDown>(&mut fresh).await?,
        Tagged::Json(LspDown::Ready)
    ));
    success(&mut replacement, 3, start(session))
        .await?
        .lsp_start()
        .checked()?;
    let new_pid = match lsp_event(&mut fresh).await? {
        LspEvent::Started { pid: Some(pid), .. } => pid,
        other => bail!("replacement LSP PID missing: {other:?}"),
    };
    drop(stream);
    sleep(Duration::from_millis(100)).await;
    assert!(
        PathBuf::from(format!("/proc/{new_pid}")).exists(),
        "stale lease killed replacement"
    );
    success(
        &mut replacement,
        4,
        Request::LspStop {
            session_id: session.into(),
        },
    )
    .await?
    .lsp_stop()
    .checked()?;
    timeout(WAIT, async {
        while PathBuf::from(format!("/proc/{new_pid}")).exists() {
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("replacement LSP survived stop")?;
    success(
        &mut replacement,
        5,
        Request::SessionStop {
            run_id: pty_run.into(),
        },
    )
    .await?
    .session_stop()
    .checked()?;
    Ok(())
}

fn process_read_chars() -> Result<u64> {
    let io = fs::read_to_string("/proc/self/io")?;
    let value = io
        .lines()
        .find_map(|line| line.strip_prefix("rchar:"))
        .context("Linux process read counter missing")?;
    Ok(value.trim().parse()?)
}
async fn file_integrity_and_shutdown(f: Fixture) -> Result<()> {
    let (mut control, id) = f.control().await?;
    let file = f.repo.join("large.bin");
    let bytes = (0..(3 * 1024 * 1024 + 13))
        .map(|i| (i % 251) as u8)
        .collect::<Vec<_>>();
    fs::write(&file, &bytes)?;
    let version = success(
        &mut control,
        1,
        Request::FileStat {
            project_path: f.folder(),
            file_path: "large.bin".into(),
        },
    )
    .await?
    .file_stat()
    .checked()?
    .version;
    let mut stream = f
        .stream(
            &id,
            Open::FileRead {
                project_path: f.folder(),
                file_path: "large.bin".into(),
                version: version.clone(),
            },
        )
        .await?;
    let mut received = Vec::new();
    let hash = format!("{:x}", Sha256::digest(&bytes));
    loop {
        match next_tagged::<FileReadDown>(&mut stream).await? {
            Tagged::Raw(chunk) => {
                assert!(chunk.len() <= MAX_FILE_CHUNK_BYTES);
                received.extend(chunk);
            }
            Tagged::Json(FileReadDown::Complete { version }) => {
                assert_eq!(version, hash);
                break;
            }
            Tagged::Json(FileReadDown::Error { error }) => bail!("file stream failed: {error:?}"),
        }
    }
    assert_eq!(received, bytes);
    fs::write(&file, "modified")?;
    let mut stale = f
        .stream(
            &id,
            Open::FileRead {
                project_path: f.folder(),
                file_path: "large.bin".into(),
                version,
            },
        )
        .await?;
    assert!(matches!(
        next_tagged::<FileReadDown>(&mut stale).await?,
        Tagged::Json(FileReadDown::Error {
            error: WireError::Conflict { .. }
        })
    ));
    fs::write(f.home.join("outside.txt"), b"outside-sentinel")?;
    let mut escaped = f
        .stream(
            &id,
            Open::FileRead {
                project_path: f.folder(),
                file_path: "../outside.txt".into(),
                version: format!("{:x}", Sha256::digest(b"outside-sentinel")),
            },
        )
        .await?;
    assert!(matches!(
        next_tagged::<FileReadDown>(&mut escaped).await?,
        Tagged::Json(FileReadDown::Error {
            error: WireError::InvalidArgument { .. } | WireError::NotFound { .. }
        })
    ));
    fs::File::create(&file)?.set_len(128 * 1024 * 1024)?;
    let version = success(
        &mut control,
        2,
        Request::FileStat {
            project_path: f.folder(),
            file_path: "large.bin".into(),
        },
    )
    .await?
    .file_stat()
    .checked()?
    .version;
    let read_before_cancel = process_read_chars()?;
    let mut cancelled = f
        .stream(
            &id,
            Open::FileRead {
                project_path: f.folder(),
                file_path: "large.bin".into(),
                version: version.clone(),
            },
        )
        .await?;
    assert!(matches!(
        next_tagged::<FileReadDown>(&mut cancelled).await?,
        Tagged::Raw(_)
    ));
    cancelled.close(None).await?;
    let _ = timeout(WAIT, cancelled.next())
        .await
        .context("file producer did not notice close")?;
    sleep(Duration::from_millis(500)).await;
    assert!(
        process_read_chars()?.saturating_sub(read_before_cancel) < 64 * 1024 * 1024,
        "closed file stream continued reading the full 128 MiB file"
    );
    let run_id = "web-shutdown-terminal";
    success(
        &mut control,
        3,
        Request::SessionStart {
            run_id: run_id.into(),
            folder_path: f.folder(),
            provider_id: "terminal".into(),
            resume_token: None,
            cols: 80,
            rows: 24,
        },
    )
    .await?
    .session_start()
    .checked()?;
    let mut active = f
        .stream(
            &id,
            Open::Pty {
                run_id: run_id.into(),
                cursor: PtyCursor::default(),
            },
        )
        .await?;
    let mut cursor = PtyCursor::default();
    raw(
        &mut active,
        b"stty -echo; printf '%s%s\\n' 'SHUTDOWN-ECHO-' 'OFF-7419'\n",
    )
    .await?;
    pty_until(&mut active, &mut cursor, b"SHUTDOWN-ECHO-OFF-7419").await?;
    raw(&mut active, b"printf 'SHUTDOWN-PID-%s-END\\n' \"$$\"\n").await?;
    let output = pty_until(&mut active, &mut cursor, b"-END").await?;
    let pid: u32 = String::from_utf8_lossy(&output)
        .split_once("SHUTDOWN-PID-")
        .context("shutdown PTY PID absent")?
        .1
        .split_once("-END")
        .context("shutdown PID terminator absent")?
        .0
        .parse()?;
    let mut stalled = f
        .stream(
            &id,
            Open::FileRead {
                project_path: f.folder(),
                file_path: "large.bin".into(),
                version,
            },
        )
        .await?;
    assert!(matches!(
        next_tagged::<FileReadDown>(&mut stalled).await?,
        Tagged::Raw(_)
    ));
    // Stop draining a 128 MiB stream so shutdown interrupts socket backpressure.
    sleep(Duration::from_millis(500)).await;
    timeout(WAIT, f.handle.shutdown())
        .await
        .context("shutdown held open WS upgrade tasks")?;
    timeout(WAIT, async {
        while PathBuf::from(format!("/proc/{pid}")).exists() {
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("shutdown left PTY process alive")?;
    let _ = timeout(WAIT, next_message(&mut active)).await;
    Ok(())
}

async fn heartbeat_liveness(f: &Fixture) -> Result<()> {
    let (mut healthy, _) = f.control().await?;
    let (mut silent, id) = f.control().await?;
    timeout(Duration::from_secs(45), async {
        tokio::try_join!(
            async {
                let mut acknowledged = 0;
                while acknowledged < 4 {
                    let message = healthy.next().await.context("healthy control closed")??;
                    if let Message::Text(text) = message {
                        let value: Value = serde_json::from_str(&text)?;
                        if let Some(ping) = value.get("ping") {
                            healthy
                                .send(Message::Text(json!({"pong": ping}).to_string().into()))
                                .await?;
                            acknowledged += 1;
                        }
                    }
                }
                Ok::<_, anyhow::Error>(())
            },
            async {
                while let Some(message) = silent.next().await {
                    if matches!(message?, Message::Close(_)) {
                        break;
                    }
                }
                Ok::<_, anyhow::Error>(())
            },
        )?;
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("unacknowledged heartbeat did not expire within its deadline")??;
    wait_revoked(f.addr(), &id).await?;
    success(&mut healthy, 1, Request::RecentFoldersList {}).await?;
    healthy.close(None).await?;
    Ok(())
}

fn request_frame(id: u64, request: Request) -> Message {
    Message::Text(json!({"id":id,"request":request}).to_string().into())
}
/// Whether this control may see `run_id` live; foreign or unknown runs read as not live.
async fn live_for(ws: &mut Socket, run_id: &str) -> Result<bool> {
    Ok(matches!(
        rpc(ws, 71, Request::RunStatus { run_id: run_id.into() })
            .await?
            .map(|reply| reply.run_status()),
        Ok(Ok(status)) if status.live
    ))
}
async fn claim_and_watch(ws: &mut Socket, folder: &str) -> Result<()> {
    success(
        ws,
        60,
        Request::FolderClaim {
            folder_path: folder.into(),
        },
    )
    .await?
    .folder_claim()
    .checked()?;
    success(
        ws,
        61,
        Request::FilesWatchDirs {
            project_path: folder.into(),
            dirs: vec![String::new()],
        },
    )
    .await?
    .files_watch_dirs()
    .checked()
}
async fn close_workbench(ws: &mut Socket, id: u64, workbench: &str) -> Result<()> {
    success(
        ws,
        id,
        Request::WorkbenchClose {
            id: workbench.into(),
        },
    )
    .await?
    .workbench_close()
    .checked()
}
/// Waits until `id`'s detach decided: pruned (`None`) or kept with a newer
/// last-seen than `seen`.
async fn detached_after(f: &Fixture, id: &str, seen: &str) -> Result<Option<WorkbenchInfo>> {
    timeout(WAIT, async {
        loop {
            match f.listed(id).await? {
                Some(info) if info.connected || info.last_seen_at == seen => {}
                settled => return Ok::<_, anyhow::Error>(settled),
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .with_context(|| format!("workbench {id} never finished detaching"))?
}

async fn handshake_modes(f: &Fixture) -> Result<()> {
    let probe = "web:unadmitted-probe";
    let put_probe = || Request::AppStatePut {
        key: probe.into(),
        value_json: "1".into(),
    };
    // Control frames before a valid hello never reach dispatch.
    rejects_first_frame(f, request_frame(1, put_probe()), 1008).await?;
    let unused = new_workbench();
    for bad in [
        // Every page names its workbench.
        json!({"hello":{"takeover":false,"controller_token":null}}),
        json!({"hello":{"workbench_id":null,"takeover":false,"controller_token":null}}),
        json!({"hello":{"workbench_id":unused,"takeover":false}}),
        json!({"hello":{"workbench_id":unused,"takeover":false,"controller_token":null,"extra":1}}),
        json!({"hello":{"workbench_id":unused,"takeover":false,"controller_token":null},"id":1}),
        json!({"hello":{"workbench_id":7,"takeover":false,"controller_token":null}}),
        json!({"hello":{"workbench_id":unused,"takeover":"no","controller_token":null}}),
        json!({"hello":null}),
    ] {
        rejects_first_frame(f, hello_text(bad), 1008).await?;
    }
    rejects_first_frame(f, Message::Text("not json".into()), 1008).await?;
    let pinged = new_workbench();
    let valid_hello =
        json!({"hello":{"workbench_id":pinged,"takeover":false,"controller_token":null}});
    rejects_first_frame(
        f,
        Message::Binary(valid_hello.to_string().into_bytes().into()),
        1008,
    )
    .await?;
    rejects_first_frame(
        f,
        hello_text(json!({"hello":{
            "workbench_id":"x".repeat(MAX_REQUEST_FRAME_BYTES),
            "takeover":false,
            "controller_token":null
        }})),
        1009,
    )
    .await?;
    // Pings may precede the hello; a second hello is a protocol violation.
    let mut ws = connect(f.addr(), "/ws", Some(&f.origin())).await?;
    ws.send(Message::Ping(b"before-hello".to_vec().into()))
        .await?;
    ws.send(hello_text(valid_hello.clone())).await?;
    assert!(
        next_text(&mut ws).await?["ready"]["connection_id"].is_string(),
        "ping before hello broke the handshake"
    );
    ws.send(hello_text(valid_hello)).await?;
    match next_message(&mut ws).await? {
        Message::Close(Some(frame)) => assert_eq!(u16::from(frame.code), 1008),
        other => bail!("duplicate hello was accepted: {other:?}"),
    }
    f.wait_pruned(&pinged).await?;

    // Ids are 1..=64 of [A-Za-z0-9_-]; anything else creates nothing.
    for bad in [
        String::new(),
        "x".repeat(65),
        "a/b".into(),
        "a.b".into(),
        "a b".into(),
        "é".into(),
        "workbench:a".into(),
    ] {
        invalid_id(f, &bad).await?;
    }
    let longest = "A-_9".repeat(16);
    let edge = f.attach(&longest, false, None).await?.ws;
    f.release(edge, &longest).await?;

    // A valid unknown id is created by its first hello and lists as connected.
    let workbench = new_workbench();
    let mut control = f.attach(&workbench, false, None).await?;
    let listed = success(&mut control.ws, 12, Request::WorkbenchList {})
        .await?
        .workbench_list()
        .checked()?;
    assert!(listed
        .iter()
        .any(|info| info.id == workbench && info.connected));
    match rpc(
        &mut control.ws,
        11,
        Request::WorkbenchClose {
            id: workbench.clone(),
        },
    )
    .await?
    {
        Err(WireError::InvalidArgument { message }) => {
            assert_eq!(message, "A workbench cannot close itself")
        }
        other => bail!("workbench closed itself: {other:?}"),
    }
    assert_eq!(
        get_state(&mut control.ws, 13, probe).await?,
        None,
        "unadmitted control wrote Host state"
    );
    // Any workbench may close another.
    let other = new_workbench();
    let mut victim = f.attach(&other, false, None).await?;
    close_workbench(&mut control.ws, 14, &other).await?;
    retired(&mut victim.ws, "closed").await?;
    assert!(f.listed(&other).await?.is_none());
    control.ws.close(None).await?;
    f.wait_pruned(&workbench).await?;
    Ok(())
}

/// Workbenches exist while a page or its state needs them: an empty one is
/// pruned when its page leaves, and a recreated id starts a new incarnation.
async fn workbench_lifecycle(f: &Fixture) -> Result<()> {
    let id = new_workbench();
    let mut first = f.attach(&id, false, None).await?;
    let created = f
        .listed(&id)
        .await?
        .context("attached workbench not listed")?;
    first.ws.close(None).await?;
    f.wait_pruned(&id).await?;

    // The same link recreates it, even from the page that still holds the old
    // lease after a network drop; its runs work and a live one keeps it.
    let mut second = f.attach(&id, false, Some(&first.token)).await?;
    assert_ne!(second.token, first.token, "pruned lease resumed");
    let recreated = f
        .listed(&id)
        .await?
        .context("recreated workbench not listed")?;
    assert_ne!(
        recreated.created_at, created.created_at,
        "pruned record survived"
    );
    let run = new_run();
    success(&mut second.ws, 1, terminal_start(f, &run))
        .await?
        .session_start()
        .checked()?;
    let seen = f
        .listed(&id)
        .await?
        .context("workbench missing")?
        .last_seen_at;
    second.ws.close(None).await?;
    let kept = detached_after(f, &id, &seen)
        .await?
        .context("workbench with a live run was pruned")?;
    assert_eq!(kept.running.len(), 1, "{kept:?}");

    // Close then reopen: the new incarnation's runs are not refused by the
    // closed owner, and Close ended the old run.
    let (mut observer, observer_id) = f.observer().await?;
    close_workbench(&mut observer, 1, &id).await?;
    let mut third = f.attach(&id, false, None).await?;
    assert!(
        !live_for(&mut third.ws, &run).await?,
        "Close left its run live"
    );
    let run = new_run();
    success(&mut third.ws, 2, terminal_start(f, &run))
        .await?
        .session_start()
        .checked()?;
    assert!(run_status(&mut third.ws, &run).await?);
    success(
        &mut third.ws,
        3,
        Request::SessionStop {
            run_id: run.clone(),
        },
    )
    .await?
    .session_stop()
    .checked()?;

    // A tab keeps an otherwise empty workbench.
    put_state(
        &mut third.ws,
        4,
        &format!("workbench:{id}"),
        &v4(&[&f.folder()], 0),
    )
    .await?;
    let seen = f
        .listed(&id)
        .await?
        .context("workbench missing")?
        .last_seen_at;
    third.ws.close(None).await?;
    detached_after(f, &id, &seen)
        .await?
        .context("workbench with a tab was pruned")?;
    close_workbench(&mut observer, 2, &id).await?;

    // Two pages closing each other both close.
    let (a, b) = (new_workbench(), new_workbench());
    let mut a1 = f.attach(&a, false, None).await?;
    let mut b1 = f.attach(&b, false, None).await?;
    let (sent_a, sent_b) = tokio::join!(
        a1.ws
            .send(request_frame(1, Request::WorkbenchClose { id: b.clone() })),
        b1.ws
            .send(request_frame(1, Request::WorkbenchClose { id: a.clone() })),
    );
    sent_a?;
    sent_b?;
    retired(&mut a1.ws, "closed").await?;
    retired(&mut b1.ws, "closed").await?;
    for id in [&a, &b] {
        f.wait_listed(id, "closed", |info| info.is_none())
            .await
            .context("mutual Close deadlocked")?;
    }
    f.release(observer, &observer_id).await?;
    Ok(())
}

async fn takeover_and_leases(f: &Fixture) -> Result<()> {
    let counter = f.repo.join("takeover-counter");
    fs::write(
        f.repo.join(".sworm/tasks.jsonc"),
        r#"{"version":1,"tasks":[{"id":"once","label":"Once","command":"printf x >> takeover-counter; exec sleep 60","singleton":true}]}"#,
    )?;
    let folder = f.folder();
    let a = new_workbench();
    let b = new_workbench();
    let mut a1 = f.attach(&a, false, None).await?;
    let mut b1 = f.attach(&b, false, None).await?;
    let key = format!("workbench:{}", a);
    let snapshot = v4(&[&folder], 0);
    claim_and_watch(&mut a1.ws, &folder).await?;
    put_state(&mut a1.ws, 1, &key, &snapshot).await?;
    let (terminal_run, task_run) = (new_run(), new_run());
    success(&mut a1.ws, 2, terminal_start(f, &terminal_run))
        .await?
        .session_start()
        .checked()?;
    success(&mut a1.ws, 3, task_start(f, &task_run, false))
        .await?
        .tasks_start()
        .checked()?;
    wait_file(&counter, b"x").await?;
    let mut old_pty = f
        .stream(
            &a1.connection,
            Open::Pty {
                run_id: terminal_run.clone(),
                cursor: PtyCursor::default(),
            },
        )
        .await?;
    let mut cursor = PtyCursor::default();
    let pid = shell_pid(&mut old_pty, &mut cursor).await?;
    let b_run = new_run();
    success(&mut b1.ws, 1, terminal_start(f, &b_run))
        .await?
        .session_start()
        .checked()?;
    let mut b_pty = f
        .stream(
            &b1.connection,
            Open::Pty {
                run_id: b_run.clone(),
                cursor: PtyCursor::default(),
            },
        )
        .await?;
    let mut b_cursor = PtyCursor::default();
    let b_pid = shell_pid(&mut b_pty, &mut b_cursor).await?;

    // A second fresh page is busy and leaves the controller untouched.
    assert_eq!(denied(f, &a, false, None).await?, json!({"busy":true}));
    raw(&mut old_pty, b"printf '%s\\n' 'STILL-A-5120'\n").await?;
    pty_until(&mut old_pty, &mut cursor, b"STILL-A-5120").await?;
    assert!(run_status(&mut a1.ws, &terminal_run).await?);

    let mut a2 = f.attach(&a, true, None).await?;
    assert_ne!(a2.token, a1.token, "takeover reused the loser's lease");
    // Everything the loser sends after the replacement is ready is inert.
    let late_run = new_run();
    for (id, request) in [
        (
            20,
            Request::FileWrite {
                project_path: folder.clone(),
                file_path: "late-write.txt".into(),
                content: "late".into(),
                expected_version: None,
            },
        ),
        (
            21,
            Request::AppStatePut {
                key: key.clone(),
                value_json: v4(&["/tmp/late"], 0),
            },
        ),
        (22, terminal_start(f, &late_run)),
    ] {
        let _ = a1.ws.send(request_frame(id, request)).await;
    }
    let _ = raw(&mut old_pty, b"touch old-input-4471\n").await;
    let mut resize = vec![0];
    resize.extend(serde_json::to_vec(&PtyUp::Resize { cols: 33, rows: 11 })?);
    let _ = old_pty.send(Message::Binary(resize.into())).await;
    retired(&mut a1.ws, "revoked").await?;
    rejected(
        f.addr(),
        &format!("/ws/stream?connection_id={}", a1.connection),
        Some(&f.origin()),
        403,
    )
    .await?;
    timeout(WAIT, async {
        while let Some(Ok(message)) = old_pty.next().await {
            if matches!(message, Message::Close(_)) {
                break;
            }
        }
    })
    .await
    .context("revoked PTY stream stayed open")?;

    // The replacement restores the same runs, processes, and saved state.
    assert_eq!(
        get_state(&mut a2.ws, 1, &key).await?.as_deref(),
        Some(snapshot.as_str())
    );
    claim_and_watch(&mut a2.ws, &folder).await?;
    success(&mut a2.ws, 2, task_start(f, &task_run, true))
        .await?
        .tasks_start()
        .checked()?;
    assert!(run_status(&mut a2.ws, &terminal_run).await?);
    assert!(run_status(&mut a2.ws, &task_run).await?);
    assert!(!live_for(&mut a2.ws, &late_run).await?, "late start ran");
    let mut new_pty = f
        .stream(
            &a2.connection,
            Open::Pty {
                run_id: terminal_run.clone(),
                cursor,
            },
        )
        .await?;
    raw(
        &mut new_pty,
        b"printf 'TAKEN-%s-%s-END\\n' \"$$\" \"$(stty size)\"\n",
    )
    .await?;
    let text =
        String::from_utf8_lossy(&pty_until(&mut new_pty, &mut cursor, b"-END").await?).into_owned();
    assert!(
        text.contains(&format!("TAKEN-{pid}-")),
        "PID changed: {text}"
    );
    assert!(
        !text.contains("11 33"),
        "revoked stream resized PTY: {text}"
    );
    assert!(!f.repo.join("old-input-4471").exists(), "revoked input ran");
    assert!(!f.repo.join("late-write.txt").exists(), "late write ran");
    assert_eq!(fs::read(&counter)?, b"x", "takeover re-ran the task");
    // The loser's late disconnect cannot clear the winner's status or claims.
    drop(a1.ws);
    sleep(Duration::from_millis(200)).await;
    let info = f.listed(&a).await?.context("A vanished")?;
    assert!(info.connected && info.running.len() == 2, "{info:?}");
    fs::write(f.repo.join("takeover-watch.txt"), "changed")?;
    assert_eq!(
        event(&mut a2.ws, "files_changed").await?["folder_path"],
        folder
    );
    // B stayed usable throughout.
    assert!(run_status(&mut b1.ws, &b_run).await?);
    raw(&mut b_pty, b"printf '%s\\n' 'B-STILL-7310'\n").await?;
    pty_until(&mut b_pty, &mut b_cursor, b"B-STILL-7310").await?;
    assert!(alive(b_pid));

    // Missed revocation: the stale token stays revoked with no active controller.
    a2.ws.close(None).await?;
    drop(new_pty);
    f.wait_detached(&a, 2).await?;
    for takeover in [false, true] {
        assert_eq!(
            denied(f, &a, takeover, Some(&a1.token)).await?,
            json!({"revoked":true}),
            "stale token reclaimed A (takeover={takeover})"
        );
    }
    // The legitimate last page resumes its lease, replacing a half-open socket without busy.
    let a3 = f.attach(&a, false, Some(&a2.token)).await?;
    assert_eq!(a3.token, a2.token);
    let mut a4 = f.attach(&a, false, Some(&a2.token)).await?;
    assert_eq!(a4.token, a2.token);
    assert_ne!(a4.connection, a3.connection);
    wait_revoked(f.addr(), &a3.connection).await?;
    assert!(run_status(&mut a4.ws, &terminal_run).await?);
    // A fresh reload is busy; takeover authority belongs to one hello only.
    assert_eq!(denied(f, &a, false, None).await?, json!({"busy":true}));
    let mut a5 = f.attach(&a, true, None).await?;
    retired(&mut a4.ws, "revoked").await?;
    assert_eq!(denied(f, &a, false, None).await?, json!({"busy":true}));
    assert!(alive(pid), "takeovers stopped the terminal");

    let (mut observer, observer_id) = f.observer().await?;
    close_workbench(&mut observer, 1, &a).await?;
    retired(&mut a5.ws, "closed").await?;
    wait_exit(pid, "closed takeover terminal").await?;
    close_workbench(&mut observer, 2, &b).await?;
    retired(&mut b1.ws, "closed").await?;
    wait_exit(b_pid, "closed B terminal").await?;
    drop(a3);
    f.release(observer, &observer_id).await?;
    Ok(())
}

async fn drain_races(f: &Fixture) -> Result<()> {
    let tasks = f.repo.join(".sworm/tasks.jsonc");
    let counter = f.repo.join("delayed-counter");
    let pids = f.repo.join("delayed-pids");
    for path in [&tasks, &counter] {
        let _ = fs::remove_file(path);
    }
    let folder = f.folder();
    let other = new_workbench();
    let mut b = f
        .attach(&other, false, None)
        .await
        .context("drain_races: attach B")?;

    // Takeover while an admitted start is stalled in its config read.
    let a = new_workbench();
    let key = format!("workbench:{}", a);
    let mut a1 = f
        .attach(&a, false, None)
        .await
        .context("drain_races: attach A1")?;
    let (mut stalled, reached) = StalledTask::new(tasks.clone())?;
    let run = new_run();
    a1.ws
        .send(request_frame(1, task_start(f, &run, false)))
        .await?;
    timeout(WAIT, reached)
        .await
        .context("start did not reach gated config read")??;
    let takeover = f.attach(&a, true, None);
    tokio::pin!(takeover);
    assert!(
        timeout(Duration::from_millis(500), &mut takeover)
            .await
            .is_err(),
        "takeover ready overtook an admitted start"
    );
    // B completes independent work while A's barrier holds.
    let b_run = new_run();
    success(&mut b.ws, 1, terminal_start(f, &b_run))
        .await?
        .session_start()
        .checked()?;
    let mut b_pty = f
        .stream(
            &b.connection,
            Open::Pty {
                run_id: b_run.clone(),
                cursor: PtyCursor::default(),
            },
        )
        .await?;
    let mut b_cursor = PtyCursor::default();
    let b_pid = shell_pid(&mut b_pty, &mut b_cursor)
        .await
        .context("drain_races: B shell pid")?;
    // Queued behind revocation: must never execute.
    let _ = a1
        .ws
        .send(request_frame(
            2,
            Request::AppStatePut {
                key: key.clone(),
                value_json: v4(&["/tmp/queued"], 0),
            },
        ))
        .await;
    stalled.unblock()?;
    // The takeover hello's frame deadline began at its first poll above, so it
    // can expire before this outer WAIT does.
    let mut a2 = timeout(WAIT, takeover)
        .await
        .context("takeover never became ready")?
        .context("drain_races: takeover hello")?;
    assert!(
        run_status(&mut a2.ws, &run).await?,
        "replacement ready overtook the admitted start"
    );
    wait_file(&counter, b"x").await?;
    // The first hello seeds the empty v4 snapshot; the queued put must not replace it.
    assert_eq!(
        get_state(&mut a2.ws, 1, &key)
            .await
            .context("drain_races: A2 get seeded state")?
            .as_deref(),
        Some(r#"{"version":4,"activeTabIndex":-1,"tabs":[]}"#),
        "queued mutation executed"
    );
    retired(&mut a1.ws, "revoked")
        .await
        .context("drain_races: A1 revoked")?;
    claim_and_watch(&mut a2.ws, &folder)
        .await
        .context("drain_races: A2 claim_and_watch")?;
    drop(a1);
    sleep(Duration::from_millis(200)).await;
    assert!(
        f.listed(&a)
            .await
            .context("drain_races: list after late A1 drop")?
            .context("A vanished")?
            .connected,
        "late old disconnect cleared the replacement"
    );
    fs::write(f.repo.join("race-watch.txt"), "changed")?;
    assert_eq!(
        event(&mut a2.ws, "files_changed")
            .await
            .context("drain_races: A2 files_changed")?["folder_path"],
        folder
    );
    success(
        &mut a2.ws,
        2,
        Request::TasksStop {
            run_id: run.clone(),
        },
    )
    .await
    .context("drain_races: A2 TasksStop")?
    .tasks_stop()
    .checked()?;
    fs::remove_file(&tasks)?;

    // Close during a stalled start, with its requester gone.
    let c = new_workbench();
    let mut c1 = f
        .attach(&c, false, None)
        .await
        .context("drain_races: attach C1")?;
    let (mut stalled, reached) = StalledTask::new(tasks.clone())?;
    let close_run = new_run();
    c1.ws
        .send(request_frame(1, task_start(f, &close_run, false)))
        .await?;
    timeout(WAIT, reached)
        .await
        .context("start did not reach gated config read")??;
    let (mut requester, requester_id) =
        f.observer().await.context("drain_races: Close requester")?;
    requester
        .send(request_frame(1, Request::WorkbenchClose { id: c.clone() }))
        .await?;
    timeout(WAIT, async {
        loop {
            let frame = denied(f, &c, false, None)
                .await
                .context("drain_races: Close fence hello")?;
            if frame.get("error").is_some() {
                assert_eq!(
                    frame["error"],
                    json!({"kind":"invalid_argument","message":"Workbench is closing; retry Close Workbench"})
                );
                return Ok::<(), anyhow::Error>(());
            }
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("Close never fenced the workbench")??;
    // The requester's page leaves; its admitted Close still runs to completion.
    requester.close(None).await?;
    drop(requester);
    sleep(Duration::from_millis(300)).await;
    assert!(
        f.listed(&c)
            .await
            .context("drain_races: list while Close drains")?
            .is_some(),
        "Close finished before the admitted start drained"
    );
    assert!(
        run_status(&mut b.ws, &b_run)
            .await
            .context("drain_races: B status during Close")?,
        "B blocked by A's Close"
    );
    stalled.unblock()?;
    f.wait_listed(&c, "closed", |info| info.is_none())
        .await
        .context("drain_races: C closed")?;
    retired(&mut c1.ws, "closed")
        .await
        .context("drain_races: C1 closed")?;
    f.wait_pruned(&requester_id).await?;
    fs::remove_file(&tasks)?;
    // Every stalled-start task process, including the one Close drained, has ended.
    for pid in fs::read_to_string(&pids).unwrap_or_default().lines() {
        wait_exit(pid.parse()?, "stalled-start task").await?;
    }
    let (mut observer, observer_id) = f.observer().await.context("drain_races: observer")?;
    close_workbench(&mut observer, 1, &a)
        .await
        .context("drain_races: close A")?;
    close_workbench(&mut observer, 2, &other)
        .await
        .context("drain_races: close B")?;
    wait_exit(b_pid, "B terminal")
        .await
        .context("drain_races: B exit")?;
    f.release(observer, &observer_id).await?;
    Ok(())
}

async fn detach_and_close(f: &Fixture) -> Result<()> {
    enable_lsp(&f.repo)?;
    let counter = f.repo.join("close-counter");
    let task_pid_file = f.repo.join("close-task.pid");
    for path in [&counter, &task_pid_file] {
        let _ = fs::remove_file(path);
    }
    fs::write(
        f.repo.join(".sworm/tasks.jsonc"),
        r#"{"version":1,"tasks":[{"id":"once","label":"Once","command":"exec sh -c 'printf %s $$ > close-task.pid; printf x >> close-counter; exec sleep 60'","singleton":true}]}"#,
    )?;
    let folder = f.folder();
    let a = new_workbench();
    let b = new_workbench();
    let (a_key, b_key) = (format!("workbench:{}", a), format!("workbench:{}", b));
    let mut a1 = f
        .attach(&a, false, None)
        .await
        .context("detach_and_close: attach A1")?;
    let mut b1 = f
        .attach(&b, false, None)
        .await
        .context("detach_and_close: attach B1")?;
    for (ws, key) in [(&mut a1.ws, &a_key), (&mut b1.ws, &b_key)] {
        put_state(ws, 1, key, &v4(&[&folder], 0)).await?;
        claim_and_watch(ws, &folder).await?;
    }
    let (a_term, a_task) = (new_run(), new_run());
    success(&mut a1.ws, 2, terminal_start(f, &a_term))
        .await?
        .session_start()
        .checked()?;
    success(&mut a1.ws, 3, task_start(f, &a_task, false))
        .await?
        .tasks_start()
        .checked()?;
    wait_file(&counter, b"x").await?;
    let task_pid: u32 = fs::read_to_string(&task_pid_file)?.parse()?;
    let mut a_pty = f
        .stream(
            &a1.connection,
            Open::Pty {
                run_id: a_term.clone(),
                cursor: PtyCursor::default(),
            },
        )
        .await?;
    let mut cursor = PtyCursor::default();
    let a_pid = shell_pid(&mut a_pty, &mut cursor)
        .await
        .context("detach_and_close: A shell pid")?;

    // B shares the folder with its own process, watch, and LSP.
    let b_term = new_run();
    success(&mut b1.ws, 2, terminal_start(f, &b_term))
        .await?
        .session_start()
        .checked()?;
    let mut b_pty = f
        .stream(
            &b1.connection,
            Open::Pty {
                run_id: b_term.clone(),
                cursor: PtyCursor::default(),
            },
        )
        .await?;
    let mut b_cursor = PtyCursor::default();
    let b_pid = shell_pid(&mut b_pty, &mut b_cursor)
        .await
        .context("detach_and_close: B shell pid")?;
    let lsp_session = "close-b-lsp";
    let mut b_lsp = f
        .stream(
            &b1.connection,
            Open::Lsp {
                session_id: lsp_session.into(),
            },
        )
        .await?;
    assert!(matches!(
        next_tagged::<LspDown>(&mut b_lsp)
            .await
            .context("detach_and_close: B LSP Ready")?,
        Tagged::Json(LspDown::Ready)
    ));
    success(
        &mut b1.ws,
        3,
        Request::LspStart {
            session_id: lsp_session.into(),
            folder_path: folder.clone(),
            server_definition_id: LSP_ID.into(),
            root_path: folder.clone(),
        },
    )
    .await?
    .lsp_start()
    .checked()?;
    let lsp_pid = match lsp_event(&mut b_lsp)
        .await
        .context("detach_and_close: B LSP Started")?
    {
        LspEvent::Started { pid: Some(pid), .. } => pid,
        other => bail!("missing B LSP PID: {other:?}"),
    };

    // B cannot observe or control A's live runs.
    for (id, request) in [
        (
            10,
            Request::RunStatus {
                run_id: a_term.clone(),
            },
        ),
        (
            11,
            Request::SessionStop {
                run_id: a_term.clone(),
            },
        ),
        (
            12,
            Request::TasksStop {
                run_id: a_task.clone(),
            },
        ),
    ] {
        let response = rpc(&mut b1.ws, id, request).await?;
        assert!(response.is_err(), "B reached A's live run: {response:?}");
    }
    let mut foreign = f
        .stream(
            &b1.connection,
            Open::Pty {
                run_id: a_term.clone(),
                cursor: PtyCursor::default(),
            },
        )
        .await?;
    pty_denied(&mut foreign)
        .await
        .context("detach_and_close: foreign PTY denied")?;
    assert!(run_status(&mut a1.ws, &a_term).await? && run_status(&mut a1.ws, &a_task).await?);

    // Detach keeps processes running and producing output.
    raw(
        &mut a_pty,
        b"(sleep 1; printf '%s%s\\n' 'DETACHED-' 'OUTPUT-4410') &\n",
    )
    .await?;
    a1.ws.close(None).await?;
    drop(a_pty);
    f.wait_detached(&a, 2)
        .await
        .context("detach_and_close: A detached")?;
    sleep(Duration::from_millis(1500)).await;
    assert!(
        alive(a_pid) && alive(task_pid),
        "detach stopped A's processes"
    );
    let mut a2 = f
        .attach(&a, false, None)
        .await
        .context("detach_and_close: reattach A2")?;
    success(&mut a2.ws, 1, task_start(f, &a_task, true))
        .await?
        .tasks_start()
        .checked()?;
    let mut a_pty = f
        .stream(
            &a2.connection,
            Open::Pty {
                run_id: a_term.clone(),
                cursor,
            },
        )
        .await?;
    pty_until(&mut a_pty, &mut cursor, b"DETACHED-OUTPUT-4410")
        .await
        .context("detach_and_close: A detached output replay")?;
    raw(&mut a_pty, b"printf 'SAME-%s-END\\n' \"$$\"\n").await?;
    let text =
        String::from_utf8_lossy(&pty_until(&mut a_pty, &mut cursor, b"-END").await?).into_owned();
    assert!(text.contains(&format!("SAME-{a_pid}-END")), "{text}");
    assert_eq!(fs::read(&counter)?, b"x", "reattach re-ran the task");

    // Explicit Close of a connected workbench.
    let (mut observer, observer_id) = f.observer().await.context("detach_and_close: observer")?;
    close_workbench(&mut observer, 1, &a)
        .await
        .context("detach_and_close: close connected A")?;
    retired(&mut a2.ws, "closed")
        .await
        .context("detach_and_close: A2 closed")?;
    wait_exit(a_pid, "closed terminal").await?;
    wait_exit(task_pid, "closed task").await?;
    let db = f.db()?;
    assert_eq!(stored(&db, &a_key)?, None, "A snapshot survived Close");
    assert!(stored(&db, &b_key)?.is_some(), "Close deleted B's snapshot");
    let ids = manifest_ids(&db)?;
    assert!(!ids.contains(&a) && ids.contains(&b), "{ids:?}");
    assert!(f
        .listed(&a)
        .await
        .context("detach_and_close: list after A Close")?
        .is_none());
    // B's shared-folder process, LSP, and watch survived.
    assert!(alive(b_pid) && alive(lsp_pid), "Close touched B");
    assert!(run_status(&mut b1.ws, &b_term).await?);
    raw(&mut b_pty, b"printf '%s\\n' 'B-AFTER-CLOSE-2231'\n").await?;
    pty_until(&mut b_pty, &mut b_cursor, b"B-AFTER-CLOSE-2231").await?;
    fs::write(f.repo.join("close-watch.txt"), "changed")?;
    assert_eq!(
        event(&mut b1.ws, "files_changed").await?["folder_path"],
        folder
    );
    // Close released A's singleton registration.
    let b_task = new_run();
    success(&mut b1.ws, 20, task_start(f, &b_task, false))
        .await?
        .tasks_start()
        .checked()?;
    wait_file(&counter, b"xx").await?;
    let b_task_pid: u32 = fs::read_to_string(&task_pid_file)?.parse()?;
    success(
        &mut b1.ws,
        21,
        Request::TasksStop {
            run_id: b_task.clone(),
        },
    )
    .await?
    .tasks_stop()
    .checked()?;
    wait_exit(b_task_pid, "B task").await?;
    // Repeat Close is idempotent and never resurrects the workbench.
    close_workbench(&mut observer, 2, &a)
        .await
        .context("detach_and_close: repeat close A")?;
    assert!(f
        .listed(&a)
        .await
        .context("detach_and_close: list after repeat Close")?
        .is_none());
    assert!(!manifest_ids(&db)?.contains(&a));

    // Explicit Close of a detached workbench.
    let d = new_workbench();
    let mut d1 = f
        .attach(&d, false, None)
        .await
        .context("detach_and_close: attach D1")?;
    let d_run = new_run();
    success(&mut d1.ws, 1, terminal_start(f, &d_run))
        .await?
        .session_start()
        .checked()?;
    let mut d_pty = f
        .stream(
            &d1.connection,
            Open::Pty {
                run_id: d_run.clone(),
                cursor: PtyCursor::default(),
            },
        )
        .await?;
    let d_pid = shell_pid(&mut d_pty, &mut PtyCursor::default()).await?;
    d1.ws.close(None).await?;
    drop(d_pty);
    f.wait_detached(&d, 1)
        .await
        .context("detach_and_close: D detached")?;
    close_workbench(&mut observer, 3, &d)
        .await
        .context("detach_and_close: close detached D")?;
    wait_exit(d_pid, "detached closed terminal").await?;
    assert!(f
        .listed(&d)
        .await
        .context("detach_and_close: list after D Close")?
        .is_none());
    assert_eq!(stored(&db, &format!("workbench:{}", d))?, None);

    close_workbench(&mut observer, 4, &b)
        .await
        .context("detach_and_close: close B")?;
    retired(&mut b1.ws, "closed")
        .await
        .context("detach_and_close: B1 closed")?;
    wait_exit(b_pid, "B terminal").await?;
    wait_exit(lsp_pid, "B LSP").await?;
    f.release(observer, &observer_id).await?;
    Ok(())
}

async fn close_storage_failure(f: &Fixture) -> Result<()> {
    let e = new_workbench();
    let key = format!("workbench:{}", e);
    let snapshot = v4(&[&f.folder()], 0);
    let mut e1 = f.attach(&e, false, None).await?;
    put_state(&mut e1.ws, 1, &key, &snapshot).await?;
    let run = new_run();
    success(&mut e1.ws, 2, terminal_start(f, &run))
        .await?
        .session_start()
        .checked()?;
    let mut pty = f
        .stream(
            &e1.connection,
            Open::Pty {
                run_id: run.clone(),
                cursor: PtyCursor::default(),
            },
        )
        .await?;
    let pid = shell_pid(&mut pty, &mut PtyCursor::default()).await?;
    let db = f.db()?;
    db.execute_batch(&format!(
        "CREATE TRIGGER block_close BEFORE DELETE ON app_state WHEN old.key = '{key}'
         BEGIN SELECT RAISE(ABORT, 'injected close failure'); END;"
    ))?;
    let (mut observer, observer_id) = f.observer().await?;
    match rpc(&mut observer, 1, Request::WorkbenchClose { id: e.clone() }).await? {
        Err(WireError::Database { .. }) => {}
        other => bail!("Close reported {other:?} despite failed deletion"),
    }
    retired(&mut e1.ws, "closed").await?;
    wait_exit(pid, "failed-close terminal").await?;
    assert!(f.listed(&e).await?.is_some(), "failed Close lost record");
    assert_eq!(stored(&db, &key)?.as_deref(), Some(snapshot.as_str()));
    assert!(manifest_ids(&db)?.contains(&e));
    assert_eq!(
        denied(f, &e, false, None).await?["error"],
        json!({"kind":"invalid_argument","message":"Workbench is closing; retry Close Workbench"}),
        "failed Close reopened the workbench"
    );
    db.execute_batch("DROP TRIGGER block_close;")?;
    timeout(WAIT, close_workbench(&mut observer, 2, &e))
        .await
        .context("retried Close hung")??;
    assert_eq!(stored(&db, &key)?, None);
    assert!(!manifest_ids(&db)?.contains(&e));
    f.release(observer, &observer_id).await?;
    Ok(())
}

async fn folders(f: &Fixture, id: &str) -> Result<Vec<String>> {
    Ok(f.listed(id).await?.context("workbench missing")?.folders)
}

async fn registry_and_restart(home: &Path) -> Result<()> {
    let root = home.join("registry");
    let f = Fixture::start(&root).await?;
    let (a, b) = (new_workbench(), new_workbench());
    let mut a1 = f.attach(&a, false, None).await?;
    let mut b1 = f.attach(&b, false, None).await?;
    let (mut observer, observer_id) = f.observer().await?;
    observer
        .send(request_frame(3, Request::WorkbenchList {}))
        .await?;
    let listed = next_reply(&mut observer).await?;
    for private in ["controller_token", "owner"] {
        assert!(
            !listed.to_string().contains(private),
            "list leaked {private}: {listed}"
        );
    }
    let listed: Vec<WorkbenchInfo> =
        serde_json::from_value::<Response>(listed["response"].clone())?
            .checked()?
            .workbench_list()
            .checked()?;
    for id in [&a, &b] {
        let info = listed
            .iter()
            .find(|info| info.id == *id)
            .context("first hello did not create its workbench")?;
        assert!(
            info.folders.is_empty() && info.running.is_empty(),
            "{info:?}"
        );
        assert!(info.connected);
    }
    f.release(observer, &observer_id).await?;

    let key = format!("workbench:{a}");
    let b_key = format!("workbench:{b}");
    put_state(&mut a1.ws, 1, &key, &v4(&["/tmp/project-a"], 0)).await?;
    assert_eq!(folders(&f, &a).await?, ["/tmp/project-a"]);
    assert!(f.listed(&a).await?.context("A missing")?.connected);
    put_state(
        &mut a1.ws,
        2,
        &key,
        &v4(&["/tmp/project-a", "/tmp/project-b"], 1),
    )
    .await?;
    // The active (second) tab's folder leads.
    assert_eq!(folders(&f, &a).await?, ["/tmp/project-b", "/tmp/project-a"]);
    for (value, expected) in [
        ("{ not json ,,".to_owned(), vec![]),
        (v4(&["/tmp/project-a"], 5), vec![]),
        (v4(&[], -1), vec![]),
        (v4(&[""], 0), vec![]),
        (
            json!({"version":3,"activeTabIndex":0,"tabs":[{"folderPath":"/tmp/project-a"}]})
                .to_string(),
            vec![],
        ),
        (v4(&["/"], 0), vec!["/"]),
    ] {
        put_state(&mut a1.ws, 3, &key, &value).await?;
        assert_eq!(
            get_state(&mut a1.ws, 4, &key).await?.as_deref(),
            Some(value.as_str()),
            "snapshot bytes were rewritten"
        );
        assert_eq!(folders(&f, &a).await?, expected, "folders of {value}");
    }
    let opaque = "{ \"version\": 4, \"activeTabIndex\": 0,  \"tabs\": [ {\"folderPath\": \"/tmp/project-a\", \"x\":  1.50} ] }";

    // Another workbench's scope and the registry itself are off limits.
    for (id, request) in [
        (40, Request::AppStateGet { key: key.clone() }),
        (
            41,
            Request::AppStatePut {
                key: key.clone(),
                value_json: "{}".into(),
            },
        ),
        (
            42,
            Request::AppStateGet {
                key: format!("branchesView:{}:main", a),
            },
        ),
        (43, Request::AppStateDelete { key: b_key.clone() }),
        (
            44,
            Request::AppStateGet {
                key: MANIFEST_KEY.into(),
            },
        ),
        (
            45,
            Request::AppStatePut {
                key: MANIFEST_KEY.into(),
                value_json: "[]".into(),
            },
        ),
        (
            46,
            Request::AppStateDelete {
                key: MANIFEST_KEY.into(),
            },
        ),
    ] {
        match rpc(&mut b1.ws, id, request).await? {
            Err(WireError::Unauthorized { .. }) => {}
            other => bail!("B reached protected state with request {id}: {other:?}"),
        }
    }
    put_state(&mut b1.ws, 47, "web:registry-preference", "1").await?;

    // Concurrent first hellos and saves preserve every record.
    let (c, d) = (new_workbench(), new_workbench());
    let b_snapshot = v4(&["/tmp/project-b"], 0);
    let (c1, d1, saved_a, saved_b) = tokio::join!(
        f.attach(&c, false, None),
        f.attach(&d, false, None),
        put_state(&mut a1.ws, 30, &key, opaque),
        put_state(&mut b1.ws, 30, &b_key, &b_snapshot),
    );
    let (c1, d1) = (c1?, d1?);
    saved_a?;
    saved_b?;
    let list = f.list().await?;
    let mut ids: Vec<_> = list.iter().map(|info| info.id.clone()).collect();
    ids.sort();
    let mut expected = vec![a.clone(), b.clone(), c.clone(), d.clone()];
    expected.sort();
    assert_eq!(ids, expected, "concurrent registry writes lost a record");
    assert_eq!(folders(&f, &a).await?, ["/tmp/project-a"]);
    assert_eq!(folders(&f, &b).await?, ["/tmp/project-b"]);
    for pair in list.windows(2) {
        let seen = |info: &WorkbenchInfo| chrono::DateTime::parse_from_rfc3339(&info.last_seen_at);
        let (first, second) = (seen(&pair[0])?, seen(&pair[1])?);
        assert!(
            first > second || (first == second && pair[0].id < pair[1].id),
            "list order: {list:?}"
        );
    }
    // Left empty, the two new workbenches are pruned.
    f.release(c1.ws, &c).await?;
    f.release(d1.ws, &d).await?;

    // A completed run's archive and a live run, both before restart.
    let archived = new_run();
    success(&mut a1.ws, 50, terminal_start(&f, &archived))
        .await?
        .session_start()
        .checked()?;
    let mut pty = f
        .stream(
            &a1.connection,
            Open::Pty {
                run_id: archived.clone(),
                cursor: PtyCursor::default(),
            },
        )
        .await?;
    raw(
        &mut pty,
        b"stty -echo; printf '%s%s\\n' 'ARCHIVE-' 'MARK-5521'; exit 7\n",
    )
    .await?;
    pty_until(&mut pty, &mut PtyCursor::default(), b"ARCHIVE-MARK-5521").await?;
    timeout(WAIT, async {
        while run_status(&mut a1.ws, &archived).await? {
            sleep(Duration::from_millis(25)).await;
        }
        Ok::<(), anyhow::Error>(())
    })
    .await
    .context("archived shell never exited")??;
    let live = new_run();
    success(&mut a1.ws, 51, terminal_start(&f, &live))
        .await?
        .session_start()
        .checked()?;
    let terminal = WorkbenchRun::Session {
        folder: fs::canonicalize(&f.repo)?.to_string_lossy().into_owned(),
        provider_id: "terminal".into(),
    };
    f.wait_listed(&a, "running", |info| {
        info.is_some_and(|info| info.connected && info.running == [terminal.clone()])
    })
    .await?;
    // A stopped session leaves the running list.
    let stopped = new_run();
    success(&mut a1.ws, 52, terminal_start(&f, &stopped))
        .await?
        .session_start()
        .checked()?;
    f.wait_listed(&a, "running twice", |info| {
        info.is_some_and(|info| info.running == [terminal.clone(), terminal.clone()])
    })
    .await?;
    success(&mut a1.ws, 53, Request::SessionStop { run_id: stopped })
        .await?
        .session_stop()
        .checked()?;
    f.wait_listed(&a, "stopped", |info| {
        info.is_some_and(|info| info.running == [terminal.clone()])
    })
    .await?;
    // Supersede a1's token, then leave everything detached.
    let mut a2 = f.attach(&a, true, None).await?;
    retired(&mut a1.ws, "revoked").await?;
    a2.ws.close(None).await?;
    b1.ws.close(None).await?;
    f.wait_detached(&a, 1).await?;
    f.wait_detached(&b, 0).await?;
    let before = f.list().await?;
    f.handle.shutdown().await;

    let db = rusqlite::Connection::open(root.join("server-data/server.db"))?;
    db.busy_timeout(Duration::from_secs(5))?;
    assert_eq!(stored(&db, &key)?.as_deref(), Some(opaque));
    db.execute(
        "INSERT INTO app_state (key, value_json, updated_at) VALUES ('workbench:dev-leftover', ?1, '2026-01-01T00:00:00+00:00')",
        [v4(&["/tmp/leftover"], 0)],
    )?;
    let manifest = stored(&db, MANIFEST_KEY)?.context("manifest missing")?;

    let f = Fixture::start(&root).await?;
    let after = f.list().await?;
    assert_eq!(after.len(), before.len(), "restart changed the registry");
    // A detach stamps last_seen_at after `connected` clears, so live timestamps
    // may predate the stored ones; restart must reproduce the stored manifest.
    let stamps: HashMap<String, (String, String)> = serde_json::from_str::<Vec<Value>>(&manifest)?
        .into_iter()
        .map(|record| {
            let field = |name: &str| record[name].as_str().map(str::to_owned);
            Ok((
                field("id").context("manifest id")?,
                (
                    field("created_at").context("manifest created_at")?,
                    field("last_seen_at").context("manifest last_seen_at")?,
                ),
            ))
        })
        .collect::<Result<_>>()?;
    for new in &after {
        let old = before
            .iter()
            .find(|old| old.id == new.id)
            .context("restart invented a workbench")?;
        assert_eq!(old.folders, new.folders);
        assert_eq!(
            stamps.get(&new.id),
            Some(&(new.created_at.clone(), new.last_seen_at.clone()))
        );
        assert!(!new.connected && new.running.is_empty(), "{new:?}");
    }
    assert_eq!(f.list().await?.len(), after.len(), "leftover was imported");
    // Durable tokens survive restart: superseded stays revoked, last page resumes.
    assert_eq!(
        denied(&f, &a, false, Some(&a1.token)).await?,
        json!({"revoked":true})
    );
    let mut a3 = f.attach(&a, false, Some(&a2.token)).await?;
    assert_eq!(a3.token, a2.token);
    assert_eq!(
        get_state(&mut a3.ws, 1, &key).await?.as_deref(),
        Some(opaque)
    );
    // Archived replay does not depend on in-memory owner metadata.
    let status = success(
        &mut a3.ws,
        2,
        Request::RunStatus {
            run_id: archived.clone(),
        },
    )
    .await?
    .run_status()
    .checked()?;
    assert!(!status.live);
    assert_eq!(status.exited, Some(Some(7)));
    let mut replay = f
        .stream(
            &a3.connection,
            Open::Pty {
                run_id: archived.clone(),
                cursor: PtyCursor::default(),
            },
        )
        .await?;
    pty_until(&mut replay, &mut PtyCursor::default(), b"ARCHIVE-MARK-5521").await?;
    a3.ws.close(None).await?;
    f.wait_detached(&a, 0).await?;
    f.handle.shutdown().await;

    // Corrupt manifests fail closed and are never replaced.
    let first = serde_json::from_str::<Vec<Value>>(&manifest)?
        .first()
        .cloned()
        .context("empty manifest")?;
    let corruptions = [
        "[{\"id\": \"truncated".to_owned(),
        json!([first.clone(), first]).to_string(),
        "{\"not\":\"an array\"}".to_owned(),
        json!([{ "id": a }]).to_string(),
    ];
    let set_manifest = |value: &str| {
        db.execute(
            "UPDATE app_state SET value_json = ?1 WHERE key = ?2",
            rusqlite::params![value, MANIFEST_KEY],
        )
    };
    let current = stored(&db, MANIFEST_KEY)?.context("manifest missing")?;
    set_manifest(&corruptions[0])?;
    let f = Fixture::start(&root).await?;
    for (index, corrupt) in corruptions.iter().enumerate() {
        if index > 0 {
            set_manifest(corrupt)?;
        }
        // Neither a known nor a new workbench opens, so nothing rewrites it.
        for id in [a.clone(), new_workbench()] {
            assert_eq!(
                denied(&f, &id, false, None).await?["error"]["kind"],
                "database",
                "corrupt manifest {corrupt} admitted {id}"
            );
        }
        assert_eq!(
            stored(&db, MANIFEST_KEY)?.as_deref(),
            Some(corrupt.as_str()),
            "corrupt manifest was replaced"
        );
        assert_eq!(stored(&db, &key)?.as_deref(), Some(opaque));
    }
    set_manifest(&current)?;
    let mut recovered: Vec<_> = f.list().await?.into_iter().map(|info| info.id).collect();
    recovered.sort();
    let mut restored: Vec<_> = after.iter().map(|info| info.id.clone()).collect();
    restored.sort();
    assert_eq!(recovered, restored, "restored manifest was not readable");
    f.handle.shutdown().await;
    Ok(())
}

async fn run(home: &Path) -> Result<()> {
    let disabled_config = home.join("disabled-config");
    let disabled_data = home.join("disabled-data");
    fs::create_dir_all(&disabled_config)?;
    fs::create_dir_all(&disabled_data)?;
    let disabled = serve(ServeOptions {
        config_dir: disabled_config,
        data_dir: disabled_data,
        listen: Some("127.0.0.1:0".parse()?),
    })
    .await?;
    let startup_config = home.join("startup-config");
    let startup_data = home.join("startup-data");
    fs::create_dir_all(&startup_config)?;
    fs::create_dir_all(&startup_data)?;
    let options = || ServeOptions {
        config_dir: startup_config.clone(),
        data_dir: startup_data.clone(),
        listen: Some("127.0.0.1:0".parse().unwrap()),
    };
    let missing = home.join("missing-web-assets");
    fs::write(
        startup_config.join("server.toml"),
        format!(
            "[web]\nbind = \"127.0.0.1:0\"\nassets_dir = {:?}\n",
            missing.to_string_lossy().as_ref()
        ),
    )?;
    fs::create_dir_all(home.join("assets"))?;
    fs::write(
        home.join("assets/index.html"),
        "<html>startup sentinel</html>",
    )?;
    assert!(
        serve(options()).await.is_err(),
        "missing web assets started successfully"
    );
    let occupied = TcpListener::bind("127.0.0.1:0")?;
    fs::write(
        startup_config.join("server.toml"),
        format!(
            "[web]\nbind = {:?}\nassets_dir = {:?}\n",
            occupied.local_addr()?.to_string(),
            home.join("assets").to_string_lossy().as_ref()
        ),
    )?;
    assert!(
        serve(options()).await.is_err(),
        "occupied web bind started successfully"
    );
    drop(occupied);
    fs::write(
        startup_config.join("server.toml"),
        format!(
            "[web]\nassets_dir = {:?}\n",
            home.join("assets").to_string_lossy().as_ref()
        ),
    )?;
    let default_web = serve(options()).await?;
    assert!(
        default_web
            .web_addr
            .expect("default web listener missing")
            .ip()
            .is_loopback(),
        "default bind exposed non-loopback interface"
    );
    default_web.shutdown().await;
    fs::write(
        startup_config.join("server.toml"),
        format!(
            "[web]\nbind = \"0.0.0.0:0\"\nassets_dir = {:?}\n",
            home.join("assets").to_string_lossy().as_ref()
        ),
    )?;
    let exposed = serve(options()).await?;
    assert!(
        exposed
            .web_addr
            .expect("explicit web bind missing")
            .ip()
            .is_unspecified(),
        "configured exposure was silently rewritten to loopback"
    );
    exposed.shutdown().await;
    assert_eq!(disabled.web_addr, None);
    disabled.shutdown().await;
    let f = Fixture::start(home).await?;
    startup_and_policy(&f)
        .await
        .context("scenario startup_and_policy")?;
    heartbeat_liveness(&f)
        .await
        .context("scenario heartbeat_liveness")?;
    events_and_correlation(&f)
        .await
        .context("scenario events_and_correlation")?;
    delayed_start_and_disconnect(&f)
        .await
        .context("scenario delayed_start_and_disconnect")?;
    retained_terminal_and_task(&f)
        .await
        .context("scenario retained_terminal_and_task")?;
    lsp_ownership(&f).await.context("scenario lsp_ownership")?;
    handshake_modes(&f)
        .await
        .context("scenario handshake_modes")?;
    workbench_lifecycle(&f)
        .await
        .context("scenario workbench_lifecycle")?;
    takeover_and_leases(&f)
        .await
        .context("scenario takeover_and_leases")?;
    drain_races(&f).await.context("scenario drain_races")?;
    detach_and_close(&f)
        .await
        .context("scenario detach_and_close")?;
    close_storage_failure(&f)
        .await
        .context("scenario close_storage_failure")?;
    file_integrity_and_shutdown(f)
        .await
        .context("scenario file_integrity_and_shutdown")?;
    registry_and_restart(home)
        .await
        .context("scenario registry_and_restart")?;
    Ok(())
}

#[test]
fn web_transport_contract() -> Result<()> {
    if std::env::var_os("SWORM_WEB_TRANSPORT_CHILD").is_none() {
        let home = tempfile::tempdir()?;
        let result = Command::new(std::env::current_exe()?)
            .args(["--exact", "web_transport_contract", "--nocapture"])
            .env("SWORM_WEB_TRANSPORT_CHILD", "1")
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_DATA_HOME", home.path().join("data"))
            .env("GIT_CONFIG_GLOBAL", home.path().join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()?;
        assert!(
            result.success(),
            "isolated web transport child failed: {result}"
        );
        return Ok(());
    }
    let home = PathBuf::from(std::env::var_os("HOME").context("isolated HOME missing")?);
    for dir in [
        "config",
        "data",
        "disabled-config",
        "disabled-data",
        "startup-config",
        "startup-data",
        "server-config",
        "server-data",
        "assets",
        "repo",
    ] {
        fs::create_dir_all(home.join(dir))?;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run(&home))
}

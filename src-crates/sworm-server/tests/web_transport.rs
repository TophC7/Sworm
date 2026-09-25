//! Real HTTP/WS daemon regressions. The test binary relaunches itself so HOME/XDG
//! are scratch paths before Tokio, Host, or global settings are initialized.
use anyhow::{bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
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
        FileReadDown, LspDown, Open, PtyCursor, PtyDown, Reply, Request, Response, WireError,
        MAX_FILE_CHUNK_BYTES,
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
    async fn control(&self) -> Result<(Socket, String)> {
        let mut socket =
            connect(self.addr(), "/ws", Some(&format!("http://{}", self.addr()))).await?;
        let first = next_text(&mut socket).await?;
        let id = first["ready"]["connection_id"]
            .as_str()
            .context("missing ready connection_id")?
            .to_owned();
        Ok((socket, id))
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
            fifo.write_all(br#"{"version":1,"tasks":[{"id":"once","label":"Once","command":"printf x >> delayed-counter; sleep 30","singleton":true}]}"#)
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
    let (mut control, _) = f.control().await?;
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
    let (mut replacement, _) = f.control().await?;
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
    assert!(event(&mut first, "recent_folders_changed")
        .await?
        .as_array()
        .context("recent event payload")?
        .iter()
        .any(|v| v == &folder));
    assert!(event(&mut second, "recent_folders_changed")
        .await?
        .as_array()
        .context("recent event payload")?
        .iter()
        .any(|v| v == &folder));
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
    let (mut control, id) = f.control().await?;
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
    let (mut replacement, new_id) = f.control().await?;
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
    let (mut owner, id) = f.control().await?;
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
    let (mut replacement, new_id) = f.control().await?;
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
    startup_and_policy(&f).await?;
    heartbeat_liveness(&f).await?;
    events_and_correlation(&f).await?;
    delayed_start_and_disconnect(&f).await?;
    retained_terminal_and_task(&f).await?;
    lsp_ownership(&f).await?;
    file_integrity_and_shutdown(f).await?;
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

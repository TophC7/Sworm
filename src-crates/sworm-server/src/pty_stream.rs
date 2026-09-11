use crate::dispatch::ServerContext;
use parking_lot::Mutex;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use sworm_core::{
    events::EventSink,
    services::{completed_runs::CompletedRun, pty::PtyReplay},
    Host,
};
use sworm_protocol::rpc::{PtyCursor, PtyDown, PtyUp, WireError};
use sworm_remote::wire::{read_tagged_frame, write_frame, write_raw_frame, Frame};
use tokio::sync::{mpsc, watch};

const STREAM_QUEUE_CAPACITY: usize = 4096;
/// Byte ceiling for queued replay. Chunk counts say nothing about memory, and
/// a stalled reader must not let the queue grow past a bounded window.
const STREAM_QUEUE_BYTES: usize = 16 * 1024 * 1024;

struct StreamOwner {
    generation: u64,
    attachment: Option<u64>,
    stop: Option<watch::Sender<bool>>,
    // Invalidated before a run id can be reused, preventing attachment-id ABA.
    active: bool,
}

pub(crate) struct RunFanout {
    owner: Mutex<StreamOwner>,
    resume_token: Arc<Mutex<Option<String>>>,
}

impl RunFanout {
    pub(crate) fn new() -> Self {
        Self {
            owner: Mutex::new(StreamOwner {
                generation: 0,
                attachment: None,
                stop: None,
                active: true,
            }),
            resume_token: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn sinks(&self) -> (EventSink<Vec<u8>>, EventSink<sworm_protocol::pty::PtyEvent>) {
        let resume_token = Arc::clone(&self.resume_token);
        (
            Arc::new(|_| Ok(())),
            Arc::new(move |event| {
                if let sworm_protocol::pty::PtyEvent::ResumeTokenBound { token, .. } = event {
                    *resume_token.lock() = Some(token);
                }
                Ok(())
            }),
        )
    }

    pub(crate) fn set_resume_token(&self, token: Option<String>) {
        if let Some(token) = token {
            *self.resume_token.lock() = Some(token);
        }
    }

    pub(crate) fn resume_token(&self) -> Option<String> {
        self.resume_token.lock().clone()
    }

    fn begin(&self) -> Option<(u64, watch::Receiver<bool>)> {
        let mut owner = self.owner.lock();
        if !owner.active {
            return None;
        }
        if let Some(stop) = owner.stop.take() {
            let _ = stop.send(true);
        }
        owner.generation = owner
            .generation
            .checked_add(1)
            .expect("PTY stream generation overflow");
        owner.attachment = None;
        let generation = owner.generation;
        let (stop, receiver) = watch::channel(false);
        owner.stop = Some(stop);
        Some((generation, receiver))
    }

    fn attach(
        &self,
        host: &Host,
        run_id: &str,
        generation: u64,
        sink: EventSink<PtyReplay>,
        cursor: PtyCursor,
    ) -> AttachOutcome {
        let mut owner = self.owner.lock();
        if !owner.active || owner.generation != generation {
            return AttachOutcome::Replaced;
        }
        owner.attachment = None;
        if host.pty.run_state(run_id).is_none() {
            return AttachOutcome::Gone;
        }
        match host.pty.attach_from(run_id, sink, cursor) {
            Ok(attachment) => {
                owner.attachment = Some(attachment);
                AttachOutcome::Attached
            }
            Err(error) if host.pty.run_state(run_id).is_some() => {
                AttachOutcome::Backpressured(error)
            }
            Err(_) => AttachOutcome::Gone,
        }
    }

    fn finish(&self, host: &Host, run_id: &str, generation: u64) {
        let mut owner = self.owner.lock();
        if !owner.active || owner.generation != generation {
            return;
        }
        if let Some(attachment) = owner.attachment.take() {
            let _ = host.pty.pause_attachment(run_id, attachment);
        }
        owner.stop = None;
    }

    pub(crate) fn cancel(&self) {
        let mut owner = self.owner.lock();
        owner.active = false;
        owner.generation = owner
            .generation
            .checked_add(1)
            .expect("PTY stream generation overflow");
        owner.attachment = None;
        if let Some(stop) = owner.stop.take() {
            let _ = stop.send(true);
        }
    }
}

enum AttachOutcome {
    Attached,
    Backpressured(String),
    Gone,
    Replaced,
}

pub(crate) async fn run(
    host: Arc<Host>,
    context: Arc<ServerContext>,
    run_id: String,
    cursor: PtyCursor,
    connection: quinn::Connection,
    mut send: quinn::SendStream,
    mut recv: quinn::RecvStream,
    mut shutdown: watch::Receiver<bool>,
) {
    let live = match context.run_fanout(&run_id).await {
        Some(fanout) => host.pty.cursor(&run_id).map(|end| (fanout, end)),
        None => None,
    };
    let Some((fanout, end)) = live else {
        // The process is gone from memory; its transcript may still be on disk.
        match context.completed.get(&run_id) {
            Ok(Some(completed)) => {
                if let Err(error) = replay_completed(&mut send, completed, cursor).await {
                    tracing::debug!(%error, run_id, "stored PTY replay failed");
                }
            }
            Ok(None) => {
                write_stream_error(
                    &mut send,
                    WireError::NotFound {
                        message: format!("PTY run not found: {run_id}"),
                    },
                )
                .await;
            }
            Err(error) => {
                write_stream_error(&mut send, WireError::Internal { message: error }).await;
            }
        }
        return;
    };
    if cursor.output_offset > end.output_offset || cursor.event_sequence > end.event_sequence {
        write_stream_error(
            &mut send,
            WireError::InvalidArgument {
                message: format!("PTY cursor is ahead of run {run_id}"),
            },
        )
        .await;
        return;
    }

    let Some((generation, replaced)) = fanout.begin() else {
        write_stream_error(
            &mut send,
            WireError::NotFound {
                message: format!("PTY run not found: {run_id}"),
            },
        )
        .await;
        return;
    };
    let (local_stop, local_stopped) = watch::channel(false);
    let writer_host = Arc::clone(&host);
    let writer_fanout = Arc::clone(&fanout);
    let writer_run_id = run_id.clone();
    let mut writer = tokio::spawn(async move {
        write_loop(
            writer_host,
            writer_fanout,
            writer_run_id,
            generation,
            cursor,
            send,
            replaced,
            local_stopped,
        )
        .await
    });

    tokio::select! {
        result = &mut writer => {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::debug!(%error, run_id, "PTY writer stopped"),
                Err(error) => tracing::warn!(%error, run_id, "PTY writer task failed"),
            }
        }
        result = read_input(&host, &run_id, &mut recv) => {
            if let Err(error) = result {
                tracing::debug!(%error, run_id, "PTY reader stopped");
            }
            let _ = local_stop.send(true);
            let _ = writer.await;
        }
        changed = shutdown.changed() => {
            let _ = changed;
            let _ = local_stop.send(true);
            let _ = writer.await;
        }
        _ = connection.closed() => {
            let _ = local_stop.send(true);
            let _ = writer.await;
        }
    }

    fanout.finish(&host, &run_id, generation);
}

async fn write_loop(
    host: Arc<Host>,
    fanout: Arc<RunFanout>,
    run_id: String,
    generation: u64,
    mut cursor: PtyCursor,
    mut send: quinn::SendStream,
    mut replaced: watch::Receiver<bool>,
    mut stopped: watch::Receiver<bool>,
) -> Result<(), String> {
    'reattach: loop {
        if *replaced.borrow() || *stopped.borrow() {
            break;
        }
        let (sender, mut receiver) = mpsc::channel(STREAM_QUEUE_CAPACITY);
        let queued = Arc::new(AtomicUsize::new(0));
        let sink_queued = Arc::clone(&queued);
        let sink: EventSink<PtyReplay> = Arc::new(move |frame| {
            let bytes = frame_bytes(&frame);
            if sink_queued.fetch_add(bytes, Ordering::AcqRel) + bytes > STREAM_QUEUE_BYTES {
                sink_queued.fetch_sub(bytes, Ordering::AcqRel);
                return Err("PTY stream queue is over its byte budget".to_owned());
            }
            sender.try_send(frame).map_err(|error| {
                sink_queued.fetch_sub(bytes, Ordering::AcqRel);
                format!("PTY stream queue full or closed: {error}")
            })
        });
        let outcome = fanout.attach(&host, &run_id, generation, Arc::clone(&sink), cursor);
        drop(sink);
        let mut wrote = false;

        loop {
            let frame = tokio::select! {
                changed = replaced.changed() => {
                    if changed.is_err() || *replaced.borrow() {
                        break 'reattach;
                    }
                    continue;
                }
                changed = stopped.changed() => {
                    if changed.is_err() || *stopped.borrow() {
                        break 'reattach;
                    }
                    continue;
                }
                _ = send.stopped() => break 'reattach,
                frame = receiver.recv() => frame,
            };
            let Some(frame) = frame else { break };
            queued.fetch_sub(frame_bytes(&frame), Ordering::AcqRel);
            write_delivery(&mut send, frame, &mut cursor).await?;
            wrote = true;
        }

        match outcome {
            AttachOutcome::Attached => {
                if host.pty.run_state(&run_id).is_none() {
                    break;
                }
                continue 'reattach;
            }
            AttachOutcome::Backpressured(error) if wrote => {
                tracing::debug!(
                    run_id,
                    "PTY replay queue filled; resuming from written cursor"
                );
                continue 'reattach;
            }
            AttachOutcome::Backpressured(error) => return Err(error),
            AttachOutcome::Gone | AttachOutcome::Replaced => break,
        }
    }

    send.finish()
        .map_err(|error| format!("finish PTY stream: {error}"))
}

fn frame_bytes(frame: &PtyReplay) -> usize {
    match frame {
        PtyReplay::Output { bytes, .. } => bytes.len(),
        PtyReplay::Gap { .. } | PtyReplay::Event { .. } => 0,
    }
}

/// Replay a finished run from its stored transcript. Same frames a live
/// window replay produces, so the desktop cannot tell the two apart.
async fn replay_completed(
    send: &mut quinn::SendStream,
    completed: CompletedRun,
    mut cursor: PtyCursor,
) -> Result<(), String> {
    if cursor.output_offset < completed.output_start {
        let lost_bytes = completed.output_start - cursor.output_offset;
        write_delivery(send, PtyReplay::Gap { lost_bytes }, &mut cursor).await?;
    }
    let skip = usize::try_from(cursor.output_offset.saturating_sub(completed.output_start))
        .unwrap_or(usize::MAX)
        .min(completed.output.len());
    if skip < completed.output.len() {
        write_delivery(
            send,
            PtyReplay::Output {
                start_offset: completed.output_start + skip as u64,
                bytes: completed.output[skip..].to_vec(),
            },
            &mut cursor,
        )
        .await?;
    }
    for (sequence, event) in completed.events {
        if sequence > cursor.event_sequence {
            write_delivery(send, PtyReplay::Event { sequence, event }, &mut cursor).await?;
        }
    }
    write_frame(
        send,
        &PtyDown::Event {
            sequence: cursor.event_sequence,
            event: sworm_protocol::pty::PtyEvent::Synced {
                run_id: completed.run_id,
                sequence: cursor.event_sequence,
            },
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    send.finish()
        .map_err(|error| format!("finish PTY stream: {error}"))
}

async fn write_delivery(
    send: &mut quinn::SendStream,
    frame: PtyReplay,
    cursor: &mut PtyCursor,
) -> Result<(), String> {
    match frame {
        PtyReplay::Gap { lost_bytes } => {
            write_frame(send, &PtyDown::Gap { lost_bytes })
                .await
                .map_err(|error| error.to_string())?;
            cursor.output_offset = cursor
                .output_offset
                .checked_add(lost_bytes)
                .ok_or_else(|| "PTY output cursor overflow".to_owned())?;
        }
        PtyReplay::Output {
            start_offset,
            bytes,
        } => {
            if start_offset != cursor.output_offset {
                return Err(format!(
                    "non-contiguous PTY output for {start_offset}: expected {}",
                    cursor.output_offset
                ));
            }
            let end_offset = start_offset
                .checked_add(bytes.len() as u64)
                .ok_or_else(|| "PTY output cursor overflow".to_owned())?;
            let mut body = Vec::with_capacity(8 + bytes.len());
            body.extend_from_slice(&start_offset.to_be_bytes());
            body.extend_from_slice(&bytes);
            write_raw_frame(send, &body)
                .await
                .map_err(|error| error.to_string())?;
            cursor.output_offset = end_offset;
        }
        PtyReplay::Event { sequence, event } => {
            write_frame(send, &PtyDown::Event { sequence, event })
                .await
                .map_err(|error| error.to_string())?;
            cursor.event_sequence = sequence;
        }
    }
    Ok(())
}

async fn read_input(host: &Host, run_id: &str, recv: &mut quinn::RecvStream) -> Result<(), String> {
    loop {
        // A rejected write or resize (exited run, replay tail) is not a stream
        // fault: tearing the stream down would only trigger a reattach loop.
        let result = match read_tagged_frame::<PtyUp>(recv)
            .await
            .map_err(|error| error.to_string())?
        {
            Frame::Raw(bytes) => host.pty.write(run_id, &bytes),
            Frame::Json(PtyUp::Resize { cols, rows }) => host.pty.resize(run_id, cols, rows),
        };
        if let Err(error) = result {
            tracing::debug!(%error, run_id, "PTY input rejected");
        }
    }
}

async fn write_stream_error(send: &mut quinn::SendStream, error: WireError) {
    if let Err(write_error) = write_frame(send, &PtyDown::Closed { error }).await {
        tracing::debug!(%write_error, "failed to write PTY stream error");
        return;
    }
    let _ = send.finish();
}

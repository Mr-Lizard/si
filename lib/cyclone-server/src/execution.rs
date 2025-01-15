use std::{
    fmt, io,
    marker::{PhantomData, Unpin},
    path::PathBuf,
    process::Stdio,
    string::FromUtf8Error,
    sync::Arc,
    time::Duration,
};

use axum::extract::ws::WebSocket;
use bytes_lines_codec::BytesLinesCodec;
use cyclone_core::{
    process::{self, ShutdownError},
    CycloneRequest, CycloneRequestable, FunctionResult, FunctionResultFailure,
    FunctionResultFailureError, FunctionResultFailureErrorKind, Message, OutputStream,
};
use futures::{SinkExt, StreamExt, TryStreamExt};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use si_crypto::SensitiveStrings;
use telemetry::prelude::*;
use thiserror::Error;
use tokio::time::timeout;
use tokio::{
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command},
    time,
};
use tokio_serde::{formats::SymmetricalJson, Deserializer, Framed, SymmetricallyFramed};
use tokio_util::codec::{Decoder, FramedRead, FramedWrite};

use crate::WebSocketMessage;

const TX_TIMEOUT_SECS: Duration = Duration::from_secs(5);
const DEFAULT_LANG_SERVER_PROCESS_TIMEOUT: Duration = Duration::from_secs(5 * 60);

pub fn new<Request, LangServerSuccess, Success>(
    lang_server_path: impl Into<PathBuf>,
    lang_server_debugging: bool,
    lang_server_function_timeout: Option<usize>,
    lang_server_process_timeout: Option<u64>,
    command: String,
) -> Execution<Request, LangServerSuccess, Success>
where
    Request: CycloneRequestable,
{
    Execution {
        lang_server_path: lang_server_path.into(),
        lang_server_debugging,
        lang_server_function_timeout,
        lang_server_process_timeout: match lang_server_process_timeout {
            Some(timeout) => Duration::from_secs(timeout),
            None => DEFAULT_LANG_SERVER_PROCESS_TIMEOUT,
        },
        command,
        request_marker: PhantomData,
        lang_server_success_marker: PhantomData,
        success_marker: PhantomData,
    }
}

#[remain::sorted]
#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("failed to consume the {0} stream for the child process")]
    ChildIO(&'static str),
    #[error("failed to receive child process message")]
    ChildRecvIO(#[source] io::Error),
    #[error("failed to send child process message")]
    ChildSendIO(#[source] io::Error),
    #[error(transparent)]
    ChildShutdown(#[from] ShutdownError),
    #[error("failed to spawn child process; program={0}")]
    ChildSpawn(#[source] io::Error, PathBuf),
    #[error("child process timed out: {0:?}")]
    ChildTimeout(Duration),
    #[error("failed to decode string as utf8")]
    FromUtf8(#[from] FromUtf8Error),
    #[error("failed to deserialize json message")]
    JSONDeserialize(#[source] serde_json::Error),
    #[error("failed to serialize json message")]
    JSONSerialize(#[source] serde_json::Error),
    #[error("send timeout")]
    SendTimeout(#[source] tokio::time::error::Elapsed),
    #[error("unexpected websocket message type: {0:?}")]
    UnexpectedMessageType(WebSocketMessage),
    #[error("failed to close websocket")]
    WSClose(#[source] axum::Error),
    #[error("failed to receive websocket message--stream is closed")]
    WSRecvClosed,
    #[error("failed to receive websocket message")]
    WSRecvIO(#[source] axum::Error),
    #[error("failed to send websocket message")]
    WSSendIO(#[source] axum::Error),
}

type Result<T> = std::result::Result<T, ExecutionError>;

#[derive(Debug)]
pub struct Execution<Request, LangServerSuccess, Success>
where
    Request: CycloneRequestable,
{
    lang_server_path: PathBuf,
    lang_server_debugging: bool,
    lang_server_function_timeout: Option<usize>,
    lang_server_process_timeout: Duration,
    command: String,
    request_marker: PhantomData<Request>,
    lang_server_success_marker: PhantomData<LangServerSuccess>,
    success_marker: PhantomData<Success>,
}

impl<Request, LangServerSuccess, Success> Execution<Request, LangServerSuccess, Success>
where
    Request: Serialize + DeserializeOwned + Unpin + core::fmt::Debug + CycloneRequestable,
    LangServerSuccess: DeserializeOwned,
    Success: Serialize,
{
    pub async fn start(
        self,
        ws: &mut WebSocket,
    ) -> Result<ExecutionStarted<LangServerSuccess, Success>> {
        // Send start is the initial communication before we read the request.
        Self::ws_send_start(ws).await?;
        // Read the request message from the web socket
        let cyclone_request = Self::read_request(ws).await?;
        let (request, sensitive_strings) = cyclone_request.into_parts();

        // Spawn lang server as a child process with handles on all i/o descriptors
        let mut command = Command::new(&self.lang_server_path);
        command
            .arg(&self.command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(timeout) = self.lang_server_function_timeout {
            command.arg("--timeout").arg(timeout.to_string());
        }
        if self.lang_server_debugging {
            command.env("SI_LANG_JS_LOG", "*");
        }

        debug!(cmd = ?command, "spawning child process");
        let mut child = command
            .spawn()
            .map_err(|err| ExecutionError::ChildSpawn(err, self.lang_server_path.clone()))?;

        let stdin = child.stdin.take().ok_or(ExecutionError::ChildIO("stdin"))?;
        Self::child_send_function_request(stdin, request).await?;

        let stderr = {
            let stderr = child
                .stderr
                .take()
                .ok_or(ExecutionError::ChildIO("stderr"))?;
            FramedRead::new(stderr, BytesLinesCodec::new())
        };

        let stdout = {
            let stdout = child
                .stdout
                .take()
                .ok_or(ExecutionError::ChildIO("stdout"))?;
            let codec = FramedRead::new(stdout, BytesLinesCodec::new());
            SymmetricallyFramed::new(codec, SymmetricalJson::default())
        };

        Ok(ExecutionStarted {
            child,
            stdout,
            stderr,
            sensitive_strings: Arc::new(sensitive_strings),
            success_marker: self.success_marker,
            lang_server_process_timeout: self.lang_server_process_timeout,
        })
    }

    async fn read_request(ws: &mut WebSocket) -> Result<CycloneRequest<Request>> {
        let request = match ws.next().await {
            Some(Ok(WebSocketMessage::Text(json_str))) => {
                serde_json::from_str(&json_str).map_err(ExecutionError::JSONDeserialize)?
            }
            Some(Ok(unexpected)) => return Err(ExecutionError::UnexpectedMessageType(unexpected)),
            Some(Err(err)) => return Err(ExecutionError::WSRecvIO(err)),
            None => return Err(ExecutionError::WSRecvClosed),
        };
        Ok(request)
    }

    async fn ws_send_start(ws: &mut WebSocket) -> Result<()> {
        let msg = Message::<Success>::Start
            .serialize_to_string()
            .map_err(ExecutionError::JSONSerialize)?;

        time::timeout(TX_TIMEOUT_SECS, ws.send(WebSocketMessage::Text(msg)))
            .await
            .map_err(ExecutionError::SendTimeout)?
            .map_err(ExecutionError::WSSendIO)?;
        Ok(())
    }

    async fn child_send_function_request(stdin: ChildStdin, request: Request) -> Result<()> {
        let value = serde_json::to_value(&request).map_err(ExecutionError::JSONSerialize)?;

        let codec = FramedWrite::new(stdin, BytesLinesCodec::new());
        let mut stdin = SymmetricallyFramed::new(codec, SymmetricalJson::default());

        time::timeout(TX_TIMEOUT_SECS, stdin.send(value))
            .await
            .map_err(ExecutionError::SendTimeout)?
            .map_err(ExecutionError::ChildSendIO)?;
        time::timeout(TX_TIMEOUT_SECS, stdin.close())
            .await
            .map_err(ExecutionError::SendTimeout)?
            .map_err(ExecutionError::ChildSendIO)?;
        Ok(())
    }
}

type SiFramedRead = FramedRead<ChildStdout, BytesLinesCodec>;
type SiFramed<S> = Framed<SiFramedRead, S, S, SymmetricalJson<S>>;
type SiMessage<S> = LangServerMessage<S>;
type SiDecoderError = <BytesLinesCodec as Decoder>::Error;
type SiJsonError<S> = <SymmetricalJson<SiMessage<S>> as Deserializer<SiMessage<S>>>::Error;

#[derive(Debug)]
pub struct ExecutionStarted<LangServerSuccess, Success> {
    child: Child,
    stdout: SiFramed<SiMessage<LangServerSuccess>>,
    stderr: FramedRead<ChildStderr, BytesLinesCodec>,
    sensitive_strings: Arc<SensitiveStrings>,
    success_marker: PhantomData<Success>,
    lang_server_process_timeout: Duration,
}

// TODO: implement shutdown oneshot
async fn handle_stderr(
    stderr: FramedRead<ChildStderr, BytesLinesCodec>,
    sensitive_strings: Arc<SensitiveStrings>,
) {
    async fn handle_stderr_fallible(
        mut stderr: FramedRead<ChildStderr, BytesLinesCodec>,
        sensitive_strings: Arc<SensitiveStrings>,
    ) -> Result<()> {
        while let Some(line) = stderr.next().await {
            let line = line.map_err(ExecutionError::ChildRecvIO)?;
            let line = String::from_utf8(line.to_vec())?;
            let line = sensitive_strings.redact(line.as_ref());

            eprintln!("{line}");
        }
        Ok(())
    }
    if let Err(error) = handle_stderr_fallible(stderr, sensitive_strings).await {
        error!("Unable to collect stderr: {}", error);
    }
}

impl<LangServerSuccess, Success> ExecutionStarted<LangServerSuccess, Success>
where
    Success: Serialize + Unpin + fmt::Debug,
    LangServerSuccess: Serialize + DeserializeOwned + Unpin + fmt::Debug + Into<Success>,
    SymmetricalJson<SiMessage<LangServerSuccess>>: Deserializer<SiMessage<LangServerSuccess>>,
    SiDecoderError: From<SiJsonError<LangServerSuccess>>,
{
    pub async fn process(mut self, ws: &mut WebSocket) -> Result<ExecutionClosing<Success>> {
        tokio::spawn(handle_stderr(self.stderr, self.sensitive_strings.clone()));

        let mut stream = self
            .stdout
            .map(|ls_result| match ls_result {
                Ok(ls_msg) => match ls_msg {
                    LangServerMessage::Output(mut output) => {
                        Self::filter_output(&mut output, &self.sensitive_strings)?;
                        Ok(Message::OutputStream(output.into()))
                    }
                    LangServerMessage::Result(mut result) => {
                        Self::filter_result(&mut result, &self.sensitive_strings)?;
                        Ok(Message::Result(result.into()))
                    }
                },
                Err(err) => Err(ExecutionError::ChildRecvIO(err)),
            })
            .map(|msg_result: Result<_>| match msg_result {
                Ok(msg) => match msg
                    .serialize_to_string()
                    .map_err(ExecutionError::JSONSerialize)
                {
                    Ok(json_str) => Ok(WebSocketMessage::Text(json_str)),
                    Err(err) => Err(err),
                },
                Err(err) => Err(err),
            });

        let receive_loop = async {
            while let Some(msg) = stream.try_next().await? {
                ws.send(msg).await.map_err(ExecutionError::WSSendIO)?;
            }

            Result::<_>::Ok(())
        };

        match timeout(self.lang_server_process_timeout, receive_loop).await {
            Ok(execution) => execution?,
            Err(err) => {
                // Exceeded timeout, shutdown child process
                process::child_shutdown(&mut self.child, Some(process::Signal::SIGTERM), None)
                    .await?;
                drop(self.child);

                error!(?err, "shutdown child process due to timeout");
                return Err(ExecutionError::ChildTimeout(
                    self.lang_server_process_timeout,
                ));
            }
        };

        Ok(ExecutionClosing {
            child: self.child,
            success_marker: PhantomData,
        })
    }

    fn filter_output(
        output: &mut LangServerOutput,
        sensitive_strings: &SensitiveStrings,
    ) -> Result<()> {
        if sensitive_strings.has_sensitive(&output.message) {
            output.message = sensitive_strings.redact(&output.message);
        }

        Ok(())
    }

    fn filter_result(
        result: &mut LangServerResult<LangServerSuccess>,
        sensitive_strings: &SensitiveStrings,
    ) -> Result<()> {
        let mut value = serde_json::to_value(&result).map_err(ExecutionError::JSONSerialize)?;

        let mut work_queue = vec![&mut value];
        while let Some(work) = work_queue.pop() {
            match work {
                Value::Array(values) => work_queue.extend(values),
                Value::Object(object) => object.values_mut().for_each(|v| work_queue.push(v)),
                Value::String(string) if sensitive_strings.has_sensitive(string) => {
                    *string = sensitive_strings.redact(string);
                }
                Value::String(_) | Value::Null | Value::Number(_) | Value::Bool(_) => {}
            }
        }

        let mut filtered_result: LangServerResult<LangServerSuccess> =
            serde_json::from_value(value).map_err(ExecutionError::JSONDeserialize)?;
        std::mem::swap(result, &mut filtered_result);
        Ok(())
    }
}

#[derive(Debug)]
pub struct ExecutionClosing<Success> {
    child: Child,
    success_marker: PhantomData<Success>,
}

impl<Success> ExecutionClosing<Success>
where
    Success: Serialize,
{
    pub async fn finish(mut self, mut ws: WebSocket) -> Result<()> {
        let finished = Self::ws_send_finish(&mut ws).await;
        let closed = Self::ws_close(ws).await;
        let shutdown =
            process::child_shutdown(&mut self.child, Some(process::Signal::SIGTERM), None)
                .await
                .map_err(Into::into);
        drop(self.child);

        match (finished, closed, shutdown) {
            // Everything succeeds, great!
            (Ok(_), Ok(_), Ok(_)) => Ok(()),

            // One of the steps failed, return its error
            (Ok(_), Ok(_), Err(err)) | (Ok(_), Err(err), Ok(_)) | (Err(err), Ok(_), Ok(_)) => {
                Err(err)
            }

            // 2/3 steps errored so warn about the lower priority error and return the highest
            // priority
            (Ok(_), Err(err), Err(shutdown)) => {
                warn!(si.error.message = ?shutdown, "failed to shutdown child cleanly");
                Err(err)
            }
            (Err(err), Ok(_), Err(shutdown)) => {
                warn!(si.error.message = ?shutdown, "failed to shutdown child cleanly");
                Err(err)
            }
            (Err(err), Err(closed), Ok(_)) => {
                warn!(si.error.message = ?closed, "failed to cleanly close websocket");
                Err(err)
            }

            // All steps failed so warn about the lower priorities and return the highest priority
            (Err(err), Err(closed), Err(shutdown)) => {
                warn!(si.error.message = ?shutdown, "failed to shutdown child cleanly");
                warn!(si.error.message = ?closed, "failed to cleanly close websocket");
                Err(err)
            }
        }
    }

    async fn ws_send_finish(ws: &mut WebSocket) -> Result<()> {
        let msg = Message::<Success>::Finish
            .serialize_to_string()
            .map_err(ExecutionError::JSONSerialize)?;
        time::timeout(TX_TIMEOUT_SECS, ws.send(WebSocketMessage::Text(msg)))
            .await
            .map_err(ExecutionError::SendTimeout)?
            .map_err(ExecutionError::WSSendIO)?;

        Ok(())
    }

    async fn ws_close(ws: WebSocket) -> Result<()> {
        ws.close().await.map_err(ExecutionError::WSClose)
    }
}

#[remain::sorted]
#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "protocol", rename_all = "camelCase")]
pub enum LangServerMessage<Success> {
    Output(LangServerOutput),
    Result(LangServerResult<Success>),
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LangServerOutput {
    execution_id: String,
    stream: String,
    level: String,
    group: Option<String>,
    message: String,
}

impl From<LangServerOutput> for OutputStream {
    fn from(value: LangServerOutput) -> Self {
        Self {
            execution_id: value.execution_id,
            stream: value.stream,
            level: value.level,
            group: value.group,
            message: value.message,
            timestamp: crate::timestamp(),
        }
    }
}

#[remain::sorted]
#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum LangServerResult<Success> {
    Failure(LangServerFailure),
    Success(Success),
}

impl<LangServerSuccess, Success> From<LangServerResult<LangServerSuccess>>
    for FunctionResult<Success>
where
    LangServerSuccess: Into<Success>,
{
    fn from(value: LangServerResult<LangServerSuccess>) -> Self {
        match value {
            LangServerResult::Success(success) => Self::Success(success.into()),
            LangServerResult::Failure(failure) => Self::Failure(FunctionResultFailure::new(
                failure.execution_id,
                FunctionResultFailureError {
                    kind: failure.error.kind,
                    message: failure.error.message,
                },
                crate::timestamp(),
            )),
        }
    }
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LangServerFailure {
    #[serde(default)]
    execution_id: String,
    error: LangServerFailureError,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct LangServerFailureError {
    kind: FunctionResultFailureErrorKind,
    message: String,
}

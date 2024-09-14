use anyhow::Context;
use futures_util::sink::{Sink, SinkExt};
use futures_util::stream::Stream;
use futures_util::TryStreamExt;
use nix::{sys::stat::Mode, unistd::mkfifo};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::net::unix::pipe;

pub type LineSink = Box<dyn Sink<String, Error = anyhow::Error> + Send + Unpin>;
pub type LineStream = Box<dyn Stream<Item = Result<String, anyhow::Error>> + Send + Unpin>;

#[derive(Clone, Debug)]
pub struct AgentIO {
    pub their_stdin: std::path::PathBuf,
    pub their_stdout: std::path::PathBuf,
}

// Opens the pipes specified in AgentIO, waiting for the sender pipe
// to become available within `sender_timeout`. Opening a Linux FIFO for writing
// blocks until it's also open for reading by someone else. If the agent is not
// opening the pipe within the timeout, an error is returned.
pub async fn open(
    pio: AgentIO,
    line_length_limit: usize,
    sender_timeout: std::time::Duration,
) -> anyhow::Result<(LineStream, LineSink)> {
    Ok((
        make_line_stream(
            pipe::OpenOptions::new().open_receiver(pio.their_stdout)?,
            line_length_limit,
        ),
        make_line_sink(
            open_sender_with_timeout(pio.their_stdin, sender_timeout).await?,
            line_length_limit,
        ),
    ))
}

// Creates the pipes specified in the AgentIO.
pub fn create(pio: &AgentIO) -> anyhow::Result<()> {
    mkfifo(&pio.their_stdin, Mode::S_IRWXU).context("Failed to create stdin fifo")?;
    mkfifo(&pio.their_stdout, Mode::S_IRWXU).context("Failed to create stdout fifo")?;
    Ok(())
}

// Opens the given file for writing, with the given timeout.
// This function exists because either opening the file in
// blocking or non-blocking mode doesn't work well with pipes.
// In blocking mode, we can just block forever, and the async
// version leaks the file descriptor.
// In non-blocking mode, if the file is not open for reading on the
// remote end, "open" returns an error and needs to be retried.
// That is what this function does.
async fn open_sender_with_timeout(
    path: impl AsRef<std::path::Path>,
    timeout: std::time::Duration,
) -> anyhow::Result<pipe::Sender> {
    let future = async move {
        loop {
            match pipe::OpenOptions::new().open_sender(&path) {
                Err(e) => {
                    if e.raw_os_error() != Some(nix::Error::ENXIO as i32) {
                        return Err(e);
                    }
                }
                Ok(x) => return Ok(x),
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    };
    Ok(tokio::time::timeout(timeout, future).await??)
}

pub async fn read_with_limit<R: AsyncRead + Unpin>(mut r: R, limit: usize) -> String {
    let mut buffer = vec![0; limit];
    let mut fullness = 0;
    loop {
        match r.read(&mut buffer[fullness..]).await {
            Ok(0) => break,
            Ok(read) => {
                fullness += read;
            }
            Err(e) => {
                log::error!("Failed to read with limit: {e}");
                break;
            }
        }
    }
    String::from_utf8_lossy(&buffer[..fullness]).into_owned()
}

fn make_line_sink<W: AsyncWrite + Send + Unpin + 'static>(w: W, line_limit: usize) -> LineSink {
    Box::new(
        tokio_util::codec::FramedWrite::new(
            w,
            tokio_util::codec::LinesCodec::new_with_max_length(line_limit),
        )
        .sink_err_into(),
    )
}

fn make_line_stream<R: AsyncRead + Send + Unpin + 'static>(r: R, line_limit: usize) -> LineStream {
    Box::new(
        tokio_util::codec::FramedRead::new(
            r,
            tokio_util::codec::LinesCodec::new_with_max_length(line_limit),
        )
        .err_into(),
    )
}

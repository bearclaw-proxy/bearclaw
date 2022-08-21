use std::io::Cursor;

use bytes::{Buf, Bytes, BytesMut};
use chrono::TimeZone;
use tokio::{
    io::{AsyncReadExt, AsyncWrite, AsyncWriteExt, BufWriter},
    net::TcpStream,
};

const DEFAULT_RECEIVE_BUFFER_SIZE_IN_BYTES: usize = 4 * 1024 * 1024;

pub(crate) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub(crate) enum Error {
    ConnectionResetByPeer,
    IOFailure(std::io::Error),
    ProtocolViolation,
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::IOFailure(e)
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(_: std::string::FromUtf8Error) -> Self {
        Self::ProtocolViolation
    }
}

/// A connection to the bootstrap proxy that sends requests through the proxy.
pub(crate) struct Forwarder {
    stream: BufWriter<TcpStream>,
    recv_buf: BytesMut,
}

impl Forwarder {
    pub(crate) async fn connect(endpoint: &str) -> Result<Self> {
        let stream = TcpStream::connect(endpoint).await?;
        Ok(Self {
            stream: BufWriter::new(stream),
            recv_buf: BytesMut::with_capacity(DEFAULT_RECEIVE_BUFFER_SIZE_IN_BYTES),
        })
    }

    pub(crate) async fn forward(
        &mut self,
        host: &str,
        port: u16,
        use_https: bool,
        bytes: &[u8],
    ) -> Result<Option<Bytes>> {
        self.stream.write_u8(1).await?;
        write_string(&mut self.stream, host).await?;
        self.stream.write_u16(port).await?;
        write_bool(&mut self.stream, use_https).await?;
        write_bytes(&mut self.stream, bytes).await?;
        write_bool(&mut self.stream, false).await?;
        self.stream.flush().await?;

        loop {
            if self.stream.read_buf(&mut self.recv_buf).await? == 0 {
                return Err(Error::ConnectionResetByPeer);
            }

            if self.has_frame() {
                tracing::trace!(forwarder_recvbuf_capacity = self.recv_buf.capacity());
                let len = self.recv_buf.get_u32() as usize;

                if len == 0 {
                    return Ok(None);
                }

                // This doesn't actually perform a copy, it will reference the existing buffer
                let result = self.recv_buf.copy_to_bytes(len);
                return Ok(Some(result));
            }
        }
    }

    fn has_frame(&mut self) -> bool {
        let mut buf = Cursor::new(&self.recv_buf[..]);

        if buf.remaining() < 4 {
            return false;
        }

        let size = buf.get_u32() as usize;
        buf.remaining() >= size
    }
}

async fn write_string<W>(stream: &mut W, val: &str) -> Result<()>
where
    W: AsyncWrite,
    W: Unpin,
{
    let bytes = val.as_bytes();
    stream.write_u32(bytes.len() as u32).await?;
    stream.write_all(bytes).await?;
    Ok(())
}

async fn write_bool<W>(stream: &mut W, val: bool) -> Result<()>
where
    W: AsyncWrite,
    W: Unpin,
{
    stream
        .write_u8(match val {
            true => 1,
            false => 0,
        })
        .await?;
    Ok(())
}

async fn write_bytes<W>(stream: &mut W, val: &[u8]) -> Result<()>
where
    W: AsyncWrite,
    W: Unpin,
{
    stream.write_u32(val.len() as u32).await?;
    stream.write_all(val).await?;
    Ok(())
}

/// A connection to the bootstrap proxy that receives messages intercepted by the proxy.
pub(super) struct Interceptor {
    stream: TcpStream,
    recv_buf: BytesMut,
}

impl Interceptor {
    pub(super) async fn connect(endpoint: &str) -> Result<Self> {
        let stream = TcpStream::connect(endpoint).await?;
        let recv_buf = BytesMut::with_capacity(DEFAULT_RECEIVE_BUFFER_SIZE_IN_BYTES);
        Ok(Self { stream, recv_buf })
    }

    pub(super) async fn intercept(&mut self) -> Result<InterceptedMessage> {
        self.stream.write_u8(2).await?;

        loop {
            if self.stream.read_buf(&mut self.recv_buf).await? == 0 {
                return Err(Error::ConnectionResetByPeer);
            }

            if InterceptedMessage::check(&mut Cursor::new(&self.recv_buf[..])) {
                tracing::trace!(interceptor_recvbuf_capacity = self.recv_buf.capacity());
                self.stream.write_u32(0).await?;
                return InterceptedMessage::parse(&mut self.recv_buf);
            }
        }
    }
}

// TODO: Remove clone when unneeded
#[derive(Clone, Debug)]
pub(crate) struct InterceptedMessage {
    pub received_at: chrono::DateTime<chrono::Local>,
    pub host: String,
    pub port: u16,
    pub is_https: bool,
    pub request: Bytes,
    pub response: Bytes,
}

impl std::fmt::Display for InterceptedMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Received at {}", self.received_at)?;
        writeln!(
            f,
            "{} connection to {}:{}\n",
            if self.is_https { "https" } else { "HTTP" },
            self.host,
            self.port,
        )?;
        writeln!(f, "{}", String::from_utf8_lossy(&self.request))?;
        writeln!(f, "{}", String::from_utf8_lossy(&self.response))?;
        Ok(())
    }
}

impl InterceptedMessage {
    fn check(buf: &mut Cursor<&[u8]>) -> bool {
        if !check_len(buf, 12) {
            return false;
        }

        if !check_bytes(buf) {
            return false;
        };

        if !check_len(buf, 2) {
            return false;
        }

        if !check_bytes(buf) {
            return false;
        };

        if !check_bytes(buf) {
            return false;
        };

        if !check_bytes(buf) {
            return false;
        };

        true
    }

    fn parse(buf: &mut BytesMut) -> Result<Self> {
        Ok(Self {
            received_at: chrono::Local.timestamp(buf.get_i64(), buf.get_u32()),
            host: get_string(buf)?,
            port: buf.get_u16(),
            is_https: match get_string(buf)?.as_ref() {
                "https" => true,
                "http" => false,
                _ => return Err(Error::ProtocolViolation),
            },
            request: get_bytes(buf),
            response: get_bytes(buf),
        })
    }
}

fn check_len<B>(buf: &mut B, len: usize) -> bool
where
    B: Buf,
{
    if buf.remaining() < len {
        return false;
    }

    buf.advance(len);
    true
}

fn check_bytes<B>(buf: &mut B) -> bool
where
    B: Buf,
{
    if buf.remaining() < 4 {
        return false;
    }

    let num_bytes = buf.get_u32() as usize;

    if buf.remaining() >= num_bytes {
        buf.advance(num_bytes);
        true
    } else {
        false
    }
}

fn get_string<B>(buf: &mut B) -> Result<String>
where
    B: Buf,
{
    let num_bytes = buf.get_u32() as usize;
    let bytes = buf.copy_to_bytes(num_bytes);
    Ok(String::from_utf8(bytes.to_vec())?)
}

fn get_bytes<B>(buf: &mut B) -> Bytes
where
    B: Buf,
{
    let num_bytes = buf.get_u32() as usize;
    buf.copy_to_bytes(num_bytes)
}

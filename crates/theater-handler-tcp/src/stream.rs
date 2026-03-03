//! Unified stream abstraction for TLS-transparent TCP connections.
//!
//! This module provides `UnifiedStream` and its split halves, which abstract over
//! plain TCP and TLS connections. This allows the TCP handler to use the same code
//! paths regardless of whether TLS is enabled.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream as ClientTlsStream;
use tokio_rustls::server::TlsStream as ServerTlsStream;

/// A unified stream that abstracts over plain TCP and TLS connections.
///
/// This enum allows the TCP handler to work with both encrypted and unencrypted
/// connections using the same interface.
pub enum UnifiedStream {
    /// Plain TCP connection (no encryption)
    Plain(TcpStream),
    /// TLS client connection (outbound connection with TLS)
    ClientTls(ClientTlsStream<TcpStream>),
    /// TLS server connection (inbound connection with TLS)
    ServerTls(ServerTlsStream<TcpStream>),
}

impl UnifiedStream {
    /// Split the stream into read and write halves.
    ///
    /// This consumes the stream and returns separate read and write halves
    /// that can be used independently.
    pub fn into_split(self) -> (UnifiedReadHalf, UnifiedWriteHalf) {
        match self {
            UnifiedStream::Plain(stream) => {
                let (read, write) = stream.into_split();
                (UnifiedReadHalf::Plain(read), UnifiedWriteHalf::Plain(write))
            }
            UnifiedStream::ClientTls(stream) => {
                let (read, write) = tokio::io::split(stream);
                (
                    UnifiedReadHalf::ClientTls(read),
                    UnifiedWriteHalf::ClientTls(write),
                )
            }
            UnifiedStream::ServerTls(stream) => {
                let (read, write) = tokio::io::split(stream);
                (
                    UnifiedReadHalf::ServerTls(read),
                    UnifiedWriteHalf::ServerTls(write),
                )
            }
        }
    }
}

impl AsyncRead for UnifiedStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            UnifiedStream::Plain(stream) => Pin::new(stream).poll_read(cx, buf),
            UnifiedStream::ClientTls(stream) => Pin::new(stream).poll_read(cx, buf),
            UnifiedStream::ServerTls(stream) => Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for UnifiedStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            UnifiedStream::Plain(stream) => Pin::new(stream).poll_write(cx, buf),
            UnifiedStream::ClientTls(stream) => Pin::new(stream).poll_write(cx, buf),
            UnifiedStream::ServerTls(stream) => Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            UnifiedStream::Plain(stream) => Pin::new(stream).poll_flush(cx),
            UnifiedStream::ClientTls(stream) => Pin::new(stream).poll_flush(cx),
            UnifiedStream::ServerTls(stream) => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            UnifiedStream::Plain(stream) => Pin::new(stream).poll_shutdown(cx),
            UnifiedStream::ClientTls(stream) => Pin::new(stream).poll_shutdown(cx),
            UnifiedStream::ServerTls(stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}

/// Read half of a unified stream.
pub enum UnifiedReadHalf {
    /// Plain TCP read half
    Plain(OwnedReadHalf),
    /// TLS client read half
    ClientTls(tokio::io::ReadHalf<ClientTlsStream<TcpStream>>),
    /// TLS server read half
    ServerTls(tokio::io::ReadHalf<ServerTlsStream<TcpStream>>),
}

impl AsyncRead for UnifiedReadHalf {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            UnifiedReadHalf::Plain(read) => Pin::new(read).poll_read(cx, buf),
            UnifiedReadHalf::ClientTls(read) => Pin::new(read).poll_read(cx, buf),
            UnifiedReadHalf::ServerTls(read) => Pin::new(read).poll_read(cx, buf),
        }
    }
}

/// Write half of a unified stream.
pub enum UnifiedWriteHalf {
    /// Plain TCP write half
    Plain(OwnedWriteHalf),
    /// TLS client write half
    ClientTls(tokio::io::WriteHalf<ClientTlsStream<TcpStream>>),
    /// TLS server write half
    ServerTls(tokio::io::WriteHalf<ServerTlsStream<TcpStream>>),
}

impl AsyncWrite for UnifiedWriteHalf {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            UnifiedWriteHalf::Plain(write) => Pin::new(write).poll_write(cx, buf),
            UnifiedWriteHalf::ClientTls(write) => Pin::new(write).poll_write(cx, buf),
            UnifiedWriteHalf::ServerTls(write) => Pin::new(write).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            UnifiedWriteHalf::Plain(write) => Pin::new(write).poll_flush(cx),
            UnifiedWriteHalf::ClientTls(write) => Pin::new(write).poll_flush(cx),
            UnifiedWriteHalf::ServerTls(write) => Pin::new(write).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            UnifiedWriteHalf::Plain(write) => Pin::new(write).poll_shutdown(cx),
            UnifiedWriteHalf::ClientTls(write) => Pin::new(write).poll_shutdown(cx),
            UnifiedWriteHalf::ServerTls(write) => Pin::new(write).poll_shutdown(cx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unified_stream_variants() {
        // Just ensure the types compile correctly
        fn _assert_send<T: Send>() {}
        fn _assert_sync<T: Sync>() {}

        // UnifiedStream should be Send
        _assert_send::<UnifiedStream>();

        // Read and write halves should be Send
        _assert_send::<UnifiedReadHalf>();
        _assert_send::<UnifiedWriteHalf>();
    }
}

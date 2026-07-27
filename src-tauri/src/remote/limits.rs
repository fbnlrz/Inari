//! What a peer may cost us before it has proved anything.
//!
//! Every check in `server` runs on a request that has already been parsed, and
//! the token check runs later still. The accept loop runs before all of it, so
//! whatever a stranger on the network can make the listener hold onto is held
//! without a token. That matters more here than in a normal web service: the
//! file descriptors and tasks come out of the *desktop app's* budget, so the
//! failure does not land on the remote, it lands on `pactl`, on the PipeWire
//! socket and on the `hidraw` opens the headset needs.
//!
//! Two bounds, both ahead of authentication:
//!
//! * [`Gated`] holds at most [`MAX_CONNECTIONS`] sockets at a time. Past that
//!   it stops calling `accept`, and the rest wait in the kernel's backlog -
//!   which is what a backlog is for. Nothing is accepted only to be dropped.
//! * [`Guarded`] closes a connection that has not moved a byte in either
//!   direction for [`IDLE_TIMEOUT`]. Opening a socket and then saying nothing
//!   is otherwise free and unbounded in time, and it is the whole of a
//!   slowloris.
//!
//! The timeout counts traffic in *both* directions on purpose. A tablet with
//! the mixer open is receiving `levels` at 10 Hz and answering heartbeats
//! every 15 s, so a live remote session never approaches the deadline, while a
//! socket that is merely open never refreshes it.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::extract::connect_info::Connected;
use axum::serve::{IncomingStream, Listener};
use log::debug;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::{Instant, Sleep};

/// Sockets the remote will hold at once, across every client.
///
/// A browser opens a handful in parallel for the bundle and one more for the
/// WebSocket, so this is a dozen tablets' worth. It is a cap on damage, not a
/// capacity plan: the number that matters is that it is finite.
pub const MAX_CONNECTIONS: usize = 64;

/// How long a connection may go without moving a byte before it is closed.
///
/// Comfortably longer than the client's 15 s heartbeat, because browsers
/// throttle timers in a backgrounded tab down to roughly one a minute - and a
/// dropped connection there is only a reconnect, which the transport treats as
/// the normal path anyway.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// How long to wait out an accept error that is about the listener rather than
/// about one connection (the classic being "out of file descriptors"), so the
/// loop cannot spin.
const ACCEPT_BACKOFF: Duration = Duration::from_secs(1);

/// A [`TcpListener`] that will not hold more than [`MAX_CONNECTIONS`] at once.
pub struct Gated {
    inner: TcpListener,
    permits: Arc<Semaphore>,
}

impl Gated {
    pub fn new(inner: TcpListener) -> Self {
        Self::with_capacity(inner, MAX_CONNECTIONS)
    }

    fn with_capacity(inner: TcpListener, capacity: usize) -> Self {
        Self {
            inner,
            permits: Arc::new(Semaphore::new(capacity)),
        }
    }
}

impl Listener for Gated {
    type Io = Guarded<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            // The permit is taken *before* the accept. Taking it afterwards
            // would mean accepting a socket we have no room for and closing
            // it again, which is a worse answer than leaving it queued.
            let permit = Arc::clone(&self.permits)
                .acquire_owned()
                .await
                // The semaphore lives as long as the listener and is never
                // closed, so this cannot fail.
                .expect("the connection semaphore is open for the listener's life");
            match self.inner.accept().await {
                Ok((io, addr)) => return (Guarded::new(io, permit), addr),
                Err(e) => {
                    // Errors about the connection that failed are not worth
                    // waiting on; the ones about the process (descriptors,
                    // memory) are, or this becomes a busy loop.
                    let connection_level = matches!(
                        e.kind(),
                        io::ErrorKind::ConnectionRefused
                            | io::ErrorKind::ConnectionAborted
                            | io::ErrorKind::ConnectionReset
                    );
                    debug!("remote: accept failed: {e}");
                    drop(permit);
                    if !connection_level {
                        tokio::time::sleep(ACCEPT_BACKOFF).await;
                    }
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}

/// One accepted connection, holding its slot and its deadline.
pub struct Guarded<Io> {
    io: Io,
    /// Held for the connection's whole life. Dropping it - which happens when
    /// the connection is dropped, however it ended - is what lets [`Gated`]
    /// accept the next one.
    _permit: OwnedSemaphorePermit,
    idle: Pin<Box<Sleep>>,
    timeout: Duration,
}

impl<Io> Guarded<Io> {
    fn new(io: Io, permit: OwnedSemaphorePermit) -> Self {
        Self::with_timeout(io, permit, IDLE_TIMEOUT)
    }

    fn with_timeout(io: Io, permit: OwnedSemaphorePermit, timeout: Duration) -> Self {
        Self {
            io,
            _permit: permit,
            idle: Box::pin(tokio::time::sleep(timeout)),
            timeout,
        }
    }

    /// Bytes moved: push the deadline out.
    fn touch(&mut self) {
        self.idle.as_mut().reset(Instant::now() + self.timeout);
    }

    /// Is the connection past its deadline? Registers the waker either way, so
    /// a socket that never speaks again is still polled once more when the
    /// deadline lands.
    fn expired(&mut self, cx: &mut Context<'_>) -> bool {
        self.idle.as_mut().poll(cx).is_ready()
    }
}

impl<Io: AsyncRead + Unpin> AsyncRead for Guarded<Io> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let me = self.get_mut();
        let polled = Pin::new(&mut me.io).poll_read(cx, buf);
        match polled {
            Poll::Ready(ready) => {
                me.touch();
                Poll::Ready(ready)
            }
            // Waiting on the peer. This is the only state a socket that is
            // being held rather than used ever sits in, so it is where the
            // deadline is worth checking.
            Poll::Pending if me.expired(cx) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the connection went idle",
            ))),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<Io: AsyncWrite + Unpin> AsyncWrite for Guarded<Io> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let me = self.get_mut();
        let polled = Pin::new(&mut me.io).poll_write(cx, buf);
        if polled.is_ready() {
            me.touch();
        }
        polled
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let me = self.get_mut();
        let polled = Pin::new(&mut me.io).poll_write_vectored(cx, bufs);
        if polled.is_ready() {
            me.touch();
        }
        polled
    }

    fn is_write_vectored(&self) -> bool {
        self.io.is_write_vectored()
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().io).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().io).poll_shutdown(cx)
    }
}

/// The address a connection came from, for the one log line that names it.
///
/// A newtype around `SocketAddr` because the `Connected` impl that carries it
/// into handlers has to name a type this crate owns.
#[derive(Debug, Clone, Copy)]
pub struct Peer(pub SocketAddr);

impl Connected<IncomingStream<'_, Gated>> for Peer {
    fn connect_info(stream: IncomingStream<'_, Gated>) -> Self {
        Self(*stream.remote_addr())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A connected pair: the server side wrapped with `timeout`, and the peer.
    ///
    /// The deadline is injected rather than waited out - what is under test is
    /// that there is one, not how long it is.
    async fn pair(timeout: Duration) -> (Guarded<TcpStream>, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        let addr = listener.local_addr().expect("bound");
        let client = TcpStream::connect(addr).await.expect("connects");
        let (server, _) = listener.accept().await.expect("accepts");
        let permit = Arc::new(Semaphore::new(1))
            .acquire_owned()
            .await
            .expect("a fresh semaphore");
        (Guarded::with_timeout(server, permit, timeout), client)
    }

    #[tokio::test]
    async fn a_socket_that_says_nothing_is_closed_rather_than_held() {
        let (mut guarded, mut client) = pair(Duration::from_millis(50)).await;

        // The peer connected and then went quiet, which is the whole attack.
        let mut buf = [0u8; 8];
        let read = guarded.read(&mut buf).await;
        assert_eq!(
            read.expect_err("an idle peer is not a peer").kind(),
            io::ErrorKind::TimedOut
        );
        let _ = client.shutdown().await;
    }

    #[tokio::test]
    async fn traffic_keeps_a_connection_alive_past_the_deadline() {
        let (mut guarded, mut client) = pair(Duration::from_millis(80)).await;

        // Three rounds, each past a third of the deadline: a client that keeps
        // talking must never be cut off, or every tablet would drop mid-drag.
        for _ in 0..3 {
            tokio::time::sleep(Duration::from_millis(40)).await;
            client.write_all(b"ping").await.expect("writes");
            let mut buf = [0u8; 4];
            guarded.read_exact(&mut buf).await.expect("still connected");
        }
    }

    #[tokio::test]
    async fn the_listener_stops_accepting_at_its_cap() {
        let inner = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        let addr = inner.local_addr().expect("bound");
        let mut gated = Gated::with_capacity(inner, 1);
        let _first = TcpStream::connect(addr).await.expect("connects");
        let _second = TcpStream::connect(addr).await.expect("connects");

        let held = gated.accept().await;
        // The second socket is queued in the kernel, not accepted: with the
        // one permit taken there is nothing to hand it.
        let blocked = tokio::time::timeout(Duration::from_millis(100), gated.accept()).await;
        assert!(blocked.is_err(), "accepted past the cap");

        // Letting the first connection go releases its slot.
        drop(held);
        let freed = tokio::time::timeout(Duration::from_millis(500), gated.accept()).await;
        assert!(freed.is_ok(), "a finished connection must free its slot");
    }
}

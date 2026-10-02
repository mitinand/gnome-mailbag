// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{Encryption, ImapFailure, ImapStep};
use futures_util::io::{AsyncRead, AsyncWrite};
use gio::prelude::*;
use glib::thread_guard::ThreadGuard;
use std::{
    cell::RefCell,
    error::Error,
    fmt, io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
};

/// The TCP connection to the server. Dropping it closes the socket at once,
/// even while a read is pending, so a failed or cancelled attempt never
/// leaves its connection open.
pub(crate) struct ServerConnection(gio::SocketConnection);

impl ServerConnection {
    pub(crate) fn close(&self) {
        // Closing an already closed socket is harmless.
        let _ = self.0.socket().close();
    }
}

impl Drop for ServerConnection {
    fn drop(&mut self) {
        self.close();
    }
}

/// Opens the TCP connection. GIO's socket timeout also bounds TLS and every
/// later read and write that makes no progress.
pub(crate) async fn connect(
    host: &str,
    encryption: Encryption,
    socket_timeout_seconds: u32,
) -> Result<(ServerConnection, gio::NetworkAddress), ImapFailure> {
    let default_port = match encryption {
        Encryption::ImplicitTls => 993,
        Encryption::StartTls => 143,
    };
    let address = gio::NetworkAddress::parse(host, default_port)
        .map_err(|_| ImapFailure::Failed(ImapStep::Connect))?;
    let client = gio::SocketClient::new();
    client.set_timeout(socket_timeout_seconds);
    // Written before the attempt, so a connection that fails still names where
    // it went.
    tracing::debug!(
        host = address.hostname().as_str(),
        port = address.port(),
        "connecting"
    );
    let connection = client
        .connect_future(&address)
        .await
        .map_err(|error| step_failure(ImapStep::Connect, &error))?;
    tracing::info!("connected");
    Ok((ServerConnection(connection), address))
}

/// Performs a TLS handshake verified against GIO's default certificate
/// database. No accept-certificate handler is ever connected.
pub(crate) async fn start_tls(
    connection: &ServerConnection,
    identity: &gio::NetworkAddress,
    encryption: Encryption,
) -> Result<gio::IOStream, ImapFailure> {
    let secure_connection = |error: glib::Error| step_failure(ImapStep::SecureConnection, &error);
    let tls =
        gio::TlsClientConnection::new(&connection.0, Some(identity)).map_err(secure_connection)?;
    if let Err(error) = tls.handshake_future(glib::Priority::DEFAULT).await {
        // The TLS library's fixed phrases, such as "An unexpected TLS packet
        // was received" for a port that expects STARTTLS; its code alone is
        // `Misc` there (specs/003-logging/research.md §7).
        let certificate_errors = tls.peer_certificate_errors();
        tracing::debug!(
            tls_error = error.message(),
            certificate_errors =
                (!certificate_errors.is_empty()).then(|| tracing::field::debug(certificate_errors)),
            "TLS handshake failed"
        );
        return Err(secure_connection(error));
    }
    tracing::info!(?encryption, tls = ?tls.protocol_version(), "connection secured");
    Ok(tls.upcast())
}

/// The unencrypted stream, used only until STARTTLS succeeds.
pub(crate) fn plaintext_stream(connection: &ServerConnection) -> gio::IOStream {
    connection.0.clone().upcast()
}

fn step_failure(step: ImapStep, error: &glib::Error) -> ImapFailure {
    if error.matches(gio::IOErrorEnum::TimedOut) {
        ImapFailure::TimedOut(step)
    } else {
        ImapFailure::Failed(step)
    }
}

/// A GIO stream as the futures-io stream that async-imap reads and writes.
/// Two handles share it: the session's, and the one that puts compression
/// in between once the server has agreed to it (`compress`), while the
/// session keeps reading and writing through its own handle.
///
/// `ThreadGuard` meets async-imap's `Send` bound without making the stream
/// usable elsewhere: it is created, polled and dropped on the mail worker.
pub(crate) struct GioStream {
    stream: ThreadGuard<Rc<RefCell<gio::IOStreamAsyncReadWrite<gio::IOStream>>>>,
    /// The secured stream underneath, which compression wraps.
    secured: ThreadGuard<gio::IOStream>,
}

impl GioStream {
    pub(crate) fn new(secured: gio::IOStream) -> Self {
        Self {
            stream: ThreadGuard::new(Rc::new(RefCell::new(pollable(secured.clone())))),
            secured: ThreadGuard::new(secured),
        }
    }

    /// Another handle on the same stream.
    pub(crate) fn share(&self) -> Self {
        Self {
            stream: ThreadGuard::new(Rc::clone(self.stream.get_ref())),
            secured: ThreadGuard::new(self.secured.get_ref().clone()),
        }
    }

    /// Puts DEFLATE compression between the session and the secured stream,
    /// once the server has agreed to `COMPRESS DEFLATE` (RFC 4978): raw
    /// deflate without the zlib header, as the extension defines. From here
    /// on every command the session flushes reaches the server compressed,
    /// and every byte read is decompressed first.
    pub(crate) fn compress(&self) {
        let secured = self.secured.get_ref();
        let decompressor = gio::ZlibDecompressor::new(gio::ZlibCompressorFormat::Raw);
        let compressor = gio::ZlibCompressor::new(gio::ZlibCompressorFormat::Raw, -1);
        let input = gio::ConverterInputStream::new(&secured.input_stream(), &decompressor);
        let output = gio::ConverterOutputStream::new(&secured.output_stream(), &compressor);
        let compressed = gio::SimpleIOStream::new(&input, &output);
        *self.stream.get_ref().borrow_mut() = pollable(compressed.upcast());
    }
}

/// The stream as futures-io reads and writes it.
fn pollable(stream: gio::IOStream) -> gio::IOStreamAsyncReadWrite<gio::IOStream> {
    stream
        .into_async_read_write()
        .expect("GIO socket, TLS and converter streams are pollable")
}

impl fmt::Debug for GioStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GioStream")
    }
}

impl AsyncRead for GioStream {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let mut stream = self.stream.get_ref().borrow_mut();
        Pin::new(&mut *stream)
            .poll_read(context, buffer)
            .map_err(mark_transport_error)
    }
}

impl AsyncWrite for GioStream {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut stream = self.stream.get_ref().borrow_mut();
        Pin::new(&mut *stream)
            .poll_write(context, buffer)
            .map_err(mark_transport_error)
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut stream = self.stream.get_ref().borrow_mut();
        Pin::new(&mut *stream)
            .poll_flush(context)
            .map_err(mark_transport_error)
    }

    fn poll_close(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut stream = self.stream.get_ref().borrow_mut();
        Pin::new(&mut *stream)
            .poll_close(context)
            .map_err(mark_transport_error)
    }
}

/// Marks an I/O error from the network, as opposed to one from async-imap's
/// parser, so its origin survives async-imap's conversion to `io::Error`.
#[derive(Debug)]
struct TransportError;

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("network input or output failed")
    }
}

impl Error for TransportError {}

/// Keeps the error kind, so a GIO timeout stays `TimedOut`.
fn mark_transport_error(error: io::Error) -> io::Error {
    io::Error::new(error.kind(), TransportError)
}

pub(crate) fn is_transport_error(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|source| source.is::<TransportError>())
}

// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{Encryption, ImapFailure, ImapStep};
use futures_util::io::{AsyncRead, AsyncWrite};
use gio::prelude::*;
use glib::thread_guard::ThreadGuard;
use std::{
    error::Error,
    fmt, io,
    pin::Pin,
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
///
/// `ThreadGuard` meets async-imap's `Send` bound without making the stream
/// usable elsewhere: it is created, polled and dropped on the mail worker.
pub(crate) struct GioStream(ThreadGuard<gio::IOStreamAsyncReadWrite<gio::IOStream>>);

impl GioStream {
    pub(crate) fn new(stream: gio::IOStream) -> Self {
        let stream = stream
            .into_async_read_write()
            .expect("GIO socket and TLS streams are pollable");
        Self(ThreadGuard::new(stream))
    }
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
        Pin::new(self.get_mut().0.get_mut())
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
        Pin::new(self.get_mut().0.get_mut())
            .poll_write(context, buffer)
            .map_err(mark_transport_error)
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(self.get_mut().0.get_mut())
            .poll_flush(context)
            .map_err(mark_transport_error)
    }

    fn poll_close(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(self.get_mut().0.get_mut())
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

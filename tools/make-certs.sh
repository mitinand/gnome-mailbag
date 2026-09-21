#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Andrey Mitin
# SPDX-License-Identifier: GPL-3.0-or-later
# Generates disposable certificates for the scripted IMAP server in
# target/test-certs. Nothing is installed into system trust.
set -euo pipefail
cd "$(dirname "$0")/.."
if ! command -v openssl >/dev/null; then
    echo 'openssl is required to generate test certificates; install the openssl package.' >&2
    exit 1
fi
directory=target/test-certs
mkdir -p "$directory"
cd "$directory"

# name, common name
make_ca() {
    openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 3650 \
        -subj "/CN=$2" -addext 'basicConstraints=critical,CA:TRUE' \
        -addext 'keyUsage=critical,keyCertSign,cRLSign' \
        -keyout "$1.key" -out "$1.pem" 2>/dev/null
}

# name, issuing CA, subjectAltName, validity options
make_server_certificate() {
    openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes \
        -subj "/CN=$1" -keyout "$1.key" -out "$1.csr" 2>/dev/null
    printf 'subjectAltName=%s\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:FALSE\n' "$3" >"$1.ext"
    openssl x509 -req -in "$1.csr" -CA "$2.pem" -CAkey "$2.key" -CAcreateserial \
        "${@:4}" -extfile "$1.ext" -out "$1.pem" 2>/dev/null
    rm -f "$1.csr" "$1.ext"
}

make_ca ca 'Mailbag IMAP test CA'
make_ca other-ca 'Mailbag IMAP untrusted test CA'
make_server_certificate localhost ca 'DNS:localhost,IP:127.0.0.1' -days 3650
make_server_certificate unknown-ca other-ca 'DNS:localhost,IP:127.0.0.1' -days 3650
make_server_certificate wrong-host ca 'DNS:wrong-host.invalid' -days 3650
make_server_certificate expired ca 'DNS:localhost,IP:127.0.0.1' \
    -not_before 20200101000000Z -not_after 20200102000000Z
rm -f ./*.srl
echo "Test certificates written to $directory"

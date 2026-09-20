// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{expect_failure, open_reader, plain_messages, run};
use crate::{
    Encryption, ImapAccount, ImapFailure, ImapStep, InboxReader, ServerReply,
    test_server::{FixtureSetup, ImapFixture, StartTlsBehavior},
};

fn starttls(behavior: StartTlsBehavior) -> FixtureSetup {
    FixtureSetup {
        encryption: Encryption::StartTls,
        starttls: behavior,
        ..FixtureSetup::default()
    }
}

#[test]
fn both_encryption_modes_sign_in_and_examine_the_inbox() {
    for encryption in [Encryption::ImplicitTls, Encryption::StartTls] {
        let fixture = ImapFixture::start(FixtureSetup {
            encryption,
            messages: plain_messages(3),
            ..FixtureSetup::default()
        });
        drop(open_reader(&fixture));
        let log = fixture.log();
        let expected: &[&str] = match encryption {
            Encryption::ImplicitTls => &["CAPABILITY", "AUTHENTICATE", "EXAMINE"],
            Encryption::StartTls => &[
                "plaintext CAPABILITY",
                "plaintext STARTTLS",
                "CAPABILITY",
                "AUTHENTICATE",
                "EXAMINE",
            ],
        };
        assert_eq!(log.commands, expected);
        assert_eq!(log.credentials_received, 1);
    }
}

#[test]
fn an_interrupted_examine_does_not_confirm_an_empty_inbox() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(1),
        close_during_examine: true,
        ..FixtureSetup::default()
    });
    let error = expect_failure(run(InboxReader::open(fixture.account())));
    assert_eq!(error.failure, ImapFailure::Failed(ImapStep::OpenInbox));
    assert!(fixture.log().fetches.is_empty());
}

#[test]
fn capability_alerts_explain_a_rejected_command_or_missing_sign_in_method() {
    for (reply, failure) in [
        (
            "{tag} NO [UNAVAILABLE] Capabilities unavailable\r\n",
            ImapFailure::Failed(ImapStep::SignIn),
        ),
        (
            "* CAPABILITY IMAP4rev1 LOGINDISABLED\r\n{tag} OK done\r\n",
            ImapFailure::NoSignInMethod,
        ),
    ] {
        let fixture = ImapFixture::start(FixtureSetup {
            capability_reply: Some(format!("* OK [ALERT] Maintenance tonight\r\n{reply}")),
            ..FixtureSetup::default()
        });
        let error = expect_failure(run(InboxReader::open(fixture.account())));
        assert_eq!(error.failure, failure);
        assert_eq!(error.alerts, ["Maintenance tonight"]);
        assert_eq!(fixture.log().credentials_received, 0);
        assert_eq!(fixture.log().commands, ["CAPABILITY"]);
    }
}

#[test]
fn missing_rejected_or_preauth_starttls_fails_before_credentials() {
    for behavior in [
        StartTlsBehavior::NotOffered,
        StartTlsBehavior::Rejected,
        StartTlsBehavior::PreauthGreeting,
    ] {
        let fixture = ImapFixture::start(starttls(behavior));
        let error = expect_failure(run(InboxReader::open(fixture.account())));
        assert_eq!(
            error.failure,
            ImapFailure::Failed(ImapStep::SecureConnection),
            "{behavior:?}"
        );
        assert_eq!(fixture.log().credentials_received, 0, "{behavior:?}");
    }
}

#[test]
fn plaintext_sent_after_the_starttls_reply_is_discarded() {
    let fixture = ImapFixture::start(FixtureSetup {
        rejection: "{tag} NO [ALERT] Rejected over TLS\r\n".to_owned(),
        ..starttls(StartTlsBehavior::InjectAfterReply)
    });
    let account = fixture.account_with_password("wrong password");
    let error = expect_failure(run(InboxReader::open(account)));
    // The sign-in happened over TLS; the injected ALERT was never read.
    assert_eq!(error.failure, ImapFailure::Failed(ImapStep::SignIn));
    assert_eq!(error.alerts, ["Rejected over TLS"]);
}

#[test]
fn certificate_failures_stop_before_credentials() {
    for encryption in [Encryption::ImplicitTls, Encryption::StartTls] {
        for certificate in ["unknown-ca", "wrong-host", "expired"] {
            let fixture = ImapFixture::start(FixtureSetup {
                encryption,
                certificate,
                ..FixtureSetup::default()
            });
            let error = expect_failure(run(InboxReader::open(fixture.account())));
            assert_eq!(
                error.failure,
                ImapFailure::Failed(ImapStep::SecureConnection),
                "{certificate} with {encryption:?}"
            );
            assert_eq!(fixture.log().credentials_received, 0);
        }
    }
}

#[test]
fn plain_sign_in_carries_non_ascii_credentials() {
    let (login, password) = ("пользователь", "пароль-ü");
    let fixture = ImapFixture::start(FixtureSetup {
        credentials: Some((login.to_owned(), password.to_owned())),
        ..FixtureSetup::default()
    });
    let account = ImapAccount {
        host: format!("localhost:{}", fixture.port()),
        login: login.to_owned(),
        password: password.to_owned(),
        encryption: Encryption::ImplicitTls,
    };
    // The server accepts only the exact credentials.
    drop(super::expect_success(run(InboxReader::open(account))));
    assert!(fixture.log().commands.contains(&"AUTHENTICATE".to_owned()));
}

#[test]
fn login_is_used_only_without_plain() {
    let fixture = ImapFixture::start(FixtureSetup {
        offers_plain: false,
        ..FixtureSetup::default()
    });
    drop(open_reader(&fixture));
    let commands = fixture.log().commands;
    assert!(commands.contains(&"LOGIN".to_owned()));
    assert!(!commands.contains(&"AUTHENTICATE".to_owned()));
}

#[test]
fn a_rejected_sign_in_is_not_retried_with_another_method() {
    let fixture = ImapFixture::start(FixtureSetup::default());
    let account = fixture.account_with_password("wrong password");
    let error = expect_failure(run(InboxReader::open(account)));
    assert_eq!(error.failure, ImapFailure::Failed(ImapStep::SignIn));
    let log = fixture.log();
    assert_eq!(log.commands, ["CAPABILITY", "AUTHENTICATE"]);
    assert_eq!(log.credentials_received, 1);
}

#[test]
fn login_sends_non_ascii_credentials_as_literals() {
    let (login, password) = ("пользователь", "пароль");
    let fixture = ImapFixture::start(FixtureSetup {
        offers_plain: false,
        credentials: Some((login.to_owned(), password.to_owned())),
        ..FixtureSetup::default()
    });
    let account = ImapAccount {
        host: format!("localhost:{}", fixture.port()),
        login: login.to_owned(),
        password: password.to_owned(),
        encryption: Encryption::ImplicitTls,
    };
    // The server accepts only the exact credentials.
    drop(super::expect_success(run(InboxReader::open(account))));
    assert!(fixture.log().commands.contains(&"LOGIN".to_owned()));
}

#[test]
fn a_rejected_sign_in_keeps_the_server_text_and_code() {
    for (rejection, code, text) in [
        (
            "{tag} NO [AUTHENTICATIONFAILED] Invalid credentials\r\n",
            "AUTHENTICATIONFAILED",
            "Invalid credentials",
        ),
        // A temporary server problem, where the password is not at fault.
        (
            "{tag} NO [UNAVAILABLE] Authentication backend is down\r\n",
            "UNAVAILABLE",
            "Authentication backend is down",
        ),
    ] {
        let fixture = ImapFixture::start(FixtureSetup {
            rejection: rejection.to_owned(),
            ..FixtureSetup::default()
        });
        let account = fixture.account_with_password("wrong password");
        let error = expect_failure(run(InboxReader::open(account)));
        assert_eq!(error.failure, ImapFailure::Failed(ImapStep::SignIn));
        assert_eq!(
            error.server_reply,
            Some(ServerReply {
                code: Some(code.to_owned()),
                text: text.to_owned(),
            })
        );
    }
}

#[test]
fn a_bye_greeting_keeps_the_server_text() {
    let text = "Maximum number of connections from user+IP exceeded";
    let fixture = ImapFixture::start(FixtureSetup {
        greeting: format!("* BYE {text}"),
        ..FixtureSetup::default()
    });
    let error = expect_failure(run(InboxReader::open(fixture.account())));
    assert_eq!(error.failure, ImapFailure::Failed(ImapStep::Connect));
    assert_eq!(
        error.server_reply,
        Some(ServerReply {
            code: None,
            text: text.to_owned(),
        })
    );
}

#[test]
fn login_disabled_without_plain_leaves_no_sign_in_method() {
    let fixture = ImapFixture::start(FixtureSetup {
        offers_plain: false,
        login_disabled: true,
        ..FixtureSetup::default()
    });
    let error = expect_failure(run(InboxReader::open(fixture.account())));
    assert_eq!(error.failure, ImapFailure::NoSignInMethod);
    assert_eq!(fixture.log().credentials_received, 0);
}

#[test]
fn utf8_texts_and_alerts_of_a_rejected_sign_in_are_kept() {
    let fixture = ImapFixture::start(FixtureSetup {
        greeting: "* OK [ALERT] Обслуживание ночью – Dovecot ready".to_owned(),
        rejection: "* OK [ALERT] Слишком много попыток\r\n\
                    {tag} NO [ALERT] Нужен пароль приложения\r\n"
            .to_owned(),
        ..FixtureSetup::default()
    });
    let account = fixture.account_with_password("wrong password");
    let error = expect_failure(run(InboxReader::open(account)));
    assert_eq!(error.failure, ImapFailure::Failed(ImapStep::SignIn));
    assert_eq!(
        error.server_reply,
        Some(ServerReply {
            code: Some("ALERT".to_owned()),
            text: "Нужен пароль приложения".to_owned(),
        })
    );
    assert_eq!(
        error.alerts,
        [
            "Обслуживание ночью – Dovecot ready",
            "Слишком много попыток",
            "Нужен пароль приложения",
        ]
    );
}

/// A server may announce maintenance before it asks for the credentials.
#[test]
fn a_notice_before_the_sign_in_request_does_not_stop_it() {
    let fixture = ImapFixture::start(FixtureSetup {
        notice_before_sign_in: Some("* OK [ALERT] Maintenance tonight".to_owned()),
        messages: plain_messages(1),
        ..FixtureSetup::default()
    });
    open_reader(&fixture);
    assert_eq!(fixture.log().credentials_received, 1);
}

/// Capability names are atoms, which a server may write in any case.
#[test]
fn capability_names_are_read_in_any_case() {
    let starttls_fixture = ImapFixture::start(FixtureSetup {
        lowercase_protocol_names: true,
        messages: plain_messages(1),
        ..starttls(StartTlsBehavior::Offered)
    });
    open_reader(&starttls_fixture);
    assert_eq!(starttls_fixture.log().credentials_received, 1);

    let login_disabled = ImapFixture::start(FixtureSetup {
        lowercase_protocol_names: true,
        offers_plain: false,
        login_disabled: true,
        messages: plain_messages(1),
        ..FixtureSetup::default()
    });
    let error = expect_failure(run(InboxReader::open(login_disabled.account())));
    assert_eq!(error.failure, ImapFailure::NoSignInMethod);
    assert_eq!(login_disabled.log().credentials_received, 0);
}

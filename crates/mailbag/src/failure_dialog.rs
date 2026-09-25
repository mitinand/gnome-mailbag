// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shows a declared failure: the failure dialog behind every Details and
//! banner button, and the parts the status pages share with it
//! (specs/006-error-handling/contracts/failure-declaration.md).

#[cfg(test)]
mod tests;

use crate::mail_ui::{inert_text, show_inert_text};
use adw::{glib, gtk, prelude::*};
use mailbag_providers::{DeclaredFailure, FailureAction};

/// The widgets of one failure dialog, filled from a declaration.
pub(crate) struct FailureDialogWidgets {
    pub(crate) dialog: adw::Dialog,
    pub(crate) explanation: gtk::Label,
    pub(crate) advice: gtk::Label,
    /// One block per remote text, then the technical details.
    pub(crate) blocks: gtk::Box,
    pub(crate) action: gtk::Button,
    pub(crate) copy_button: gtk::Button,
}

/// The label and the action name of a failure's action button; the one place
/// that turns an action into a button.
pub fn action_button(action: FailureAction) -> (&'static str, &'static str) {
    match action {
        FailureAction::Retry => ("Retry", "app.refresh-inbox"),
        FailureAction::OnlineAccounts => ("Online Accounts", "app.accounts"),
    }
}

/// Gives a form's action button the declared action, or hides it.
pub fn show_action_button(button: &gtk::Button, action: Option<FailureAction>) {
    let Some(action) = action else {
        button.set_visible(false);
        return;
    };
    let (label, action_name) = action_button(action);
    button.set_label(label);
    button.set_action_name(Some(action_name));
    button.set_sensitive(true);
    button.set_visible(true);
}

/// A status page's description: the explanation and the advice as two
/// paragraphs, escaped, because a status page reads its description as markup.
pub fn status_description(failure: &DeclaredFailure) -> String {
    let paragraphs: Vec<&str> = [failure.explanation.as_str()]
        .into_iter()
        .chain(failure.advice)
        .collect();
    glib::markup_escape_text(&inert_text(&paragraphs.join("\n\n"))).to_string()
}

/// Opens the failure dialog over the window that holds `parent`.
pub fn present(parent: &impl IsA<gtk::Widget>, failure: &DeclaredFailure) {
    let widgets = build(failure);
    let clipboard = parent.clipboard();
    let copied_text = report_text(failure);
    widgets
        .copy_button
        .connect_clicked(move |_| clipboard.set_text(&copied_text));
    widgets.dialog.present(Some(parent));
}

/// Builds the dialog from its form and fills it in the order the spec gives:
/// explanation, advice, remote texts, technical details, action.
pub(crate) fn build(failure: &DeclaredFailure) -> FailureDialogWidgets {
    let builder = gtk::Builder::from_string(include_str!("../resources/ui/failure-dialog.ui"));
    let widgets = FailureDialogWidgets {
        dialog: builder
            .object("failure_dialog")
            .expect("failure-dialog.ui: failure_dialog"),
        explanation: builder
            .object("dialog_explanation")
            .expect("failure-dialog.ui: dialog_explanation"),
        advice: builder
            .object("dialog_advice")
            .expect("failure-dialog.ui: dialog_advice"),
        blocks: builder
            .object("dialog_blocks")
            .expect("failure-dialog.ui: dialog_blocks"),
        action: builder
            .object("dialog_action")
            .expect("failure-dialog.ui: dialog_action"),
        copy_button: builder
            .object("copy_button")
            .expect("failure-dialog.ui: copy_button"),
    };
    widgets.dialog.set_title(failure.title);
    show_paragraph(&widgets.explanation, &failure.explanation);
    show_paragraph(&widgets.advice, failure.advice.unwrap_or_default());
    for remote_text in &failure.remote_texts {
        widgets
            .blocks
            .append(&build_block(remote_text.source, &remote_text.text));
    }
    if !failure.details.is_empty() {
        widgets
            .blocks
            .append(&build_block("Technical details", &failure.details));
    }
    show_action_button(&widgets.action, failure.action);
    let dialog = widgets.dialog.clone();
    widgets.action.connect_clicked(move |_| {
        dialog.close();
    });
    widgets
}

/// What the copy button puts on the clipboard: the dialog's text in its
/// order, one snippet for an issue report.
pub fn report_text(failure: &DeclaredFailure) -> String {
    let mut parts = vec![
        failure.title.to_owned(),
        failure.explanation.clone(),
        failure.advice.unwrap_or_default().to_owned(),
    ];
    parts.extend(
        failure
            .remote_texts
            .iter()
            .map(|remote_text| format!("{}:\n{}", remote_text.source, remote_text.text)),
    );
    if !failure.details.is_empty() {
        parts.push(format!("Technical details:\n{}", failure.details));
    }
    parts.retain(|part| !part.is_empty());
    inert_text(&parts.join("\n\n"))
}

fn show_paragraph(label: &gtk::Label, text: &str) {
    show_inert_text(label, &inert_text(text));
    label.set_visible(!text.is_empty());
}

/// One block of the dialog from its form: a heading and selectable text.
fn build_block(heading: &str, text: &str) -> gtk::Box {
    let builder = gtk::Builder::from_string(include_str!("../resources/ui/failure-block.ui"));
    builder
        .object::<gtk::Label>("block_heading")
        .expect("failure-block.ui: block_heading")
        .set_text(heading);
    show_inert_text(
        &builder
            .object::<gtk::Label>("block_text")
            .expect("failure-block.ui: block_text"),
        &inert_text(text),
    );
    builder.object("block").expect("failure-block.ui: block")
}

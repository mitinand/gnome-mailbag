// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shows a declared failure: the failure dialog behind every Details and
//! banner button, and the parts the status pages share with it
//! (specs/006-error-handling/contracts/failure-declaration.md).

#[cfg(test)]
mod tests;

use crate::failure_declarations::{DeclaredFailure, FailureAction, remote_heading};
use crate::mail_ui::{cut_unbroken_runs, inert_text, show_inert_text};
use adw::{glib, gtk, prelude::*};

/// Gives a form's action button the declared action, or hides it. The one
/// place that turns an action into a label and an action name. Retry runs
/// Refresh Inbox because every failure shown today is a load's.
pub fn show_action_button(button: &gtk::Button, action: Option<FailureAction>) {
    let Some(action) = action else {
        button.set_visible(false);
        return;
    };
    let (label, action_name) = match action {
        FailureAction::Retry => ("Retry", "app.refresh-inbox"),
        FailureAction::OnlineAccounts => ("Online Accounts", "app.accounts"),
    };
    button.set_label(label);
    button.set_action_name(Some(action_name));
    button.set_visible(true);
}

/// A status page's description: the explanation and the advice as two
/// paragraphs, escaped, because a status page reads its description as markup.
pub fn status_description(failure: &DeclaredFailure) -> String {
    let paragraphs: Vec<&str> = [failure.explanation.as_str()]
        .into_iter()
        .chain(failure.advice)
        .collect();
    let description = cut_unbroken_runs(&inert_text(&paragraphs.join("\n\n")));
    glib::markup_escape_text(&description).to_string()
}

/// Opens the failure dialog over the window that holds `parent`, filled from
/// its form in the order the spec gives: explanation, advice, remote texts,
/// technical details, action.
pub fn present(parent: &impl IsA<gtk::Widget>, failure: &DeclaredFailure) {
    let builder = gtk::Builder::from_string(include_str!("../resources/ui/failure-dialog.ui"));
    let dialog: adw::Dialog = builder
        .object("failure_dialog")
        .expect("failure-dialog.ui: failure_dialog");
    let explanation: gtk::Label = builder
        .object("dialog_explanation")
        .expect("failure-dialog.ui: dialog_explanation");
    let advice: gtk::Label = builder
        .object("dialog_advice")
        .expect("failure-dialog.ui: dialog_advice");
    let blocks: gtk::Box = builder
        .object("dialog_blocks")
        .expect("failure-dialog.ui: dialog_blocks");
    let action: gtk::Button = builder
        .object("dialog_action")
        .expect("failure-dialog.ui: dialog_action");
    let copy_button: gtk::Button = builder
        .object("copy_button")
        .expect("failure-dialog.ui: copy_button");

    dialog.set_title(failure.title);
    show_paragraph(&explanation, &failure.explanation);
    show_paragraph(&advice, failure.advice.unwrap_or_default());
    append_blocks(&blocks, failure);
    show_action_button(&action, failure.action);
    // Weak, because the dialog owns the button that owns this handler.
    let closing = dialog.downgrade();
    action.connect_clicked(move |_| {
        if let Some(dialog) = closing.upgrade() {
            dialog.close();
        }
    });
    let clipboard = parent.clipboard();
    let copied_text = report_text(failure);
    copy_button.connect_clicked(move |_| clipboard.set_text(&copied_text));
    dialog.present(Some(parent));
}

/// What the copy button puts on the clipboard: the dialog's text in its
/// order, one snippet for an issue report.
pub fn report_text(failure: &DeclaredFailure) -> String {
    let mut parts = vec![
        failure.title.to_owned(),
        failure.explanation.clone(),
        failure.advice.unwrap_or_default().to_owned(),
    ];
    parts.extend(failure.remote_texts.iter().map(|remote_text| {
        let heading = remote_heading(remote_text.source);
        format!("{heading}:\n{}", remote_text.text)
    }));
    if !failure.details.is_empty() {
        parts.push(format!("Technical details:\n{}", failure.details));
    }
    parts.retain(|part| !part.is_empty());
    // Each part is bounded as its block is in the dialog, so a long server
    // text never pushes the later blocks out of the report.
    parts
        .iter()
        .map(|part| inert_text(part))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn show_paragraph(label: &gtk::Label, text: &str) {
    show_inert_text(label, &inert_text(text));
    label.set_visible(!text.is_empty());
}

/// One block per remote text, then the technical details.
fn append_blocks(blocks: &gtk::Box, failure: &DeclaredFailure) {
    for remote_text in &failure.remote_texts {
        blocks.append(&build_block(
            remote_heading(remote_text.source),
            &remote_text.text,
        ));
    }
    if !failure.details.is_empty() {
        blocks.append(&build_block("Technical details", &failure.details));
    }
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

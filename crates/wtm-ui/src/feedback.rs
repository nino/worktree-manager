//! The Feedback window: a bug report, a feature request or a question,
//! written here and sent to the developer as a PostHog event, where the
//! daily look at PostHog picks it up.
//!
//! It is sent whether or not error reports are on: pressing Send is consent
//! for that one message (see `wtm_core::telemetry`). The window stays open
//! while it sends and keeps the draft if sending fails; once sent it closes
//! and the notice bar says so.

use wtm_core::telemetry::{Feedback, FeedbackKind};
use wtm_toolkit::{
    Align, Button, ButtonKind, Choice, Field, Form, Ink, Panel, Stack, Text, TextArea, TextStyle,
    ViewCx,
};

use crate::Msg;

pub const KEY: &str = "feedback";

/// What the popup offers, in order.
pub const KINDS: [(FeedbackKind, &str); 4] = [
    (FeedbackKind::Bug, "Bug report"),
    (FeedbackKind::Feature, "Feature request"),
    (FeedbackKind::Question, "Question"),
    (FeedbackKind::Other, "Something else"),
];

/// Said in the notice bar once a message has gone.
pub const THANKS: &str = "Thanks! Your feedback was sent.";

#[derive(Debug, Clone)]
pub enum FeedbackMsg {
    Kind(usize),
    Message(String),
    Email(String),
    Send,
    /// How the send with this number went.
    Sent(u64, Result<(), String>),
    Closed,
}

#[derive(Debug, Default)]
pub struct FeedbackForm {
    pub kind: usize,
    pub message: String,
    pub email: String,
    /// The send on its way, by number, and what it carries; Send is off
    /// until it lands.
    pub sending: Option<(u64, Feedback)>,
    /// Why the last send failed.
    pub error: Option<String>,
    /// Something else worth saying under the form.
    pub note: Option<String>,
}

/// Said under the form when a send lands after the draft changed: a
/// backend never disables a field being typed in, so typing can go on.
pub const EDITED_WHILE_SENDING: &str =
    "Sent. What you changed while it was sending was not; press Send to send it as well.";

/// What a send's answer meant to the window that is open.
#[derive(Debug, PartialEq, Eq)]
pub enum Landed {
    /// This window's message went: close it.
    Sent,
    /// It went, but the draft has changed since; the window stays.
    EditedSince,
    /// It failed; the window says why.
    Failed,
    /// Not this window's send (it was closed and opened again since).
    NotMine,
}

impl FeedbackForm {
    /// Take in the answer to send `id`.
    pub fn landed(&mut self, id: u64, result: &Result<(), String>) -> Landed {
        let Some((sent_id, sent)) = self.sending.take() else {
            return Landed::NotMine;
        };
        if sent_id != id {
            self.sending = Some((sent_id, sent));
            return Landed::NotMine;
        }
        match result {
            Err(e) => {
                self.error = Some(e.clone());
                Landed::Failed
            }
            Ok(()) if sent == self.feedback() => Landed::Sent,
            Ok(()) => {
                self.note = Some(EDITED_WHILE_SENDING.into());
                Landed::EditedSince
            }
        }
    }

    pub fn can_send(&self) -> bool {
        self.sending.is_none() && !self.message.trim().is_empty()
    }

    pub fn feedback(&self) -> Feedback {
        Feedback {
            kind: KINDS.get(self.kind).map_or(FeedbackKind::Other, |k| k.0),
            message: self.message.clone(),
            email: self.email.clone(),
        }
    }

    pub fn view(&self, v: &ViewCx<Msg>) -> Panel {
        let sending = self.sending.is_some();
        let status = match (&self.error, &self.note) {
            _ if sending => Text::new("Sending…").ink(Ink::Secondary),
            (Some(e), _) => Text::new(e.clone()).ink(Ink::Error),
            (None, Some(n)) => Text::new(n.clone()).ink(Ink::Secondary),
            (None, None) => Text::new(""),
        }
        .id("status")
        .style(TextStyle::Small)
        .wrap()
        .lines(3)
        .selectable()
        .grow();
        let body = Form::default()
            .row(
                "Kind:",
                Choice {
                    id: "kind",
                    options: KINDS.iter().map(|(_, t)| t.to_string()).collect(),
                    selected: self.kind,
                    a11y_label: Some("Kind of feedback".into()),
                    on_select: v.map(|i| Msg::Feedback(FeedbackMsg::Kind(i))),
                },
            )
            .row(
                "Message:",
                TextArea {
                    id: "message",
                    value: self.message.clone(),
                    lines: 8,
                    enabled: !sending,
                    on_change: v.map(|s| Msg::Feedback(FeedbackMsg::Message(s))),
                },
            )
            .hint("What happened, what you expected, or what you would like the app to do.")
            .row(
                "Email:",
                Field {
                    id: "email",
                    value: self.email.clone(),
                    placeholder: "e.g., ada@example.com".into(),
                    enabled: !sending,
                    on_change: v.map(|s| Msg::Feedback(FeedbackMsg::Email(s))),
                },
            )
            .hint("Optional: only if you would like a reply.")
            .hint("Sent to the developer through PostHog with the app version and OS, whether or not error reports are on. Your home folder is shortened to ~.")
            .row(
                "",
                Stack::row(8.0)
                    .align(Align::Center)
                    .child(status)
                    .child(
                        Button::new(
                            "send",
                            ButtonKind::Push,
                            "Send",
                            v.on(Msg::Feedback(FeedbackMsg::Send)),
                        )
                        .enabled(self.can_send()),
                    ),
            );
        Panel {
            key: KEY.into(),
            title: "Send Feedback".into(),
            body: body.into(),
            focus: Some("message"),
            on_close: v.on(Msg::Feedback(FeedbackMsg::Closed)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_needs_words_and_nothing_on_its_way() {
        let mut f = FeedbackForm::default();
        assert!(!f.can_send());
        f.message = " \n ".into();
        assert!(!f.can_send());
        f.message = "It crashed".into();
        assert!(f.can_send());
        f.sending = Some((1, f.feedback()));
        assert!(!f.can_send());
    }

    #[test]
    fn only_the_window_that_sent_reacts_to_the_answer() {
        let mut f = FeedbackForm {
            message: "It crashed".into(),
            ..Default::default()
        };
        assert_eq!(f.landed(1, &Ok(())), Landed::NotMine, "nothing sent");
        f.sending = Some((2, f.feedback()));
        assert_eq!(f.landed(1, &Ok(())), Landed::NotMine);
        assert!(f.sending.is_some(), "still waiting for its own");
        assert_eq!(f.landed(2, &Err("offline".into())), Landed::Failed);
        assert_eq!(f.error.as_deref(), Some("offline"));
        assert!(f.can_send(), "can try again");
        f.sending = Some((3, f.feedback()));
        assert_eq!(f.landed(3, &Ok(())), Landed::Sent);
    }

    #[test]
    fn typing_while_it_sends_is_not_thrown_away() {
        let mut f = FeedbackForm {
            message: "It crashed".into(),
            ..Default::default()
        };
        f.sending = Some((1, f.feedback()));
        f.message.push_str(" twice");
        assert_eq!(f.landed(1, &Ok(())), Landed::EditedSince);
        assert_eq!(f.note.as_deref(), Some(EDITED_WHILE_SENDING));
        assert!(f.can_send());
    }

    #[test]
    fn the_kind_is_what_the_popup_shows() {
        let mut f = FeedbackForm::default();
        assert_eq!(f.feedback().kind, FeedbackKind::Bug);
        f.kind = 1;
        assert_eq!(f.feedback().kind, FeedbackKind::Feature);
        f.kind = 99;
        assert_eq!(f.feedback().kind, FeedbackKind::Other);
    }
}

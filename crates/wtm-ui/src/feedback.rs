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
    /// How sending went.
    Sent(Result<(), String>),
    Closed,
}

#[derive(Debug, Default)]
pub struct FeedbackForm {
    pub kind: usize,
    pub message: String,
    pub email: String,
    /// A message is on its way; Send is off until it lands.
    pub sending: bool,
    /// Why the last send failed.
    pub error: Option<String>,
}

impl FeedbackForm {
    pub fn can_send(&self) -> bool {
        !self.sending && !self.message.trim().is_empty()
    }

    pub fn feedback(&self) -> Feedback {
        Feedback {
            kind: KINDS.get(self.kind).map_or(FeedbackKind::Other, |k| k.0),
            message: self.message.clone(),
            email: self.email.clone(),
        }
    }

    pub fn view(&self, v: &ViewCx<Msg>) -> Panel {
        let status = match (&self.error, self.sending) {
            (_, true) => Text::new("Sending…").ink(Ink::Secondary),
            (Some(e), false) => Text::new(e.clone()).ink(Ink::Error),
            (None, false) => Text::new(""),
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
                    enabled: !self.sending,
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
                    enabled: !self.sending,
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
        f.sending = true;
        assert!(!f.can_send());
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

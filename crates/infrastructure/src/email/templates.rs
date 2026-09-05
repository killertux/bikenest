//! Transactional email rendering.
//!
//! Subject and body come from the shared message catalog, in the *recipient's*
//! locale — the one stored on their account, not the one of whichever request
//! happens to be in flight (there usually is none: mail is sent by the
//! `email.send` job). No user-facing string is written here or in the
//! application layer; this module only picks the keys and fills placeholders.

use bikesnest_application::{EmailKind, EmailMessage};
use bikesnest_i18n::{Locale, Translator};

/// The product name, interpolated into `{app}`. One place, so a rename does
/// not need six catalog edits.
pub const APP_NAME: &str = "BikesNest";

/// A message ready for a relay/ESP: plain text only. Both providers send
/// text/plain; an HTML alternative would be a second template per key with no
/// current caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedEmail {
    pub subject: String,
    pub text: String,
}

/// Render `msg` in its own locale.
///
/// An unparseable locale code cannot happen (the column is CHECK-constrained
/// and `LocaleCode` is validated on the way in), but if one ever did, falling
/// back to the product default is better than failing a send.
pub fn render(msg: &EmailMessage) -> RenderedEmail {
    let locale = Locale::from_code(msg.locale.as_str()).unwrap_or(Locale::PtBr);
    let tr = Translator::new(locale);
    let (subject_key, body_key) = keys(&msg.kind);
    let link = msg.kind.link();
    RenderedEmail {
        subject: fill(tr.t(subject_key), link),
        text: fill(tr.t(body_key), link),
    }
}

/// The catalog keys for one message kind.
fn keys(kind: &EmailKind) -> (&'static str, &'static str) {
    match kind {
        EmailKind::VerifyEmail { .. } => ("email.verify.subject", "email.verify.body"),
        EmailKind::ResetPassword { .. } => ("email.reset.subject", "email.reset.body"),
        EmailKind::ConfirmEmailChange { .. } => ("email.change.subject", "email.change.body"),
    }
}

/// Substitute the catalog placeholders (the same `{name}` convention the page
/// strings use).
fn fill(template: &str, link: &str) -> String {
    template.replace("{app}", APP_NAME).replace("{link}", link)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bikesnest_domain::LocaleCode;

    const LINK: &str = "http://localhost:8080/verify-email?token=abc-123";

    fn msg(locale: LocaleCode, kind: EmailKind) -> EmailMessage {
        EmailMessage::new("ada@example.com", locale, kind)
    }

    fn kinds() -> [EmailKind; 3] {
        [
            EmailKind::VerifyEmail { link: LINK.into() },
            EmailKind::ResetPassword { link: LINK.into() },
            EmailKind::ConfirmEmailChange { link: LINK.into() },
        ]
    }

    /// Every kind, in both locales, must carry the link and the product name —
    /// and must never leak a placeholder or the missing-key marker into mail.
    #[test]
    fn every_kind_renders_completely_in_both_locales() {
        for locale in [LocaleCode::PtBr, LocaleCode::En] {
            for kind in kinds() {
                let code = kind.code();
                let out = render(&msg(locale, kind));
                assert!(
                    out.text.contains(LINK),
                    "{code}/{locale}: body lost the link"
                );
                assert!(
                    out.text.contains(APP_NAME) || out.subject.contains(APP_NAME),
                    "{code}/{locale}: neither subject nor body names the app"
                );
                for part in [&out.subject, &out.text] {
                    assert!(!part.contains('{'), "{code}/{locale}: unfilled placeholder");
                    assert!(!part.contains("⟨i18n?⟩"), "{code}/{locale}: missing key");
                    assert!(!part.trim().is_empty(), "{code}/{locale}: empty");
                }
            }
        }
    }

    /// The point of the whole work package: a pt-BR user gets a Portuguese
    /// subject, an en user an English one — same message, same code path.
    #[test]
    fn subjects_are_written_in_the_recipients_language() {
        let pt = render(&msg(
            LocaleCode::PtBr,
            EmailKind::VerifyEmail { link: LINK.into() },
        ));
        assert_eq!(pt.subject, "Confirme seu e-mail no BikesNest");
        assert!(pt.text.starts_with("Bem-vindo ao BikesNest"));

        let en = render(&msg(
            LocaleCode::En,
            EmailKind::VerifyEmail { link: LINK.into() },
        ));
        assert_eq!(en.subject, "Confirm your BikesNest email");
        assert!(en.text.starts_with("Welcome to BikesNest"));

        let pt_reset = render(&msg(
            LocaleCode::PtBr,
            EmailKind::ResetPassword { link: LINK.into() },
        ));
        assert_eq!(pt_reset.subject, "Redefina sua senha do BikesNest");
        let en_reset = render(&msg(
            LocaleCode::En,
            EmailKind::ResetPassword { link: LINK.into() },
        ));
        assert_eq!(en_reset.subject, "Reset your BikesNest password");
    }

    /// The three kinds must be distinguishable in the inbox: a password reset
    /// that looks like a verification mail is a phishing-shaped surprise.
    #[test]
    fn each_kind_has_its_own_subject() {
        for locale in [LocaleCode::PtBr, LocaleCode::En] {
            let subjects: Vec<String> = kinds()
                .into_iter()
                .map(|k| render(&msg(locale, k)).subject)
                .collect();
            let unique: std::collections::HashSet<&String> = subjects.iter().collect();
            assert_eq!(unique.len(), subjects.len(), "{locale}: {subjects:?}");
        }
    }
}

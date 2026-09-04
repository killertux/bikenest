//! The published policy documents (privacy, terms, cookies) and their
//! version history.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use bikenest_application::POLICY_FALLBACK_LOCALE;
use bikenest_domain::PolicyKind;

use crate::auth::Auth;
use crate::i18n::{Locale, Translator};
use crate::state::AppState;
use crate::view;
use crate::{PageLayout, PolicyPage, PolicyVersionsPage};

use super::common::render;

/// Resolve the current document for the request locale, falling back to
/// pt-BR when that locale has no published document.
pub(crate) async fn current_policy(
    state: &AppState,
    kind: PolicyKind,
    locale: Locale,
) -> Option<bikenest_application::PolicyDocument> {
    let code = locale.html_lang();
    match state.policy.current(kind, code).await {
        Ok(Some(doc)) => Some(doc),
        Ok(None) if code != POLICY_FALLBACK_LOCALE => state
            .policy
            .current(kind, POLICY_FALLBACK_LOCALE)
            .await
            .ok()
            .flatten(),
        _ => None,
    }
}

/// Version history for the request locale, falling back to pt-BR.
pub(crate) async fn policy_history(
    state: &AppState,
    kind: PolicyKind,
    locale: Locale,
) -> Vec<bikenest_application::PolicyDocument> {
    let code = locale.html_lang();
    let docs = state.policy.history(kind, code).await.unwrap_or_default();
    if docs.is_empty() && code != POLICY_FALLBACK_LOCALE {
        return state
            .policy
            .history(kind, POLICY_FALLBACK_LOCALE)
            .await
            .unwrap_or_default();
    }
    docs
}

pub(crate) fn policy_kind_meta(tr: Translator, kind: PolicyKind) -> (&'static str, &'static str) {
    match kind {
        PolicyKind::Privacy => ("privacy", tr.t("nav.privacy")),
        PolicyKind::Terms => ("terms", tr.t("nav.terms")),
        PolicyKind::Cookies => ("cookies", tr.t("nav.cookies")),
    }
}

/// Shared builder for a public versioned legal page (P4/P5/P6). The stored
/// markdown goes through [`crate::markdown::render_policy_markdown`], which
/// escapes raw HTML — that output is the only `|safe` value in the template.
/// Takes `auth` so a signed-in visitor still sees their own header here (these
/// pages are reachable from the footer on every page, logged in or not).
pub(crate) async fn policy_page_impl(
    state: &AppState,
    locale: Locale,
    auth: &Auth,
    kind: PolicyKind,
) -> Response {
    let tr = Translator::new(locale);
    let (kind_code, kind_label) = policy_kind_meta(tr, kind);
    let layout = PageLayout::for_request(
        format!("{kind_label} — BikeNest"),
        kind_code,
        auth,
        &state.map,
    );
    match current_policy(state, kind, locale).await {
        Some(doc) => render(
            PolicyPage {
                layout,
                tr,
                kind_code,
                kind_label,
                version: doc.version.clone(),
                effective_label: view::iso_datetime_label(tr, doc.effective_at),
                content: crate::markdown::render_policy_markdown(&doc.content),
            },
            StatusCode::OK,
        ),
        None => render(
            PolicyPage {
                layout,
                tr,
                kind_code,
                kind_label,
                version: "—".to_string(),
                effective_label: String::new(),
                content: format!(
                    "<p>{}</p>",
                    crate::markdown::escape_text(tr.t("policy.missing"))
                ),
            },
            StatusCode::OK,
        ),
    }
}

pub(crate) async fn privacy_page(
    locale: Locale,
    auth: Auth,
    State(state): State<AppState>,
) -> Response {
    policy_page_impl(&state, locale, &auth, PolicyKind::Privacy).await
}

pub(crate) async fn terms_page(
    locale: Locale,
    auth: Auth,
    State(state): State<AppState>,
) -> Response {
    policy_page_impl(&state, locale, &auth, PolicyKind::Terms).await
}

pub(crate) async fn cookies_page(
    locale: Locale,
    auth: Auth,
    State(state): State<AppState>,
) -> Response {
    policy_page_impl(&state, locale, &auth, PolicyKind::Cookies).await
}

/// Shared builder for a policy version-history page.
pub(crate) async fn policy_versions_impl(
    state: &AppState,
    locale: Locale,
    auth: &Auth,
    kind: PolicyKind,
) -> Response {
    let tr = Translator::new(locale);
    let (kind_code, kind_label) = policy_kind_meta(tr, kind);
    let docs = policy_history(state, kind, locale).await;
    let items: Vec<view::PolicyVersionVm> = docs
        .iter()
        .map(|d| view::policy_version_vm(tr, d))
        .collect();
    render(
        PolicyVersionsPage {
            layout: PageLayout::for_request(
                format!("{} — BikeNest", tr.t("policy.versions_title")),
                kind_code,
                auth,
                &state.map,
            ),
            tr,
            kind_code,
            kind_label,
            items,
        },
        StatusCode::OK,
    )
}

pub(crate) async fn privacy_versions(
    locale: Locale,
    auth: Auth,
    State(state): State<AppState>,
) -> Response {
    policy_versions_impl(&state, locale, &auth, PolicyKind::Privacy).await
}

pub(crate) async fn terms_versions(
    locale: Locale,
    auth: Auth,
    State(state): State<AppState>,
) -> Response {
    policy_versions_impl(&state, locale, &auth, PolicyKind::Terms).await
}

pub(crate) async fn cookies_versions(
    locale: Locale,
    auth: Auth,
    State(state): State<AppState>,
) -> Response {
    policy_versions_impl(&state, locale, &auth, PolicyKind::Cookies).await
}

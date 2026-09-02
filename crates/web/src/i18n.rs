//! Internationalization (REQUIREMENTS §12: pt-BR + en; strings not hard-coded
//! in domain/application logic).
//!
//! Locale is resolved per request: a `lang` cookie (set by the header toggle
//! via `GET /lang/{code}`) wins; otherwise the `Accept-Language` header is
//! parsed; the fallback is pt-BR. The catalog is a compile-time `match`, so a
//! missing key degrades to the key itself rather than panicking.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    En,
    PtBr,
}

impl Locale {
    /// Value for `<html lang>` and `hreflang`.
    pub fn html_lang(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::PtBr => "pt-BR",
        }
    }

    /// Cookie/route code.
    pub fn code(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::PtBr => "pt-br",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code.to_ascii_lowercase().as_str() {
            "en" => Some(Locale::En),
            "pt-br" | "pt" => Some(Locale::PtBr),
            _ => None,
        }
    }

    /// Resolve from request headers: `lang` cookie first, then Accept-Language,
    /// then the pt-BR fallback.
    pub fn from_headers(headers: &axum::http::HeaderMap) -> Self {
        if let Some(cookie) = headers.get(axum::http::header::COOKIE).and_then(|v| v.to_str().ok())
            && let Some(l) = cookie_lang(cookie)
        {
            return l;
        }
        if let Some(al) = headers
            .get(axum::http::header::ACCEPT_LANGUAGE)
            .and_then(|v| v.to_str().ok())
            && let Some(l) = accept_language(al)
        {
            return l;
        }
        Locale::PtBr
    }
}

/// Extract the `lang` cookie value, if present and recognized.
fn cookie_lang(cookie_header: &str) -> Option<Locale> {
    cookie_header
        .split(';')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| k.trim() == "lang")
        .and_then(|(_, v)| Locale::from_code(v.trim()))
}

/// Parse `Accept-Language`, honoring the first tag whose primary subtag we
/// support (quality values are ignored — good enough for a two-locale app).
fn accept_language(header: &str) -> Option<Locale> {
    for part in header.split(',') {
        let tag = part.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        if tag.starts_with("pt") {
            return Some(Locale::PtBr);
        }
        if tag.starts_with("en") {
            return Some(Locale::En);
        }
    }
    None
}

impl<S: Send + Sync> FromRequestParts<S> for Locale {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Locale::from_headers(&parts.headers))
    }
}

/// Request-scoped translator carried into every template and view builder.
#[derive(Debug, Clone, Copy)]
pub struct Translator {
    pub locale: Locale,
}

impl Translator {
    pub fn new(locale: Locale) -> Self {
        Self { locale }
    }

    /// Look up a message by key (Askama-facing).
    pub fn t(&self, key: &str) -> &'static str {
        msg(self.locale, key)
    }

    pub fn lang(&self) -> &'static str {
        self.locale.html_lang()
    }

    pub fn is_en(&self) -> bool {
        self.locale == Locale::En
    }

    pub fn is_pt(&self) -> bool {
        self.locale == Locale::PtBr
    }

    /// Localized "N parking spots" (singular/plural).
    pub fn spots(&self, n: i64) -> String {
        if n == 1 {
            self.t("search.count.one").to_string()
        } else {
            self.t("search.count.other").replace("{n}", &n.to_string())
        }
    }

    /// Localized security-attribute label from its catalog code.
    pub fn security(&self, code: &str) -> &'static str {
        match code {
            "dedicated_locking_point" => self.t("security.dedicated_locking_point"),
            "indoor" => self.t("security.indoor"),
            "cctv" => self.t("security.cctv"),
            "staffed" => self.t("security.staffed"),
            "security_guard" => self.t("security.security_guard"),
            "controlled_access" => self.t("security.controlled_access"),
            "well_lit" => self.t("security.well_lit"),
            "restricted_access" => self.t("security.restricted_access"),
            _ => "",
        }
    }
}

/// The bilingual catalog. Returns `(en, pt-BR)`; unknown keys fall back to the
/// key so a missing translation is visible but never fatal.
pub fn msg(locale: Locale, key: &str) -> &'static str {
    let (en, pt): (&str, &str) = match key {
        // --- nav / brand / footer -----------------------------------------
        "brand.home_aria" => ("BikeNest — home", "BikeNest — início"),
        "nav.how" => ("How it works", "Como funciona"),
        "nav.spots" => ("Parking spots", "Vagas de bike"),
        "nav.community" => ("Community", "Comunidade"),
        "nav.home" => ("Home", "Início"),
        "nav.community_how" => ("Community & how it works", "Comunidade e como funciona"),
        "menu.open" => ("Open menu", "Abrir menu"),
        "lang.group" => ("Language", "Idioma"),
        "lang.pt_aria" => ("Português (Brasil)", "Português (Brasil)"),
        "lang.en_aria" => ("English", "English"),
        "auth.login" => ("Log in", "Entrar"),
        "auth.signup" => ("Sign up", "Criar conta"),
        "footer.tagline" => (
            "A community-maintained map of bicycle parking, built by and for cyclists.",
            "Um mapa de bicicletários mantido pela comunidade, feito por e para quem pedala.",
        ),
        "footer.about" => ("About", "Sobre"),
        "footer.privacy" => ("Privacy policy", "Política de privacidade"),
        "footer.terms" => ("Terms of service", "Termos de uso"),
        "footer.cookies" => ("Cookie policy", "Política de cookies"),
        "footer.coming" => ("Coming in a later milestone", "Em breve, em uma próxima etapa"),
        "footer.subtitle" => (
            "Community-maintained bicycle parking",
            "Bicicletários mantidos pela comunidade",
        ),

        // --- home (P1) -----------------------------------------------------
        "home.title" => (
            "BikeNest — find bicycle parking you can trust",
            "BikeNest — encontre bicicletários confiáveis",
        ),
        "home.eyebrow" => (
            "Community-powered bike parking",
            "Bicicletários feitos pela comunidade",
        ),
        "home.hero.title" => ("From destination to parked bike", "Do destino à bike estacionada"),
        "home.hero.subtitle" => (
            "Search any address in Curitiba and see nearby bicycle parking — with cost, security and how recently each spot was checked.",
            "Busque qualquer endereço em Curitiba e veja bicicletários por perto — com custo, segurança e há quanto tempo cada vaga foi conferida.",
        ),
        "home.search.placeholder" => (
            "Where are you going? e.g. Rua XV de Novembro",
            "Para onde você vai? ex.: Rua XV de Novembro",
        ),
        "home.search.button" => ("Search parking", "Buscar vagas"),
        "home.search.locate" => ("Use my location", "Usar minha localização"),
        "home.search.locate_hint" => (
            "Used once, only for this search.",
            "Usada uma vez, só para esta busca.",
        ),
        "home.how.title" => ("How it works", "Como funciona"),
        "home.how.s1.title" => ("Search a destination", "Busque um destino"),
        "home.how.s1.body" => (
            "Type where you are headed. We find bicycle parking within walking distance.",
            "Digite para onde você vai. Encontramos bicicletários a uma curta caminhada.",
        ),
        "home.how.s2.title" => ("Compare the spots", "Compare as vagas"),
        "home.how.s2.body" => (
            "Cost, security features, opening hours and how fresh the information is — side by side.",
            "Custo, itens de segurança, horários e quão atual é a informação — lado a lado.",
        ),
        "home.how.s3.title" => ("Park with confidence", "Estacione com confiança"),
        "home.how.s3.body" => (
            "Pick a spot, navigate there, and help keep the map honest for the next rider.",
            "Escolha uma vaga, siga até lá e ajude a manter o mapa confiável para quem vem depois.",
        ),
        "home.featured.title" => ("Recently added near Rua XV", "Adicionados recentemente perto da Rua XV"),
        "home.featured.link" => ("See all parking", "Ver todas as vagas"),
        "home.community.eyebrow" => ("A map kept honest by riders", "Um mapa mantido honesto por quem pedala"),
        "home.community.title" => (
            "Every good parking spot is known by someone who rides past it",
            "Toda boa vaga é conhecida por alguém que passa por ela",
        ),
        "home.community.body" => (
            "BikeNest grows from real riders adding spots, confirming they still exist, and flagging what changed. No single source — just the people who park there.",
            "O BikeNest cresce com ciclistas reais adicionando vagas, confirmando que ainda existem e sinalizando o que mudou. Sem fonte única — só quem estaciona ali.",
        ),
        "home.community.p1" => ("Anyone can add a spot", "Qualquer pessoa pode adicionar uma vaga"),
        "home.community.p2" => ("Riders confirm and correct details", "Quem pedala confirma e corrige detalhes"),
        "home.community.p3" => ("Freshness shows how current it is", "A atualidade mostra o quão recente é"),
        "home.community.link" => ("Learn how contributing works", "Veja como contribuir"),
        "home.cta.title" => ("Find your next parking spot", "Encontre sua próxima vaga"),
        "home.cta.body" => (
            "Start with a destination — no account needed.",
            "Comece por um destino — sem precisar de conta.",
        ),
        "home.cta.button" => ("Search parking", "Buscar vagas"),

        // --- search (P2) ---------------------------------------------------
        "search.title" => ("Search — BikeNest", "Busca — BikeNest"),
        "search.heading.near" => ("Parking near", "Vagas perto de"),
        "search.heading.generic" => ("Nearby parking", "Vagas por perto"),
        "search.count.one" => ("1 parking spot", "1 vaga"),
        "search.count.other" => ("{n} parking spots", "{n} vagas"),
        "search.map.show" => ("Show map", "Mostrar mapa"),
        "search.map.hide" => ("Show list", "Mostrar lista"),
        "search.map.recenter" => ("Recenter", "Recentralizar"),
        "search.map.title" => ("Map", "Mapa"),
        "search.map.pins" => ("Numbered pins match the list", "Os pinos numerados batem com a lista"),
        "search.sort.label" => ("Sort", "Ordenar"),
        "search.sort.recommended" => ("Recommended", "Recomendados"),
        "search.sort.distance" => ("Distance", "Distância"),
        "search.sort.security" => ("Security", "Segurança"),
        "search.sort.rating" => ("Rating", "Avaliação"),
        "search.sort.recently_verified" => ("Recently verified", "Verificados recentemente"),
        "search.filters.button" => ("Filters", "Filtros"),
        "search.filters.results" => ("Filter results", "Filtrar resultados"),
        "search.filters.clear" => ("Clear all", "Limpar tudo"),
        "search.filters.security" => ("Security features", "Itens de segurança"),
        "search.filters.cost" => ("Cost", "Custo"),
        "search.filters.cost.any" => ("Any", "Qualquer"),
        "search.filters.cost.free" => ("Free", "Grátis"),
        "search.filters.cost.paid" => ("Paid", "Pago"),
        "search.filters.cost.unknown" => ("Unknown", "Não informado"),
        "search.filters.type" => ("Type", "Tipo"),
        "search.filters.radius" => ("Radius", "Raio"),
        "search.filters.open_now" => ("Open now", "Aberto agora"),
        "search.filters.apply" => ("Apply filters", "Aplicar filtros"),
        "search.radius.default" => ("default", "padrão"),
        "search.radius.hint" => (
            "Selections apply immediately and stay in the URL.",
            "As seleções valem na hora e ficam na URL.",
        ),
        "search.updating" => ("Updating results…", "Atualizando resultados…"),
        "search.results_aria" => ("Parking results", "Resultados de vagas"),
        "search.empty.title" => ("No parking here yet", "Nenhuma vaga por aqui ainda"),
        "search.empty.body" => (
            "Try a wider radius or a different destination.",
            "Tente um raio maior ou outro destino.",
        ),
        "search.missing" => (
            "Type a destination (or use your location) to find parking nearby.",
            "Digite um destino (ou use sua localização) para encontrar vagas por perto.",
        ),
        "search.next" => ("Next page", "Próxima página"),

        // --- parking card --------------------------------------------------
        "card.security_unknown" => ("Security unknown", "Segurança não informada"),
        "card.view" => ("View details", "Ver detalhes"),
        "card.no_photo" => ("No photo yet", "Sem foto ainda"),
        "card.until" => ("until", "até"),

        // --- computed labels: type ----------------------------------------
        "type.rack" => ("Bike rack", "Paraciclo"),
        "type.parking_facility" => ("Bicycle parking", "Bicicletário"),
        "type.indoor" => ("Indoor bicycle parking", "Bicicletário coberto"),
        "type.secured" => ("Secured bicycle parking", "Bicicletário vigiado"),
        "type.locker" => ("Bicycle locker", "Armário para bike"),
        "type.other" => ("Other", "Outro"),

        // --- computed labels: cost ----------------------------------------
        "cost.free" => ("Free", "Grátis"),
        "cost.unknown" => ("Cost unknown", "Custo não informado"),
        "cost.paid_unknown" => ("Paid — price unknown", "Pago — preço não informado"),
        "unit.hour" => ("hour", "hora"),
        "unit.day" => ("day", "dia"),
        "unit.month" => ("month", "mês"),
        "unit.entry" => ("entry", "entrada"),

        // --- computed labels: rating / freshness / open -------------------
        "rating.none" => ("No reviews yet", "Sem avaliações ainda"),
        "freshness.fresh" => ("Fresh", "Atual"),
        "freshness.recently_verified" => ("Recently verified", "Verificado há pouco"),
        "freshness.aging" => ("Aging", "Envelhecendo"),
        "freshness.stale" => ("Stale", "Desatualizado"),
        "freshness.very_stale" => ("Very stale", "Muito desatualizado"),
        "freshness.never" => ("Never verified", "Nunca verificado"),
        "open.now" => ("Open now", "Aberto agora"),
        "open.closed" => ("Closed", "Fechado"),
        "open.unknown" => ("Hours unknown", "Horário não informado"),

        // --- computed labels: security codes ------------------------------
        "security.dedicated_locking_point" => ("Dedicated locking point", "Ponto de fixação"),
        "security.indoor" => ("Indoor", "Coberto"),
        "security.cctv" => ("CCTV", "Câmeras"),
        "security.staffed" => ("Staffed", "Com funcionário"),
        "security.security_guard" => ("Security guard", "Segurança"),
        "security.controlled_access" => ("Controlled access", "Acesso controlado"),
        "security.well_lit" => ("Well lit", "Bem iluminado"),
        "security.restricted_access" => ("Restricted access", "Acesso restrito"),

        // --- computed labels: hours + days --------------------------------
        "hours.unknown" => ("Unknown", "Não informado"),
        "hours.closed" => ("Closed", "Fechado"),
        "hours.all_day" => ("Open 24 hours", "Aberto 24 horas"),
        "day.mon" => ("Monday", "Segunda"),
        "day.tue" => ("Tuesday", "Terça"),
        "day.wed" => ("Wednesday", "Quarta"),
        "day.thu" => ("Thursday", "Quinta"),
        "day.fri" => ("Friday", "Sexta"),
        "day.sat" => ("Saturday", "Sábado"),
        "day.sun" => ("Sunday", "Domingo"),

        // --- verification labels ------------------------------------------
        "verified.today" => ("Verified today", "Verificado hoje"),
        "verified.yesterday" => ("Verified yesterday", "Verificado ontem"),
        "verified.days_ago" => ("Last verified {n} days ago", "Verificado há {n} dias"),
        "verified.never" => ("Never verified", "Nunca verificado"),

        // --- details (P3) --------------------------------------------------
        "details.breadcrumb.home" => ("Home", "Início"),
        "details.breadcrumb.search" => ("Parking", "Vagas"),
        "details.badge.community" => ("Community verified", "Verificado pela comunidade"),
        "details.navigate.google" => ("Open in Google Maps", "Abrir no Google Maps"),
        "details.navigate.osm" => ("Open in OpenStreetMap", "Abrir no OpenStreetMap"),
        "details.facts.title" => ("Key facts", "Informações principais"),
        "details.facts.cost" => ("Cost", "Custo"),
        "details.facts.security" => ("Security", "Segurança"),
        "details.facts.freshness" => ("Freshness", "Atualidade"),
        "details.facts.rating" => ("Rating", "Avaliação"),
        "details.hours.title" => ("Opening hours", "Horário de funcionamento"),
        "details.hours.tz" => ("Times shown in", "Horários no fuso"),
        "details.security.title" => ("Security attributes", "Itens de segurança"),
        "details.security.yes" => ("Yes", "Sim"),
        "details.security.no" => ("No", "Não"),
        "details.security.unknown" => ("Unknown", "Não informado"),
        "details.gallery.empty" => ("No photos yet", "Sem fotos ainda"),
        "details.gallery.empty_hint" => (
            "Photos help riders recognize a spot. Adding them arrives with community accounts.",
            "Fotos ajudam a reconhecer uma vaga. Enviá-las chega com as contas da comunidade.",
        ),
        "details.reviews.title" => ("Reviews", "Avaliações"),
        "details.reviews.empty" => (
            "No reviews yet — reviews arrive with community accounts.",
            "Sem avaliações ainda — as avaliações chegam com as contas da comunidade.",
        ),
        "details.contribute.title" => ("Help keep this spot accurate", "Ajude a manter esta vaga correta"),
        "details.contribute.body" => (
            "Favoriting, verifying and proposing changes arrive with community accounts.",
            "Favoritar, verificar e propor mudanças chegam com as contas da comunidade.",
        ),

        // --- about (P7) ----------------------------------------------------
        "about.title" => ("About — BikeNest", "Sobre — BikeNest"),
        "about.hero.eyebrow" => ("The community model", "O modelo da comunidade"),
        "about.hero.title" => (
            "Every good parking spot, known by someone who already rides past it",
            "Toda boa vaga, conhecida por alguém que já passa por ela",
        ),
        "about.hero.body" => (
            "BikeNest is a community-maintained map of bicycle parking. There is no official dataset — the map is only as good, and as current, as the riders who build it.",
            "O BikeNest é um mapa de bicicletários mantido pela comunidade. Não há base oficial — o mapa é tão bom, e tão atual, quanto quem pedala e o constrói.",
        ),
        "about.hero.cta_search" => ("Search parking", "Buscar vagas"),
        "about.how.title" => ("A loop that runs on riders", "Um ciclo movido por quem pedala"),
        "about.how.s1.title" => ("Someone adds a spot", "Alguém adiciona uma vaga"),
        "about.how.s1.body" => (
            "A rider marks where parking exists, with its type, cost, hours and security.",
            "Um ciclista marca onde há vaga, com tipo, custo, horário e segurança.",
        ),
        "about.how.s2.title" => ("Others confirm it", "Outros confirmam"),
        "about.how.s2.body" => (
            "Riders verify a spot still exists and correct anything that changed.",
            "Quem passa por ali verifica que a vaga existe e corrige o que mudou.",
        ),
        "about.how.s3.title" => ("Everyone sees how fresh it is", "Todos veem quão atual é"),
        "about.how.s3.body" => (
            "Each spot shows when it was last confirmed, so you know how much to trust it.",
            "Cada vaga mostra quando foi confirmada, para você saber o quanto confiar.",
        ),
        "about.fresh.title" => ("How verification and freshness work", "Como funcionam verificação e atualidade"),
        "about.fresh.body" => (
            "Every spot carries a freshness signal based on when it was last verified.",
            "Cada vaga carrega um sinal de atualidade com base na última verificação.",
        ),
        "about.fresh.col.label" => ("Freshness", "Atualidade"),
        "about.fresh.col.meaning" => ("What it means", "O que significa"),
        "about.fresh.fresh" => ("Verified very recently — trust it.", "Verificado há muito pouco — pode confiar."),
        "about.fresh.recently_verified" => ("Confirmed lately — likely accurate.", "Confirmado recentemente — provavelmente correto."),
        "about.fresh.aging" => ("A while since the last check.", "Faz um tempo desde a última conferida."),
        "about.fresh.stale" => ("Overdue for a re-check.", "Passou da hora de reconferir."),
        "about.fresh.very_stale" => ("Long unverified — treat with care.", "Sem verificação há muito — cuidado."),
        "about.contribute.title" => ("Four ways to contribute", "Quatro formas de contribuir"),
        "about.contribute.add.title" => ("Add a spot", "Adicionar uma vaga"),
        "about.contribute.add.body" => ("Map parking that isn't here yet.", "Mapeie vagas que ainda não estão aqui."),
        "about.contribute.verify.title" => ("Verify a spot", "Verificar uma vaga"),
        "about.contribute.verify.body" => ("Confirm a spot still exists as described.", "Confirme que a vaga ainda existe como descrito."),
        "about.contribute.review.title" => ("Review a spot", "Avaliar uma vaga"),
        "about.contribute.review.body" => ("Share how good it is to park there.", "Conte como é estacionar ali."),
        "about.contribute.report.title" => ("Report a problem", "Relatar um problema"),
        "about.contribute.report.body" => ("Flag a spot that's gone or wrong.", "Sinalize uma vaga que sumiu ou está errada."),
        "about.moderation.title" => ("Moderation keeps it trustworthy", "A moderação mantém a confiança"),
        "about.moderation.body" => (
            "Photos and reports are reviewed by moderators before they affect the map, and every moderation action is recorded. These tools arrive as the project grows.",
            "Fotos e denúncias passam por moderadores antes de afetarem o mapa, e cada ação de moderação é registrada. Essas ferramentas chegam conforme o projeto cresce.",
        ),
        "about.cta.title" => ("Ready to find parking?", "Pronto para encontrar vagas?"),
        "about.cta.button" => ("Search parking", "Buscar vagas"),

        // --- errors --------------------------------------------------------
        "error.404.title" => ("Page not found", "Página não encontrada"),
        "error.404.body" => (
            "The page you are looking for does not exist.",
            "A página que você procura não existe.",
        ),
        "error.500.title" => ("Something went wrong", "Algo deu errado"),
        "error.500.body" => (
            "Something went wrong on our side. Please try again.",
            "Algo deu errado do nosso lado. Tente novamente.",
        ),
        "error.home" => ("Back to home", "Voltar ao início"),
        "error.search" => ("Search parking", "Buscar vagas"),

        // --- auth: register / login (A1/A2) -------------------------------
        "auth.register_title" => ("Create your account", "Crie sua conta"),
        "auth.register_subtitle" => (
            "A community account keeps your contributions and the map honest.",
            "Uma conta da comunidade mantém suas contribuições e o mapa honestos.",
        ),
        "auth.login_title" => ("Log in", "Entrar"),
        "auth.login_subtitle" => ("Welcome back.", "Bem-vindo de volta."),
        "auth.email" => ("Email", "E-mail"),
        "auth.display_name" => ("Display name (optional)", "Nome de exibição (opcional)"),
        "auth.password" => ("Password", "Senha"),
        "auth.password_hint" => ("At least 8 characters", "No mínimo 8 caracteres"),
        "auth.have_account" => ("Already have an account?", "Já tem uma conta?"),
        "auth.forgot" => ("Forgot password?", "Esqueceu a senha?"),
        "auth.logout" => ("Log out", "Sair"),
        "auth.google" => ("Continue with Google", "Continuar com o Google"),
        "auth.oauth_note" => ("Or", "Ou"),

        // --- auth: verification (A3) ---------------------------------------
        "auth.verify_title" => ("Verify your email", "Verifique seu e-mail"),
        "auth.verify_success" => ("Email verified", "E-mail verificado"),
        "auth.verify_success_body" => (
            "Your account is active. You can now log in and contribute.",
            "Sua conta está ativa. Agora você pode entrar e contribuir.",
        ),
        "auth.verify_invalid" => ("Verification link invalid or expired", "Link de verificação inválido ou expirado"),
        "auth.resend_hint" => ("Enter your email to resend the link:", "Digite seu e-mail para reenviar o link:"),
        "auth.resend_link" => ("Resend verification email", "Reenviar e-mail de verificação"),

        // --- auth: password reset (A4/A5) ---------------------------------
        "auth.reset_title" => ("Reset your password", "Redefinir sua senha"),
        "auth.reset_subtitle" => (
            "Enter your email and we will send a reset link if it exists.",
            "Digite seu e-mail e enviaremos um link de redefinição se ele existir.",
        ),
        "auth.reset_button" => ("Send reset link", "Enviar link de redefinição"),
        "auth.reset_new_title" => ("Set a new password", "Defina uma nova senha"),
        "auth.reset_new_subtitle" => ("Choose a strong new password.", "Escolha uma nova senha forte."),

        // --- auth: notices / errors ---------------------------------------
        "auth.registered" => (
            "Check your inbox to verify your email, then log in.",
            "Confira seu e-mail para verificar seu endereço e depois entre.",
        ),
        "auth.verified" => ("Email verified — you can log in.", "E-mail verificado — você pode entrar."),
        "auth.reset_sent" => (
            "If that address exists, a reset link has been sent.",
            "Se esse endereço existir, um link de redefinição foi enviado.",
        ),
        "auth.resend_sent" => (
            "If that address exists, a verification email has been sent.",
            "Se esse endereço existir, um e-mail de verificação foi enviado.",
        ),
        "auth.oauth_failed" => ("Google sign-in failed. Try again.", "Falha ao entrar com o Google. Tente novamente."),
        "auth.error.invalid_credentials" => (
            "Email or password is incorrect.",
            "E-mail ou senha incorretos.",
        ),
        "auth.error.weak_password" => ("Password must be at least 8 characters.", "A senha deve ter pelo menos 8 caracteres."),
        "auth.error.invalid_email" => ("That email is not valid.", "Esse e-mail não é válido."),
        "auth.error.rate_limited" => ("Too many attempts. Try again later.", "Muitas tentativas. Tente novamente mais tarde."),
        "auth.error.invalid_token" => ("That link is invalid or has expired.", "Esse link é inválido ou expirou."),
        "auth.error.last_admin" => ("You cannot remove your own last admin role.", "Você não pode remover sua própria última função de admin."),
        "auth.error.generic" => ("Something went wrong. Try again.", "Algo deu errado. Tente novamente."),

        // --- account (C1) --------------------------------------------------
        "account.title" => ("Your account", "Sua conta"),
        "account.nav" => ("Account", "Conta"),
        "account.profile" => ("Profile", "Perfil"),
        "account.display_name" => ("Display name", "Nome de exibição"),
        "account.roles" => ("Roles", "Funções"),
        "account.email_verified" => ("Email verified", "E-mail verificado"),
        "account.verified_yes" => ("Yes", "Sim"),
        "account.verified_no" => ("No", "Não"),
        "account.settings" => ("Security", "Segurança"),
        "account.change_password" => ("Change password", "Alterar senha"),
        "account.change_email" => ("Change email", "Alterar e-mail"),
        "account.back" => ("Back to account", "Voltar à conta"),
        "account.banner_title" => ("Verify your email to contribute", "Verifique seu e-mail para contribuir"),
        "account.banner_body" => (
            "Your account is active, but you must confirm your email address before adding or editing parking.",
            "Sua conta está ativa, mas você precisa confirmar seu e-mail antes de adicionar ou editar vagas.",
        ),
        "account.pw_changed" => ("Your password was changed.", "Sua senha foi alterada."),
        "account.email_pending" => (
            "A confirmation link was sent to your new email.",
            "Um link de confirmação foi enviado para seu novo e-mail.",
        ),

        // --- change password (C2) / change email (C3) ----------------------
        "account.pw_title" => ("Change password", "Alterar senha"),
        "account.pw_current" => ("Current password", "Senha atual"),
        "account.pw_new" => ("New password", "Nova senha"),
        "account.pw_submit" => ("Update password", "Atualizar senha"),
        "account.email_title" => ("Change email", "Alterar e-mail"),
        "account.email_current" => ("Current email:", "E-mail atual:"),
        "account.email_new" => ("New email", "Novo e-mail"),
        "account.email_submit" => ("Request change", "Solicitar alteração"),

        // --- roles / account state labels ---------------------------------
        "role.user" => ("User", "Usuário"),
        "role.moderator" => ("Moderator", "Moderador"),
        "role.admin" => ("Admin", "Administrador"),
        "account.state.pending" => ("Pending verification", "Aguardando verificação"),
        "account.state.active" => ("Active", "Ativa"),
        "account.state.suspended" => ("Suspended", "Suspensa"),
        "account.state.deleted" => ("Deleted", "Excluída"),
        "account.unverified" => ("Unverified", "Não verificado"),

        // --- admin user management (M5) ------------------------------------
        "admin.users_title" => ("Users", "Usuários"),
        "admin.user" => ("User", "Usuário"),
        "admin.state" => ("State", "Estado"),
        "admin.roles" => ("Roles", "Funções"),
        "admin.actions" => ("Actions", "Ações"),
        "admin.granted" => ("Role granted.", "Função concedida."),
        "admin.revoked" => ("Role revoked.", "Função revogada."),
        "admin.role_error" => ("That action could not be completed.", "Não foi possível concluir essa ação."),
        "admin.grant_moderator" => ("+ Moderator", "+ Moderador"),
        "admin.revoke_moderator" => ("− Moderator", "− Moderador"),
        "admin.grant_admin" => ("+ Admin", "+ Admin"),
        "admin.revoke_admin" => ("− Admin", "− Admin"),

        // --- M3 contributions: add (D1) ---------------------------------
        "new.title" => ("Add a parking spot", "Adicionar uma vaga"),
        "new.subtitle" => (
            "Mark where bicycle parking exists — type, cost, hours, security.",
            "Marque onde há bicicletário — tipo, custo, horário, segurança.",
        ),
        "new.added" => ("Added as spot", "Vaga adicionada"),
        "new.duplicate.title" => ("This may already be listed", "Pode já estar cadastrado"),
        "new.duplicate.warning" => (
            "It looks similar to existing listings. You can still keep the new spot, but consider that it may already be mapped.",
            "Parece similar a vagas existentes. Você ainda pode manter a nova, mas considere que talvez já esteja mapeada.",
        ),
        "new.field.name" => ("Name", "Nome"),
        "new.field.address" => ("Address", "Endereço"),
        "new.field.description" => ("Description (optional)", "Descrição (opcional)"),
        "new.field.type" => ("Type", "Tipo"),
        "new.field.cost" => ("Cost", "Custo"),
        "new.field.lat" => ("Latitude", "Latitude"),
        "new.field.lon" => ("Longitude", "Longitude"),
        "new.field.tz" => ("Timezone", "Fuso horário"),
        "new.field.tz_hint" => (
            "Left blank, we derive it from the pin. You can override.",
            "Em branco, o derivamos do ponto. Você pode alterar.",
        ),
        "new.field.open_24h" => ("Open 24 hours", "Aberto 24 horas"),
        "new.field.security" => ("Security attributes", "Itens de segurança"),
        "new.field.price" => ("Price", "Preço"),
        "new.field.currency" => ("Currency", "Moeda"),
        "new.field.unit" => ("Per", "Por"),
        "new.submit" => ("Add parking spot", "Adicionar vaga"),

        // --- M3 contributions: edit (D2) --------------------------------
        "edit.title" => ("Edit parking spot", "Editar vaga"),
        "edit.subtitle" => (
            "Update the details of this spot. Moving the pin or removal are separate actions below.",
            "Atualize os detalhes desta vaga. Mover o ponto ou remover são ações separadas abaixo.",
        ),
        "edit.submit" => ("Save changes", "Salvar alterações"),
        "edit.link" => ("Edit this spot", "Editar esta vaga"),
        "edit.sensitive.title" => ("Sensitive changes", "Mudanças sensíveis"),
        "edit.sensitive.body" => (
            "Moving the pin or removing a spot could mislead other riders, so these are proposed and reviewed.",
            "Mover o ponto ou remover uma vaga pode enganar outros ciclistas, então isso é proposto e revisado.",
        ),
        "edit.move.title" => ("Move the pin", "Mover o ponto"),
        "edit.move.submit" => ("Propose new location", "Propor novo local"),
        "edit.remove.title" => ("Remove / mark gone", "Remover / marcar como sumido"),
        "edit.remove.submit" => ("Propose change", "Propor mudança"),
        "edit.reason" => ("Reason", "Motivo"),

        // --- M3 reviews (D3) -------------------------------------------
        "review.title" => ("Write a review", "Escrever uma avaliação"),
        "review.subtitle" => ("How was it to park here?", "Como foi estacionar aqui?"),
        "review.rating" => ("Rating", "Avaliação"),
        "review.select" => ("Choose a rating", "Escolha uma nota"),
        "review.body" => ("Your review", "Sua avaliação"),
        "review.length_hint" => ("Between 1 and 2000 characters.", "Entre 1 e 2000 caracteres."),
        "review.submit" => ("Save review", "Salvar avaliação"),
        "review.write" => ("Write a review", "Escrever uma avaliação"),
        "review.edit" => ("Edit your review", "Editar sua avaliação"),
        "review.error.invalid" => ("Rating must be 1 to 5 stars.", "A nota deve ser de 1 a 5 estrelas."),
        "review.error.length" => ("Review must be 1 to 2000 characters.", "A avaliação deve ter de 1 a 2000 caracteres."),
        "review.error.generic" => ("Could not save your review.", "Não foi possível salvar sua avaliação."),

        // --- M3 favorites (C4) -----------------------------------------
        "favorites.title" => ("Your favorites", "Seus favoritos"),
        "favorites.subtitle" => ("Spots you saved for later.", "Vagas que você salvou."),
        "favorites.save" => ("Save to favorites", "Salvar nos favoritos"),
        "favorites.saved" => ("Saved", "Salvo"),
        "favorites.empty" => ("No favorites yet", "Nenhum favorito ainda"),
        "favorites.empty_hint" => (
            "Tap the heart on a spot to keep it here.",
            "Toque no coração em uma vaga para mantê-la aqui.",
        ),

        // --- M3 contributions history (C5) ------------------------------
        "contrib.title" => ("Your contributions", "Suas contribuições"),
        "contrib.subtitle" => ("Everything you have added, verified or reviewed.", "Tudo que você adicionou, verificou ou avaliou."),
        "contrib.empty" => ("No contributions yet", "Nenhuma contribuição ainda"),
        "contrib.empty_hint" => (
            "Add a spot, verify one, or write a review.",
            "Adicione uma vaga, verifique uma ou escreva uma avaliação.",
        ),
        "contrib.kind.added" => ("Added", "Adicionou"),
        "contrib.kind.edited" => ("Edited", "Editou"),
        "contrib.kind.proposed" => ("Proposed", "Propôs"),
        "contrib.kind.reviewed" => ("Reviewed", "Avaliou"),
        "contrib.kind.verified" => ("Verified", "Verificou"),
        "contrib.kind.favorited" => ("Favorited", "Favoritou"),
        "contrib.kind.other" => ("Contributed", "Contribuiu"),
        "contrib.state.active" => ("Active", "Ativa"),
        "contrib.state.pending" => ("Pending", "Pendente"),
        "contrib.state.history" => ("History", "História"),
        "contrib.state.other" => ("—", "—"),

        // --- Confidence (M3 §106) ---------------------------------------
        "confidence.title" => ("Confidence", "Confiança"),
        "confidence.reported" => ("Reported", "Reportado"),
        "confidence.verified" => ("Verified", "Verificado"),
        "confidence.recently_verified" => ("Recently verified", "Verificado há pouco"),
        "confidence.stale" => ("Stale", "Desatualizado"),
        "confidence.conflicting" => ("Conflicting", "Conflitante"),
        "confidence.disputed" => (
            "Some riders say this spot has changed. The map is not averaged — it shows both sides.",
            "Alguns ciclistas dizem que esta vaga mudou. O mapa não faz média — mostra os dois lados.",
        ),
        "confidence.disputes" => ("disputes:", "disputas:"),
        "confidence.parked_here_count" => ("Riders who parked here:", "Ciclistas que estacionaram aqui:"),

        // --- Verification (M3 §39) ---------------------------------------
        "verification.title" => ("Verify this spot", "Verificar esta vaga"),
        "verification.anonymous" => ("Log in and verify your email to confirm this spot.", "Entre e verifique seu e-mail para confirmar esta vaga."),
        "verification.saved" => ("Thanks — recorded.", "Obrigado — registrado."),
        "verify.still_exists" => ("Still exists", "Ainda existe"),
        "verify.no_longer_exists" => ("No longer exists", "Não existe mais"),
        "verify.info_changed" => ("Information changed", "Informação mudou"),
        "verify.parked_here" => ("I parked here", "Estacionei aqui"),
        "parked.saved" => ("Noted. This helps others spot usage.", "Anotado. Isso ajuda a ver o uso."),

        // --- P3 post-action notices --------------------------------------
        "details.notice.proposed" => (
            "Your change has been submitted and will be reviewed by a moderator before it appears.",
            "Sua mudança foi enviada e será revisada por um moderador antes de aparecer.",
        ),
        "details.notice.edited" => ("Your changes were saved.", "Suas alterações foram salvas."),
        "details.notice.reviewed" => ("Thanks — your review was saved.", "Obrigado — sua avaliação foi salva."),
        "details.notice.added" => ("The parking spot was added.", "A vaga foi adicionada."),

        // --- P3 recommended because (§105) -------------------------------
        "details.recommend.title" => ("Recommended because", "Recomendado porque"),
        "reason.distance" => ("Close to your destination", "Perto do seu destino"),
        "reason.security" => ("Security attributes", "Itens de segurança"),
        "reason.rating" => ("Rated by riders", "Avaliado por ciclistas"),
        "reason.freshness" => ("Recently verified", "Verificado há pouco"),
        "reason.verification" => ("Confirmed by riders", "Confirmado por ciclistas"),

        // --- Contribution errors -----------------------------------------
        "contribution.error.not_verified" => (
            "Verify your email to contribute.",
            "Verifique seu e-mail para contribuir.",
        ),
        "contribution.error.rate_limited" => ("Too many attempts. Try again later.", "Muitas tentativas. Tente novamente mais tarde."),
        "contribution.error.version_conflict" => (
            "Someone else recently changed this. We reloaded the latest values — try again.",
            "Alguém alterou isto recentemente. Recarregamos os valores atuais — tente de novo.",
        ),
        "contribution.error.not_found" => ("That spot could not be found.", "Não foi possível encontrar essa vaga."),
        "contribution.error.invalid" => ("Some fields are missing or invalid.", "Alguns campos estão vazios ou inválidos."),
        "contribution.error.unauthorized" => ("You cannot perform this action.", "Você não pode fazer isso."),
        "contribution.error.timezone" => ("Could not determine the timezone from that point.", "Não foi possível determinar o fuso a partir do ponto."),
        "contribution.error.internal" => ("Something went wrong. Please try again.", "Algo deu errado. Tente novamente."),
        "contribution.error.generic" => ("Something went wrong.", "Algo deu errado."),
        "contribution.verify_to" => ("Verify your email to confirm this spot.", "Verifique seu e-mail para confirmar esta vaga."),

        // --- time-ago labels ---------------------------------------------
        "time.today" => ("today", "hoje"),
        "time.yesterday" => ("yesterday", "ontem"),
        "time.days_ago" => ("{n} days ago", "há {n} dias"),
        "time.months_ago" => ("{n} months ago", "há {n} meses"),

        // --- attribute codes (verification) ------------------------------
        "attr.name" => ("Name", "Nome"),
        "attr.address" => ("Address", "Endereço"),
        "attr.type" => ("Type", "Tipo"),
        "attr.cost" => ("Cost", "Custo"),
        "attr.hours" => ("Hours", "Horário"),
        "attr.security" => ("Security", "Segurança"),
        "attr.location" => ("Location", "Localização"),
        "attr.unknown" => ("Details", "Detalhes"),

        // Unknown key: a visible marker (all real keys are defined above, so
        // this only appears when a template references a typo'd key).
        _ => ("⟨i18n?⟩", "⟨i18n?⟩"),
    };
    match locale {
        Locale::En => en,
        Locale::PtBr => pt,
    }
}

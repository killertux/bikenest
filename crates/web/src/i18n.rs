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

        // Unknown key: a visible marker (all real keys are defined above, so
        // this only appears when a template references a typo'd key).
        _ => ("⟨i18n?⟩", "⟨i18n?⟩"),
    };
    match locale {
        Locale::En => en,
        Locale::PtBr => pt,
    }
}

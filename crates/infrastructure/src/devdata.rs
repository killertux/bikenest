//! Deterministic development dataset shared by the mock seeder and the
//! `FakeGeocoder` (Ledger #1/#2). Never used outside dev/demo workflows.
//!
//! Geography: Curitiba, Paraná. Timezone is still `America/Sao_Paulo` (Curitiba
//! keeps BRT, no DST) — the seeder tags every row with it.

use bikenest_domain::{Cost, CurrencyCode, Money, ParkingType, PricingUnit};

/// One weekly hours row: (iso_day, opens(h,m), closes(h,m), all_day).
pub type HoursRow = (u8, (u32, u32), (u32, u32), bool);

/// Known destinations for the fake geocoder: (normalized query, lat, lon).
/// Keys are stored in the `normalize()`d form (lowercase, accents stripped) so
/// they match the normalized search query.
pub const LANDMARKS: &[(&str, f64, f64)] = &[
    ("rua xv de novembro", -25.429_700, -49.270_500),
    ("boca maldita", -25.429_500, -49.270_000),
    ("praca tiradentes", -25.428_800, -49.271_200),
    ("catedral de curitiba", -25.429_100, -49.271_800),
    ("mercado municipal", -25.435_800, -49.266_200),
    ("largo da ordem", -25.427_000, -49.274_500),
    ("museu oscar niemeyer", -25.409_000, -49.268_000),
    ("mon", -25.409_000, -49.268_000),
    ("jardim botanico", -25.443_000, -49.238_500),
    ("parque barigui", -25.425_800, -49.308_500),
    ("parque tangua", -25.378_800, -49.286_200),
    ("opera de arame", -25.385_500, -49.276_000),
    ("passeio publico", -25.423_000, -49.269_500),
    ("praca do japao", -25.447_000, -49.282_000),
    ("batel", -25.441_500, -49.287_000),
    ("shopping estacao", -25.439_200, -49.264_000),
    ("ufpr", -25.430_000, -49.265_000),
    ("torre panoramica", -25.418_000, -49.290_500),
    ("teatro guaira", -25.431_800, -49.268_800),
    ("curitiba", -25.428_400, -49.273_300),
];

/// One mock parking row (all fields explicit; the seeder inserts exactly this).
#[derive(Debug, Clone)]
pub struct MockParking {
    pub name: &'static str,
    pub address: &'static str,
    pub description: &'static str,
    pub parking_type: ParkingType,
    pub cost: Cost,
    pub lat: f64,
    pub lon: f64,
    /// Weekly hours as (iso_day, opens, closes, all_day); empty = closed all week.
    pub hours: Vec<HoursRow>,
    pub hours_unknown: bool,
    /// (feature_code, state) — 0 unknown, 1 yes, 2 no.
    pub security: Vec<(&'static str, i16)>,
    /// Target average the seeder approximates when synthesizing the backing
    /// `review` rows (`rating_count` reviews via [`star_ratings_for`]). Never
    /// written to `parking_location.rating_avg` directly — the seeder always
    /// recomputes that column from the real reviews it inserts, so a rating
    /// never appears without reviews behind it.
    pub rating_avg: Option<f64>,
    /// How many backing reviews to synthesize for this location (0 = no
    /// reviews, no rating).
    pub rating_count: i64,
    /// Days since last verification (relative to seed time).
    pub verified_days_ago: Option<i64>,
    /// Basename of the seeded photo (under `web/static/img/`) attached to this
    /// location, or `None` for a location with no photo yet. The seeder stores
    /// the file through the object-storage port and links it (moderation
    /// `APPROVED`). See `crates/infrastructure/src/parking/seed.rs`.
    pub photo: Option<&'static str>,
}

fn paid(cents: i64, currency: &str, unit: PricingUnit) -> Cost {
    Cost::Paid {
        price: Some(Money::new(
            cents,
            CurrencyCode::parse(currency).expect("devdata currency"),
            unit,
        )),
    }
}

/// Seeded community reviewers (email, display name): the seeder finds-or-
/// creates these `users` rows and authors the backing `review` rows for every
/// [`MockParking::rating_count`] (Problem #2 — a rating never appears without
/// real reviews behind it). 25 entries comfortably covers the highest seeded
/// `rating_count` (21) with room to spare; `users` has no `seed_key` column,
/// so re-seeding finds them by email instead of deleting/reinserting.
pub const REVIEW_AUTHORS: &[(&str, &str)] = &[
    ("mariana.silva@seed.bikenest.dev", "Mariana Silva"),
    ("joao.pereira@seed.bikenest.dev", "João Pereira"),
    ("ana.souza@seed.bikenest.dev", "Ana Souza"),
    ("carlos.oliveira@seed.bikenest.dev", "Carlos Oliveira"),
    ("beatriz.santos@seed.bikenest.dev", "Beatriz Santos"),
    ("lucas.almeida@seed.bikenest.dev", "Lucas Almeida"),
    ("fernanda.costa@seed.bikenest.dev", "Fernanda Costa"),
    ("rafael.gomes@seed.bikenest.dev", "Rafael Gomes"),
    ("juliana.ribeiro@seed.bikenest.dev", "Juliana Ribeiro"),
    ("marcos.carvalho@seed.bikenest.dev", "Marcos Carvalho"),
    ("patricia.martins@seed.bikenest.dev", "Patrícia Martins"),
    ("diego.rocha@seed.bikenest.dev", "Diego Rocha"),
    ("camila.barbosa@seed.bikenest.dev", "Camila Barbosa"),
    ("bruno.teixeira@seed.bikenest.dev", "Bruno Teixeira"),
    ("larissa.pinto@seed.bikenest.dev", "Larissa Pinto"),
    ("thiago.cardoso@seed.bikenest.dev", "Thiago Cardoso"),
    ("amanda.moreira@seed.bikenest.dev", "Amanda Moreira"),
    ("felipe.araujo@seed.bikenest.dev", "Felipe Araújo"),
    ("gabriela.correia@seed.bikenest.dev", "Gabriela Correia"),
    ("vinicius.melo@seed.bikenest.dev", "Vinícius Melo"),
    ("bianca.dias@seed.bikenest.dev", "Bianca Dias"),
    ("gustavo.nascimento@seed.bikenest.dev", "Gustavo Nascimento"),
    ("isabela.freitas@seed.bikenest.dev", "Isabela Freitas"),
    ("rodrigo.lima@seed.bikenest.dev", "Rodrigo Lima"),
    ("carolina.azevedo@seed.bikenest.dev", "Carolina Azevedo"),
];

/// Distribute `count` integer star ratings (1..=5) whose sum rounds to
/// `avg * count`, so the `review` rows the seeder inserts approximate the
/// dataset's intended average once real reviews back it (Problem #2). Pure
/// and deterministic — no I/O.
pub fn star_ratings_for(avg: f64, count: i64) -> Vec<u8> {
    if count <= 0 {
        return Vec::new();
    }
    let count = count as usize;
    let target_sum = (avg * count as f64).round() as i64;
    let target_sum = target_sum.clamp(count as i64, count as i64 * 5);
    let base = (target_sum / count as i64) as u8;
    let remainder = (target_sum % count as i64) as usize;
    let mut stars = vec![base; count];
    for s in stars.iter_mut().take(remainder) {
        *s += 1;
    }
    stars
}

/// A short pt-BR review sentence for a given star rating, cycling through a
/// few variants per level so a location's reviews don't all read identically.
pub fn review_body_for(star: u8, i: usize) -> &'static str {
    const FIVE: &[&str] = &[
        "Excelente, super recomendo!",
        "Muito seguro e bem localizado.",
        "Perfeito para deixar a bike, sem preocupação.",
    ];
    const FOUR: &[&str] = &[
        "Bom paraciclo, só faltou mais iluminação.",
        "Estrutura boa, acesso fácil.",
        "Gostei, costumo usar toda semana.",
    ];
    const THREE: &[&str] = &[
        "Razoável, cumpre a função.",
        "Ok, nada excepcional.",
        "Serve, mas podia ser melhor cuidado.",
    ];
    const TWO: &[&str] = &[
        "Estrutura antiga, poderia melhorar.",
        "Pouca vaga no horário de pico.",
        "Meio escondido, difícil de achar.",
    ];
    const ONE: &[&str] = &[
        "Vaga insuficiente, sempre lotado.",
        "Não recomendo, sem nenhuma segurança.",
    ];
    let pool: &[&str] = match star {
        5 => FIVE,
        4 => FOUR,
        3 => THREE,
        2 => TWO,
        _ => ONE,
    };
    pool[i % pool.len()]
}

/// The bike photos bundled under `web/static/img/`, cycled across the dataset so
/// most (but not all) seeded locations show a real image.
const PHOTOS: &[&str] = &[
    "hero-bike-parking.jpg",
    "street-rack-mint-bike.jpg",
    "mtb-pair-rack.jpg",
    "square-bike-rows.jpg",
    "cyclist-foggy-avenue.jpg",
];

/// ~24 locations around Curitiba landmarks, spanning every freshness bucket,
/// cost kind, type, hours shape and security mix (plans/m1-search-map.md §8).
pub fn mock_parkings() -> Vec<MockParking> {
    let full_week = |open: (u32, u32), close: (u32, u32)| {
        (1..=7).map(|d| (d, open, close, false)).collect::<Vec<_>>()
    };
    vec![
        MockParking {
            name: "Paraciclo Rua XV",
            address: "R. XV de Novembro, 300",
            description: "Racks no calçadão, em frente às lojas.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.429_700,
            lon: -49.270_500,
            hours: full_week((0, 0), (23, 59)),
            hours_unknown: false,
            security: vec![("well_lit", 1), ("cctv", 0), ("indoor", 2)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(3),
            photo: Some("street-rack-mint-bike.jpg"),
        },
        MockParking {
            name: "Bicicletário MON",
            address: "R. Marechal Hermes, 999",
            description: "Bicicletário coberto no Museu Oscar Niemeyer.",
            parking_type: ParkingType::Indoor,
            cost: paid(1000, "BRL", PricingUnit::Day),
            lat: -25.409_000,
            lon: -49.268_000,
            hours: (1..=6).map(|d| (d, (9, 0), (18, 0), false)).collect(),
            hours_unknown: false,
            security: vec![
                ("indoor", 1),
                ("cctv", 1),
                ("staffed", 1),
                ("controlled_access", 1),
                ("well_lit", 1),
            ],
            rating_avg: Some(4.6),
            rating_count: 12,
            verified_days_ago: Some(1),
            photo: Some("hero-bike-parking.jpg"),
        },
        MockParking {
            name: "Bicicletário Rodoferroviária",
            address: "Av. Pres. Affonso Camargo, 330",
            description: "Acesso pela lateral do terminal; vagas cobertas.",
            parking_type: ParkingType::ParkingFacility,
            cost: Cost::Free,
            lat: -25.439_000,
            lon: -49.261_000,
            hours: (1..=7).map(|d| (d, (5, 0), (22, 0), false)).collect(),
            hours_unknown: false,
            security: vec![("cctv", 1), ("well_lit", 1), ("restricted_access", 1)],
            rating_avg: Some(4.1),
            rating_count: 7,
            verified_days_ago: Some(20),
            photo: Some("square-bike-rows.jpg"),
        },
        MockParking {
            name: "Lockers Shopping Estação",
            address: "Av. Sete de Setembro, 2775",
            description: "Armários individuais com tomada.",
            parking_type: ParkingType::Locker,
            cost: paid(500, "BRL", PricingUnit::Hour),
            lat: -25.439_200,
            lon: -49.264_000,
            hours: vec![
                (1, (6, 0), (21, 0), false),
                (2, (6, 0), (21, 0), false),
                (3, (6, 0), (21, 0), false),
                (4, (6, 0), (21, 0), false),
                (5, (6, 0), (21, 0), false),
            ],
            hours_unknown: false,
            security: vec![
                ("restricted_access", 1),
                ("indoor", 1),
                ("dedicated_locking_point", 1),
            ],
            rating_avg: Some(3.8),
            rating_count: 4,
            verified_days_ago: Some(60),
            photo: None,
        },
        MockParking {
            name: "Parque Barigui Portaria",
            address: "Av. Cândido Hartmann, s/n",
            description: "Racks junto à portaria dos ciclistas.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.425_800,
            lon: -49.308_500,
            hours: (1..=7).map(|d| (d, (5, 0), (23, 0), false)).collect(),
            hours_unknown: false,
            security: vec![("well_lit", 1)],
            rating_avg: Some(4.0),
            rating_count: 9,
            verified_days_ago: Some(75),
            photo: Some("mtb-pair-rack.jpg"),
        },
        MockParking {
            name: "Bicicletário Jardim Botânico",
            address: "R. Eng. Ostoja Roguski, 690",
            description: "Vigia no período diurno, próximo à estufa.",
            parking_type: ParkingType::Secured,
            cost: paid(1500, "BRL", PricingUnit::Month),
            lat: -25.443_000,
            lon: -49.238_500,
            hours: (1..=7).map(|d| (d, (6, 0), (20, 0), false)).collect(),
            hours_unknown: false,
            security: vec![
                ("security_guard", 1),
                ("controlled_access", 1),
                ("cctv", 1),
                ("indoor", 1),
                ("dedicated_locking_point", 1),
            ],
            rating_avg: Some(4.9),
            rating_count: 21,
            verified_days_ago: Some(5),
            photo: Some("hero-bike-parking.jpg"),
        },
        MockParking {
            name: "Mercado Municipal Curitiba",
            address: "Av. Sete de Setembro, 1865",
            description: "Racks no estacionamento dos fornecedores.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.435_800,
            lon: -49.266_200,
            hours: vec![
                (1, (4, 0), (16, 0), false),
                (2, (4, 0), (16, 0), false),
                (3, (4, 0), (16, 0), false),
                (4, (4, 0), (16, 0), false),
                (5, (4, 0), (16, 0), false),
                (6, (4, 0), (14, 0), false),
            ],
            hours_unknown: false,
            security: vec![("cctv", 1), ("well_lit", 0)],
            rating_avg: Some(3.5),
            rating_count: 2,
            verified_days_ago: Some(120),
            photo: Some("street-rack-mint-bike.jpg"),
        },
        MockParking {
            name: "Bikes Largo da Ordem",
            address: "Largo Coronel Enéas, 30",
            description: "Suportes em U no Setor Histórico.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.427_000,
            lon: -49.274_500,
            hours: (1..=7).map(|d| (d, (10, 0), (18, 0), false)).collect(),
            hours_unknown: false,
            security: vec![("staffed", 1)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(200),
            photo: None,
        },
        MockParking {
            name: "Bicicletário Praça Tiradentes 24h",
            address: "Praça Tiradentes, s/n",
            description: "Abrigo coberto, aberto o dia todo.",
            parking_type: ParkingType::ParkingFacility,
            cost: Cost::Free,
            lat: -25.428_800,
            lon: -49.271_200,
            hours: vec![
                (1, (0, 0), (23, 59), true),
                (2, (0, 0), (23, 59), true),
                (3, (0, 0), (23, 59), true),
                (4, (0, 0), (23, 59), true),
                (5, (0, 0), (23, 59), true),
                (6, (0, 0), (23, 59), true),
                (7, (0, 0), (23, 59), true),
            ],
            hours_unknown: false,
            security: vec![("cctv", 1), ("well_lit", 1)],
            rating_avg: Some(4.2),
            rating_count: 15,
            verified_days_ago: Some(400),
            photo: Some("cyclist-foggy-avenue.jpg"),
        },
        MockParking {
            name: "Parque Tanguá Bike Hub",
            address: "R. Oswaldo Maciel, s/n",
            description: "Estrutura coberta perto da ciclovia.",
            parking_type: ParkingType::Secured,
            cost: paid(800, "BRL", PricingUnit::Day),
            lat: -25.378_800,
            lon: -49.286_200,
            hours: (1..=7).map(|d| (d, (6, 0), (21, 0), false)).collect(),
            hours_unknown: false,
            security: vec![
                ("controlled_access", 1),
                ("cctv", 1),
                ("dedicated_locking_point", 1),
            ],
            rating_avg: Some(4.4),
            rating_count: 6,
            verified_days_ago: None,
            photo: Some("mtb-pair-rack.jpg"),
        },
        MockParking {
            name: "Arena da Baixada Eventos",
            address: "R. Buenos Aires, 1260",
            description: "Bicicletário apenas em dias de evento.",
            parking_type: ParkingType::Indoor,
            cost: paid(2000, "BRL", PricingUnit::Entry),
            lat: -25.448_500,
            lon: -49.277_000,
            hours: vec![],
            hours_unknown: true,
            security: vec![("security_guard", 1), ("indoor", 1)],
            rating_avg: Some(3.9),
            rating_count: 3,
            verified_days_ago: Some(45),
            photo: None,
        },
        MockParking {
            name: "UFPR Reitoria Racks",
            address: "R. XV de Novembro, 1299",
            description: "Racks na Praça Santos Andrade.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.430_000,
            lon: -49.265_000,
            hours: (1..=6).map(|d| (d, (6, 0), (22, 0), false)).collect(),
            hours_unknown: false,
            security: vec![("well_lit", 1), ("cctv", 0)],
            rating_avg: Some(4.3),
            rating_count: 8,
            verified_days_ago: Some(15),
            photo: Some("square-bike-rows.jpg"),
        },
        MockParking {
            name: "Estacionamento Batel",
            address: "Av. do Batel, 1868",
            description: "Vagas para bikes no pavimento térreo.",
            parking_type: ParkingType::ParkingFacility,
            cost: paid(700, "BRL", PricingUnit::Day),
            lat: -25.441_500,
            lon: -49.287_000,
            hours: (1..=7).map(|d| (d, (0, 0), (23, 59), true)).collect(),
            hours_unknown: false,
            security: vec![("cctv", 1), ("staffed", 1), ("well_lit", 1)],
            rating_avg: Some(3.6),
            rating_count: 5,
            verified_days_ago: Some(95),
            photo: Some("hero-bike-parking.jpg"),
        },
        MockParking {
            name: "BikeLocker Shopping Curitiba",
            address: "R. Brig. Franco, 2300",
            description: "Armários no estacionamento do shopping.",
            parking_type: ParkingType::Locker,
            cost: paid(2500, "BRL", PricingUnit::Month),
            lat: -25.440_500,
            lon: -49.288_000,
            hours: (1..=6).map(|d| (d, (6, 0), (20, 0), false)).collect(),
            hours_unknown: false,
            security: vec![
                ("restricted_access", 1),
                ("indoor", 1),
                ("cctv", 1),
                ("dedicated_locking_point", 1),
                ("controlled_access", 1),
            ],
            rating_avg: Some(4.8),
            rating_count: 11,
            verified_days_ago: Some(2),
            photo: Some("street-rack-mint-bike.jpg"),
        },
        MockParking {
            name: "Paraciclo Sete de Setembro",
            address: "Av. Sete de Setembro, 3000",
            description: "Paraciclo na esquina, junto à ciclofaixa.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.437_000,
            lon: -49.279_000,
            hours: (1..=7).map(|d| (d, (0, 0), (23, 59), true)).collect(),
            hours_unknown: false,
            security: vec![("well_lit", 0), ("cctv", 0)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(320),
            photo: None,
        },
        MockParking {
            name: "Bicicletário Passeio Público",
            address: "R. Pres. Faria, 51",
            description: "Abrigo com cadeado próprio, na entrada do parque.",
            parking_type: ParkingType::ParkingFacility,
            cost: Cost::Unknown,
            lat: -25.423_000,
            lon: -49.269_500,
            hours: vec![],
            hours_unknown: true,
            security: vec![("indoor", 1)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: None,
            photo: None,
        },
        MockParking {
            name: "Paraciclo Praça do Japão",
            address: "R. Mal. Hermes, 762",
            description: "Racks ao lado da praça, bastante movimentada.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.447_000,
            lon: -49.282_000,
            hours: (1..=7).map(|d| (d, (0, 0), (23, 59), true)).collect(),
            hours_unknown: false,
            security: vec![],
            rating_avg: Some(3.0),
            rating_count: 1,
            verified_days_ago: Some(150),
            photo: Some("cyclist-foggy-avenue.jpg"),
        },
        MockParking {
            name: "Bike Point Praça Osório",
            address: "Praça Gen. Osório, s/n",
            description: "Vaga vigiada pelo prédio vizinho.",
            parking_type: ParkingType::Secured,
            cost: paid(600, "BRL", PricingUnit::Day),
            lat: -25.432_000,
            lon: -49.273_500,
            hours: (1..=6).map(|d| (d, (7, 0), (19, 0), false)).collect(),
            hours_unknown: false,
            security: vec![
                ("security_guard", 1),
                ("cctv", 1),
                ("indoor", 1),
                ("well_lit", 1),
            ],
            rating_avg: Some(4.0),
            rating_count: 4,
            verified_days_ago: Some(28),
            photo: Some("mtb-pair-rack.jpg"),
        },
        MockParking {
            name: "Bicicletário Teatro Guaíra",
            address: "R. XV de Novembro, 971",
            description: "Coberto, ao lado do teatro.",
            parking_type: ParkingType::Indoor,
            cost: Cost::Free,
            lat: -25.431_800,
            lon: -49.268_800,
            hours: (2..=7).map(|d| (d, (9, 0), (21, 0), false)).collect(),
            hours_unknown: false,
            security: vec![("staffed", 1), ("indoor", 1), ("well_lit", 1)],
            rating_avg: Some(4.5),
            rating_count: 13,
            verified_days_ago: Some(9),
            photo: Some("hero-bike-parking.jpg"),
        },
        MockParking {
            name: "Racks PUCPR Prado Velho",
            address: "R. Imac. Conceição, 1155",
            description: "Próximo ao paraciclo do campus.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.450_000,
            lon: -49.250_000,
            hours: (1..=6).map(|d| (d, (6, 0), (21, 0), false)).collect(),
            hours_unknown: false,
            security: vec![("well_lit", 1)],
            rating_avg: Some(3.7),
            rating_count: 2,
            verified_days_ago: Some(240),
            photo: None,
        },
        MockParking {
            name: "Bicicletário Terminal Guadalupe",
            address: "R. João Negrão, 340",
            description: "Área coberta no terminal de ônibus.",
            parking_type: ParkingType::ParkingFacility,
            cost: paid(300, "BRL", PricingUnit::Day),
            lat: -25.434_000,
            lon: -49.272_000,
            hours: (1..=7).map(|d| (d, (4, 0), (23, 0), false)).collect(),
            hours_unknown: false,
            security: vec![("cctv", 1), ("restricted_access", 0)],
            rating_avg: Some(3.2),
            rating_count: 6,
            verified_days_ago: Some(70),
            photo: Some("square-bike-rows.jpg"),
        },
        MockParking {
            name: "Suportes Parque São Lourenço",
            address: "R. Mateus Leme, 4700",
            description: "Suportes em U próximos à entrada.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.393_000,
            lon: -49.256_000,
            hours: (1..=7).map(|d| (d, (5, 0), (22, 0), false)).collect(),
            hours_unknown: false,
            security: vec![("well_lit", 1), ("security_guard", 0)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(30),
            photo: Some("cyclist-foggy-avenue.jpg"),
        },
        MockParking {
            name: "Bike Point Alto da XV",
            address: "R. XV de Novembro, 2500",
            description: "Vagas com ponto de fixação dedicado.",
            parking_type: ParkingType::Secured,
            cost: paid(1200, "BRL", PricingUnit::Month),
            lat: -25.427_000,
            lon: -49.256_000,
            hours: (1..=7).map(|d| (d, (6, 0), (22, 0), false)).collect(),
            hours_unknown: false,
            security: vec![
                ("dedicated_locking_point", 1),
                ("controlled_access", 1),
                ("cctv", 1),
                ("indoor", 1),
            ],
            rating_avg: Some(4.7),
            rating_count: 10,
            verified_days_ago: Some(4),
            photo: Some("street-rack-mint-bike.jpg"),
        },
        MockParking {
            name: "Racks Praça Rui Barbosa",
            address: "Praça Rui Barbosa, s/n",
            description: "Racks na praça, junto aos terminais de ônibus.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.435_000,
            lon: -49.272_000,
            hours: (1..=7).map(|d| (d, (0, 0), (23, 59), true)).collect(),
            hours_unknown: false,
            security: vec![("well_lit", 1)],
            rating_avg: Some(3.9),
            rating_count: 3,
            verified_days_ago: Some(48),
            photo: Some("mtb-pair-rack.jpg"),
        },
        // Additional downtown spots, all within ~800 m of the "Rua XV de
        // Novembro" fake-geocoder centroid (Problem #3): with the 9 existing
        // locations already inside 1 km, this brings the total past the P2
        // default page size (20), so the default search reaches a second
        // page. `verified_days_ago` spans every freshness bucket (including
        // `None`, never verified) so the recently-verified sort is meaningful.
        MockParking {
            name: "Racks Rua Marechal Deodoro",
            address: "R. Marechal Deodoro, 500",
            description: "Suportes em frente ao prédio histórico.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.427_322,
            lon: -49.273_300,
            hours: (1..=7).map(|d| (d, (6, 0), (22, 0), false)).collect(),
            hours_unknown: false,
            security: vec![("well_lit", 1), ("cctv", 0)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: None,
            photo: None,
        },
        MockParking {
            name: "Paraciclo Rua Riachuelo",
            address: "R. Riachuelo, 300",
            description: "Paraciclo na calçada, movimento constante.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.427_033,
            lon: -49.272_808,
            hours: (1..=7).map(|d| (d, (0, 0), (23, 59), true)).collect(),
            hours_unknown: false,
            security: vec![("cctv", 1)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(2),
            photo: Some("street-rack-mint-bike.jpg"),
        },
        MockParking {
            name: "Bicicletário Praça Zacarias",
            address: "Praça Zacarias, s/n",
            description: "Bicicletário coberto ao lado da praça.",
            parking_type: ParkingType::Indoor,
            cost: paid(800, "BRL", PricingUnit::Day),
            lat: -25.426_947,
            lon: -49.272_131,
            hours: (1..=6).map(|d| (d, (7, 0), (21, 0), false)).collect(),
            hours_unknown: false,
            security: vec![("indoor", 1), ("cctv", 1), ("staffed", 1)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(5),
            photo: Some("hero-bike-parking.jpg"),
        },
        MockParking {
            name: "Racks Rua Comendador Araújo",
            address: "R. Comendador Araújo, 250",
            description: "Racks junto às lojas do térreo.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.427_133,
            lon: -49.271_369,
            hours: (1..=7).map(|d| (d, (0, 0), (23, 59), true)).collect(),
            hours_unknown: false,
            security: vec![("well_lit", 1)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(12),
            photo: None,
        },
        MockParking {
            name: "Paraciclo Alameda Dr. Muricy",
            address: "Al. Dr. Muricy, 100",
            description: "Paraciclo na alameda arborizada.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.427_623,
            lon: -49.270_651,
            hours: (1..=5).map(|d| (d, (7, 0), (19, 0), false)).collect(),
            hours_unknown: false,
            security: vec![("cctv", 0), ("well_lit", 1)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(25),
            photo: Some("square-bike-rows.jpg"),
        },
        MockParking {
            name: "Bikes Rua Emiliano Perneta",
            address: "R. Emiliano Perneta, 700",
            description: "Armários no subsolo do edifício comercial.",
            parking_type: ParkingType::Locker,
            cost: paid(400, "BRL", PricingUnit::Hour),
            lat: -25.428_400,
            lon: -49.270_117,
            hours: (1..=6).map(|d| (d, (6, 0), (22, 0), false)).collect(),
            hours_unknown: false,
            security: vec![("restricted_access", 1), ("indoor", 1)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: None,
            photo: None,
        },
        MockParking {
            name: "Racks Rua Voluntários da Pátria",
            address: "R. Voluntários da Pátria, 445",
            description: "Racks junto à ciclofaixa da rua.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.429_399,
            lon: -49.269_894,
            hours: (1..=7).map(|d| (d, (0, 0), (23, 59), true)).collect(),
            hours_unknown: false,
            security: vec![],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(35),
            photo: Some("mtb-pair-rack.jpg"),
        },
        MockParking {
            name: "Bicicletário Rua Ébano Pereira",
            address: "R. Ébano Pereira, 90",
            description: "Vagas mensalistas com controle de acesso.",
            parking_type: ParkingType::Secured,
            cost: paid(1000, "BRL", PricingUnit::Month),
            lat: -25.430_512,
            lon: -49.270_081,
            hours: (1..=5).map(|d| (d, (6, 0), (21, 0), false)).collect(),
            hours_unknown: false,
            security: vec![
                ("dedicated_locking_point", 1),
                ("controlled_access", 1),
                ("cctv", 1),
            ],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(50),
            photo: Some("cyclist-foggy-avenue.jpg"),
        },
        MockParking {
            name: "Paraciclo Rua José Loureiro",
            address: "R. José Loureiro, 480",
            description: "Paraciclo simples, uso livre.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.431_598,
            lon: -49.270_728,
            hours: (1..=7).map(|d| (d, (0, 0), (23, 59), true)).collect(),
            hours_unknown: false,
            security: vec![("well_lit", 0)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(70),
            photo: None,
        },
        MockParking {
            name: "Racks Rua Trajano Reis",
            address: "R. Trajano Reis, 60",
            description: "Racks próximos ao comércio popular.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.432_501,
            lon: -49.271_825,
            hours: (1..=6).map(|d| (d, (6, 0), (20, 0), false)).collect(),
            hours_unknown: false,
            security: vec![("cctv", 1)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(85),
            photo: Some("street-rack-mint-bike.jpg"),
        },
        MockParking {
            name: "Bikes Rua Barão do Rio Branco",
            address: "R. Barão do Rio Branco, 300",
            description: "Vagas cobertas no estacionamento do edifício.",
            parking_type: ParkingType::ParkingFacility,
            cost: paid(300, "BRL", PricingUnit::Day),
            lat: -25.433_071,
            lon: -49.273_300,
            hours: (1..=7).map(|d| (d, (5, 0), (23, 0), false)).collect(),
            hours_unknown: false,
            security: vec![("cctv", 1), ("well_lit", 1), ("staffed", 0)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(100),
            photo: Some("hero-bike-parking.jpg"),
        },
        MockParking {
            name: "Bicicletário Rua Desembargador Motta",
            address: "R. Desembargador Motta, 300",
            description: "Bicicletário no saguão do edifício comercial.",
            parking_type: ParkingType::Indoor,
            cost: Cost::Free,
            lat: -25.433_184,
            lon: -49.275_021,
            hours: vec![],
            hours_unknown: true,
            security: vec![("indoor", 1)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(130),
            photo: None,
        },
        MockParking {
            name: "Racks Rua Conselheiro Laurindo",
            address: "R. Conselheiro Laurindo, 200",
            description: "Racks na calçada larga, sombreados.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.432_760,
            lon: -49.276_808,
            hours: (1..=7).map(|d| (d, (0, 0), (23, 59), true)).collect(),
            hours_unknown: false,
            security: vec![],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(160),
            photo: Some("mtb-pair-rack.jpg"),
        },
        MockParking {
            name: "Paraciclo Rua Presidente Faria",
            address: "R. Pres. Faria, 100",
            description: "Paraciclo próximo à entrada do parque.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.431_779,
            lon: -49.278_450,
            hours: (1..=7).map(|d| (d, (6, 0), (22, 0), false)).collect(),
            hours_unknown: false,
            security: vec![("well_lit", 1)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: None,
            photo: Some("square-bike-rows.jpg"),
        },
        MockParking {
            name: "Bikes Praça Carlos Gomes",
            address: "Praça Carlos Gomes, s/n",
            description: "Vaga vigiada junto ao teatro.",
            parking_type: ParkingType::Secured,
            cost: paid(900, "BRL", PricingUnit::Day),
            lat: -25.430_288,
            lon: -49.279_733,
            hours: (1..=6).map(|d| (d, (7, 0), (20, 0), false)).collect(),
            hours_unknown: false,
            security: vec![("security_guard", 1), ("cctv", 1), ("indoor", 1)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(190),
            photo: Some("cyclist-foggy-avenue.jpg"),
        },
        MockParking {
            name: "Racks Rua Cruz Machado",
            address: "R. Cruz Machado, 100",
            description: "Racks simples, sem cobertura.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.428_400,
            lon: -49.280_263,
            hours: (1..=7).map(|d| (d, (0, 0), (23, 59), true)).collect(),
            hours_unknown: false,
            security: vec![("cctv", 0), ("well_lit", 0)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(220),
            photo: None,
        },
        MockParking {
            name: "Bicicletário Rua Inácio Lustosa",
            address: "R. Inácio Lustosa, 300",
            description: "Vagas cobertas junto ao estacionamento.",
            parking_type: ParkingType::ParkingFacility,
            cost: paid(500, "BRL", PricingUnit::Day),
            lat: -25.426_401,
            lon: -49.280_111,
            hours: (1..=7).map(|d| (d, (6, 0), (22, 0), false)).collect(),
            hours_unknown: false,
            security: vec![("restricted_access", 1), ("cctv", 1)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(260),
            photo: Some("street-rack-mint-bike.jpg"),
        },
        MockParking {
            name: "Paraciclo Rua Saldanha Marinho",
            address: "R. Saldanha Marinho, 100",
            description: "Paraciclo pequeno, uso livre.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.424_493,
            lon: -49.279_255,
            hours: (1..=5).map(|d| (d, (8, 0), (18, 0), false)).collect(),
            hours_unknown: false,
            security: vec![("well_lit", 1)],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: None,
            photo: None,
        },
        MockParking {
            name: "Racks Rua Doutor Faivre",
            address: "R. Dr. Faivre, 300",
            description: "Racks junto ao ponto de ônibus.",
            parking_type: ParkingType::Rack,
            cost: Cost::Free,
            lat: -25.422_877,
            lon: -49.277_743,
            hours: (1..=7).map(|d| (d, (0, 0), (23, 59), true)).collect(),
            hours_unknown: false,
            security: vec![],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(340),
            photo: Some("mtb-pair-rack.jpg"),
        },
        MockParking {
            name: "Bikes Rua Duque de Caxias",
            address: "R. Duque de Caxias, 150",
            description: "Armários mensalistas com controle de acesso.",
            parking_type: ParkingType::Locker,
            cost: paid(2000, "BRL", PricingUnit::Month),
            lat: -25.421_736,
            lon: -49.275_697,
            hours: (1..=5).map(|d| (d, (6, 0), (20, 0), false)).collect(),
            hours_unknown: false,
            security: vec![
                ("dedicated_locking_point", 1),
                ("indoor", 1),
                ("controlled_access", 1),
                ("cctv", 1),
            ],
            rating_avg: None,
            rating_count: 0,
            verified_days_ago: Some(400),
            photo: Some("hero-bike-parking.jpg"),
        },
    ]
}

/// Deterministic photo assignment used when a mock row leaves `photo` as the
/// sentinel — kept for callers that want a stable image for any index.
pub fn photo_for_index(i: usize) -> &'static str {
    PHOTOS[i % PHOTOS.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_ratings_sum_matches_rounded_target_and_stays_in_bounds() {
        for (avg, count) in [(4.6, 12), (4.1, 7), (3.8, 4), (4.9, 21), (3.0, 1), (4.0, 9)] {
            let stars = star_ratings_for(avg, count);
            assert_eq!(stars.len(), count as usize);
            assert!(stars.iter().all(|s| (1..=5).contains(s)));
            let sum: i64 = stars.iter().map(|s| i64::from(*s)).sum();
            assert_eq!(sum, (avg * count as f64).round() as i64);
        }
    }

    #[test]
    fn star_ratings_empty_for_zero_count() {
        assert!(star_ratings_for(4.5, 0).is_empty());
    }

    #[test]
    fn review_author_pool_covers_the_highest_seeded_rating_count() {
        let max_count = mock_parkings().iter().map(|m| m.rating_count).max().unwrap_or(0);
        assert!(
            REVIEW_AUTHORS.len() as i64 >= max_count,
            "need at least {max_count} distinct authors, have {}",
            REVIEW_AUTHORS.len()
        );
    }

    #[test]
    fn at_least_25_active_mock_locations_within_1km_of_the_centroid() {
        // Mirrors the `ST_DWithin` check the db_test runs against the seeded
        // rows; this is the cheap, DB-less version (Problem #3).
        const CENTROID: (f64, f64) = (-25.4284, -49.2733);
        fn meters(lat: f64, lon: f64) -> f64 {
            let dlat = (lat - CENTROID.0) * 111_320.0;
            let dlon = (lon - CENTROID.1) * 111_320.0 * CENTROID.0.to_radians().cos();
            (dlat * dlat + dlon * dlon).sqrt()
        }
        let within = mock_parkings()
            .iter()
            .filter(|m| meters(m.lat, m.lon) <= 1000.0)
            .count();
        assert!(within >= 25, "only {within} mock locations within 1 km");
    }
}

//! i18n formatting unit tests (M8): locale-aware money + date formatting.

use bikesnest_web::i18n::{Locale, Translator};
use bikesnest_web::view::{format_money, iso_datetime_label};

fn en() -> Translator {
    Translator::new(Locale::En)
}

fn pt() -> Translator {
    Translator::new(Locale::PtBr)
}

#[test]
fn money_uses_locale_decimal_separator() {
    assert_eq!(format_money(en(), 1234.56), "1234.56");
    // pt-BR swaps the decimal separator to a comma.
    assert_eq!(format_money(pt(), 1234.56), "1234,56");
}

#[test]
fn datetime_uses_locale_order() {
    let dt = chrono::DateTime::parse_from_rfc3339("2024-03-05T14:30:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    // en keeps ISO-ish ordering.
    assert_eq!(iso_datetime_label(en(), dt), "2024-03-05 14:30");
    // pt-BR uses day/month/year.
    assert_eq!(iso_datetime_label(pt(), dt), "05/03/2024 14:30");
}

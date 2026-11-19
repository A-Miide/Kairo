//! QR codes for slides and themes (Theme Studio's QR tool): a giving page, a sign-up form or the
//! church's site, scanned from the screen. The server encodes; the editor draws (dot and corner
//! shapes, colours, a logo, a "Scan me" frame) as an SVG, sharp at any size, kept in the layer.

use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use qrcode::{Color, EcLevel, QrCode};
use serde_json::json;

/// Longer than a QR code a screen across a room can still be scanned from.
const MAX_CHARS: usize = 1_000;

#[derive(serde::Deserialize)]
pub struct Params {
    text: String,
    /// "H" when a logo covers the middle: the code still reads with 30% of it hidden. Else "M",
    /// enough for a little glare off a projector.
    ec: Option<String>,
}

/// `GET /api/qr?text=…[&ec=H]`: `{ size, modules }`, the grid row by row, "1" for a dark module.
pub async fn matrix(Query(p): Query<Params>) -> Response {
    let text = p.text.trim();
    if text.is_empty() || text.chars().count() > MAX_CHARS {
        return (StatusCode::BAD_REQUEST, format!("A QR code holds 1 to {MAX_CHARS} characters")).into_response();
    }
    let ec = if p.ec.as_deref() == Some("H") { EcLevel::H } else { EcLevel::M };
    let code = match QrCode::with_error_correction_level(text.as_bytes(), ec) {
        Ok(c) => c,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("That can't be made into a QR code: {e}")).into_response(),
    };
    Json(json!({ "size": code.width(), "modules": modules(&code) })).into_response()
}

fn modules(code: &QrCode) -> String {
    code.to_colors().iter().map(|c| if *c == Color::Dark { '1' } else { '0' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_is_square_with_its_finder_corners_dark() {
        let code = QrCode::with_error_correction_level(b"https://example.org/give", EcLevel::M).unwrap();
        let (n, m) = (code.width(), modules(&code));
        assert_eq!(m.len(), n * n);
        let at = |x: usize, y: usize| m.as_bytes()[y * n + x] == b'1';
        assert!(at(0, 0) && at(n - 1, 0) && at(0, n - 1));
    }
}

//! Rough cost estimation for the dashboard's spend analytics.
//!
//! Providers don't reliably stream a price, so we estimate from billed
//! input/output tokens using a small public-list-price table (USD per million
//! tokens), matched by a substring of the model id. Cache-discounted Anthropic
//! reads are billed separately and excluded from `input`, so an estimate from
//! `input`/`output` tracks the real bill reasonably well. Unknown models price
//! at zero (the UI shows "n/a" rather than a wrong number).

/// (input $/Mtok, output $/Mtok). Order matters: most specific substrings first.
const PRICES: &[(&str, f64, f64)] = &[
    // Anthropic
    ("claude-opus", 15.0, 75.0),
    ("claude-haiku", 0.80, 4.0),
    ("claude-sonnet", 3.0, 15.0),
    ("opus", 15.0, 75.0),
    ("haiku", 0.80, 4.0),
    ("sonnet", 3.0, 15.0),
    // OpenAI
    ("gpt-4o-mini", 0.15, 0.60),
    ("gpt-4o", 2.50, 10.0),
    ("gpt-4.1-mini", 0.40, 1.60),
    ("gpt-4.1", 2.0, 8.0),
    ("o4-mini", 1.10, 4.40),
    ("o3-mini", 1.10, 4.40),
    ("o3", 2.0, 8.0),
    // Google
    ("gemini-2.5-pro", 1.25, 10.0),
    ("gemini-1.5-pro", 1.25, 5.0),
    ("gemini", 0.10, 0.40), // flash-class default
    // Others
    ("deepseek", 0.27, 1.10),
    ("grok", 2.0, 10.0),
];

/// Per-million-token (input, output) list price for `model`, if known.
fn price_of(model: &str) -> Option<(f64, f64)> {
    let m = model.to_ascii_lowercase();
    PRICES
        .iter()
        .find(|(needle, _, _)| m.contains(needle))
        .map(|(_, pin, pout)| (*pin, *pout))
}

/// Whether we have list pricing for `model` (so the UI can show "n/a" instead
/// of a misleading $0.00 for unknown/local models).
pub fn is_priced(model: &str) -> bool {
    price_of(model).is_some()
}

/// Estimated USD cost of one request's billed `input`/`output` tokens.
pub fn estimate(model: &str, input: u32, output: u32) -> f64 {
    match price_of(model) {
        Some((pin, pout)) => {
            (input as f64 / 1_000_000.0) * pin + (output as f64 / 1_000_000.0) * pout
        }
        None => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_models_price_nonzero() {
        // 1M in + 1M out of sonnet = $3 + $15.
        assert!((estimate("claude-sonnet-4-5", 1_000_000, 1_000_000) - 18.0).abs() < 1e-9);
        assert!(is_priced("gpt-4o-mini"));
        assert!(estimate("gpt-4o-mini", 1_000_000, 0) > 0.0);
    }

    #[test]
    fn mini_matches_before_base() {
        // "gpt-4o-mini" must not be priced as "gpt-4o".
        let mini = estimate("gpt-4o-mini", 1_000_000, 0);
        let base = estimate("gpt-4o", 1_000_000, 0);
        assert!(mini < base);
    }

    #[test]
    fn unknown_model_is_zero_and_unpriced() {
        assert_eq!(estimate("some-local-llama", 1_000_000, 1_000_000), 0.0);
        assert!(!is_priced("some-local-llama"));
    }
}

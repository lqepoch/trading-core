//! Stable exchange-neutral domain contracts. No API secrets and no broker-specific SDK types.
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Right {
    Call,
    Put,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OccContract {
    pub underlying: String,
    pub expiration: NaiveDate,
    pub right: Right,
    pub strike: f64,
}

#[derive(Debug, Error, PartialEq)]
pub enum ContractError {
    #[error("invalid OCC option symbol")]
    InvalidOcc,
}

pub fn parse_occ(symbol: &str) -> Result<OccContract, ContractError> {
    if !symbol.is_ascii() || symbol.len() <= 15 {
        return Err(ContractError::InvalidOcc);
    }
    let (root, suffix) = symbol.split_at(symbol.len() - 15);
    if root.is_empty() || root.len() > 6 || !root.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(ContractError::InvalidOcc);
    }
    let expiration =
        NaiveDate::parse_from_str(&suffix[..6], "%y%m%d").map_err(|_| ContractError::InvalidOcc)?;
    let right = match &suffix[6..7] {
        "C" => Right::Call,
        "P" => Right::Put,
        _ => return Err(ContractError::InvalidOcc),
    };
    let strike = suffix[7..]
        .parse::<u64>()
        .map_err(|_| ContractError::InvalidOcc)? as f64
        / 1000.0;
    if strike <= 0.0 {
        return Err(ContractError::InvalidOcc);
    }
    Ok(OccContract {
        underlying: root.to_owned(),
        expiration,
        right,
        strike,
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct StockSnapshot {
    pub symbol: String,
    pub last: Option<f64>,
    pub open: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub previous_close: Option<f64>,
    pub change_percent: Option<f64>,
    pub bid: Option<f64>,
    pub ask: Option<f64>,
    pub volume: Option<f64>,
    /// Legacy trade-only timestamp retained for existing clients.
    pub updated_at: Option<String>,
    pub last_basis: Option<String>,
    pub last_as_of: Option<String>,
    pub quote_at: Option<String>,
    pub daily_bar_at: Option<String>,
    pub previous_daily_bar_at: Option<String>,
    pub feed: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Bar {
    pub time: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct OptionSnapshot {
    pub symbol: String,
    pub underlying: String,
    pub expiration: String,
    pub right: Right,
    pub strike: f64,
    pub bid: Option<f64>,
    pub ask: Option<f64>,
    pub last: Option<f64>,
    pub bid_size: Option<f64>,
    pub ask_size: Option<f64>,
    pub iv: Option<f64>,
    pub delta: Option<f64>,
    pub gamma: Option<f64>,
    pub theta: Option<f64>,
    pub vega: Option<f64>,
    /// Legacy quote-first timestamp retained for existing clients.
    pub updated_at: Option<String>,
    pub quote_at: Option<String>,
    pub trade_at: Option<String>,
    /// Alpaca does not currently provide a dedicated IV/Greeks model timestamp.
    pub model_as_of: Option<String>,
    pub feed: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MarketEvent {
    StockQuote {
        symbol: String,
        bid: Option<f64>,
        ask: Option<f64>,
        timestamp: String,
    },
    StockTrade {
        symbol: String,
        price: f64,
        size: f64,
        timestamp: String,
    },
    OptionQuote {
        symbol: String,
        bid: Option<f64>,
        ask: Option<f64>,
        bid_size: Option<f64>,
        ask_size: Option<f64>,
        timestamp: String,
    },
    OptionTrade {
        symbol: String,
        price: f64,
        size: f64,
        timestamp: String,
    },
    FeedStatus {
        feed: String,
        state: String,
        timestamp: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_standard_and_weekly_symbols() {
        let c = parse_occ("QQQ261007P00600000").unwrap();
        assert_eq!(c.underlying, "QQQ");
        assert_eq!(c.expiration.to_string(), "2026-10-07");
        assert_eq!(c.right, Right::Put);
        assert!((c.strike - 600.0).abs() < 1e-12);
        let w = parse_occ("SPXW261009C06000000").unwrap();
        assert_eq!(w.underlying, "SPXW");
        assert_eq!(w.right, Right::Call);
    }

    #[test]
    fn rejects_bad_occ() {
        for s in [
            "QQQ",
            "QQQ261007X00600000",
            "QQQ261032P00600000",
            "QQQ261007P00000000",
        ] {
            assert_eq!(parse_occ(s).unwrap_err(), ContractError::InvalidOcc);
        }
    }
}

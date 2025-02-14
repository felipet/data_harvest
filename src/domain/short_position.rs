// Copyright 2025 Felipe Torres González
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Module that includes the definition of the ShortPosition data object and other related stuff.

use crate::domain::{CnmvError, DataProviderError};
use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use regex::Regex;
use std::fmt;

/// Wrapper for a Result enum that might contain [ShortPosition] entries.
pub type ShortResult = Result<Vec<ShortPosition>, DataProviderError>;

/// Short position entry.
#[derive(Default, Debug, PartialEq)]
pub struct ShortPosition {
    /// This is the name of the investment fund that owns the short position.
    pub owner: String,
    /// This is a percentage over the company's total capitalization that indicates
    /// the amount of shares sold in short by the owner against the value of the
    /// company.
    pub weight: f32,
    /// Date in which the short position was stated.
    pub open_date: DateTime<Utc>,
    /// The ticker of the asset.
    pub ticker: String,
}

impl fmt::Display for ShortPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} - {} ({})", self.owner, self.weight, self.open_date)
    }
}

/// Container of active short positions of a company.
///
/// # Description
///
/// This `struct` gathers all the active short positions of a company. It is alike to
/// the table shown in the web page when checking for the short positions of a company.
///
/// Short positions are stated once per day, no later than 15:30. Thus a full timestamp
/// is not really useful. Only the date is kept for the entries.
#[derive(Debug)]
pub struct AliveShortPositions {
    /// Summation of all the active [ShortPosition::weight] of the company.
    pub total: f32,
    /// Collection of active [ShortPosition] for a company.
    pub positions: Vec<ShortPosition>,
    /// Timestamp of the active positions.
    pub date: DateTime<Utc>,
}

impl AliveShortPositions {
    /// Constructor of the [AliveShortPositions] class.
    pub fn new() -> AliveShortPositions {
        AliveShortPositions {
            total: 0.0,
            positions: Vec::new(),
            date: Utc::now(),
        }
    }
}

impl Default for AliveShortPositions {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AliveShortPositions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for position in self.positions.iter() {
            writeln!(
                f,
                "✓ {}: {}% ({})",
                position.owner.as_str(),
                position.weight,
                position.open_date
            )?;
        }

        Ok(())
    }
}

/// Data type that checks whether a response for a short position request succeeded or not.
#[derive(Debug)]
pub struct ShortResponse(String);

impl ShortResponse {
    /// Use this method to check whether a response of the GET method returned valid
    /// content or not.
    pub fn parse(s: String) -> Result<Self, CnmvError> {
        static REG_ISIN: Lazy<Regex> = Lazy::new(|| Regex::new(r"ES(\d){10}").unwrap());

        if s.contains("No ha sido posible completar su consulta") {
            return Err(CnmvError::ExternalError(
                "The request could be processed by the external server".to_owned(),
            ));
        }

        match s.find("No se han encontrado datos disponibles") {
            Some(_) => match s.find("Serie histórica") {
                Some(_) => Ok(Self(s)),
                // Companies with a lack of historic (see Puig Brands) fall in this branch, though it is not an error.
                None => {
                    if REG_ISIN.is_match(&s) {
                        Ok(Self(s))
                    } else {
                        Err(CnmvError::UnknownCompany)
                    }
                }
            },
            None => Ok(Self(s)),
        }
    }
}

impl AsRef<str> for ShortResponse {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::*;
    use std::fs::read_to_string;

    #[fixture]
    #[once]
    fn short_valid_fixture() -> String {
        read_to_string("test/fixtures/valid_short_response.html")
            .expect("Failed to read the valid short response fixture")
    }

    #[fixture]
    #[once]
    fn short_invalid_fixture() -> String {
        read_to_string("test/fixtures/invalid_short_response.html")
            .expect("Failed to read the valid short response fixture")
    }

    #[fixture]
    #[once]
    fn short_invalid_fixture_unknown() -> String {
        read_to_string("test/fixtures/invalid_short_response_unknown_error.html")
            .expect("Failed to read the valid short response fixture")
    }

    #[fixture]
    #[once]
    fn short_valid_fixture_puig() -> String {
        read_to_string("test/fixtures/valid_short_response_puig.html")
            .expect("Failed to read the valid short response fixture")
    }

    #[rstest]
    fn parse_valid_response(short_valid_fixture: &String) -> Result<(), CnmvError> {
        ShortResponse::parse(short_valid_fixture.clone())?;

        Ok(())
    }

    #[rstest]
    fn parse_valid_response_puig(short_valid_fixture_puig: &String) -> Result<(), CnmvError> {
        ShortResponse::parse(short_valid_fixture_puig.clone())?;

        Ok(())
    }

    #[rstest]
    fn parse_invalid_response(short_invalid_fixture: &String) {
        assert!(ShortResponse::parse(short_invalid_fixture.clone()).is_err());
    }

    #[rstest]
    fn parse_invalid_response_unknown(short_invalid_fixture_unknown: &String) {
        assert!(ShortResponse::parse(short_invalid_fixture_unknown.clone()).is_err());
    }
}

// Copyright 2025 Felipe Torres González
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! # Finance Data Harvest Library
//!
//! This library includes several modules that extract finance and stock related data from web sites. The main purpose
//! of the library is to retrieve useful data for financial quantitative analysis, and organise it into a data base.
//!
//! ## Supported Sources of Data
//!
//! At the moment the following sources of data are supported:
//!
//! ### [Comisión Nacional de Valores](https://www.cnmv.es)
//!
//! The CNMV is the Spanish stock exchange supervisor and regulator. Its web site includes many useful registers,
//! though the access is complicated due to the organisation of the web site, and certainly not friendly for
//! machines.
//!
//! The library supports extracting data about [short positions](https://www.cnmv.es/portal/consultas/busqueda?id=29)
//! against companies listed in the IBEX35.
//!
//! #### [Short Data Provider][web_scrappers::CnmvProvider]
//!
//! This `struct` [web_scrappers::CnmvProvider] includes logic to extract the active short positions against a
//! given company of the IBEX35. The module works together with the API defined in the
//! [Finance Lib](https://crates.io/crates/finance_api) crate, and uses the implementation of such API
//! [Finance Ibex](https://crates.io/crates/finance_ibex). So have a look at them before using this module.
//!
//! ## API
//!
//! There is no defined API of this library because the main goal of the library is to keep a private data base with
//! all the harvested data. Hence if you just need to scrap data from a supported web site, look for any of the
//! modules within [web_scrappers].
//!
//! ## Data Base Management
//!
//! The modules within [feeders] are meant to call modules that produce data and push the new data to the private
//! data base.

use chrono::{DateTime, Utc};
use finance_api::Company;

pub mod feeders {
    mod ibex_short_provider;
    pub use ibex_short_provider::IbexShortFeeder;
}

pub mod web_scrappers {
    mod cnmv_scrapper;
    pub use cnmv_scrapper::CnmvProvider;
}

pub mod domain {
    mod errors;
    mod short_position;

    pub use errors::{CnmvError, DataProviderError, DbError};
    pub use short_position::{AliveShortPositions, ShortPosition, ShortResponse, ShortResult};
}

pub enum TimeFrame {
    Current,
    Historical(DateTime<Utc>),
}

pub(crate) use domain::{
    AliveShortPositions, CnmvError, DbError, ShortPosition, ShortResponse, ShortResult,
};

/// Trait ShortDataProvider
///
/// # Description
///
/// This trait describes the common interface for any object that implements a
/// data collector for short positions over stocks of a particular stock exchange.
/// Each market regulator has to provide an open source where investors can retrieve
/// the information of the short positions.
///
/// Given that each regulator does such thing in a different way, the implementation
/// of a data collector is highly dependant on the exchange to which a particular
/// stock belongs to.
///
/// In general, it is expected a descriptor such as [ShortPosition] for each
/// short position. So any data collector object that implements this interface
/// shall parse any source of data including such information, and format it
/// following the data objects included in this library.
///
/// Usually, positions distinguish two types: alive or open positions, i.e. the
/// position holder eventually will need to buy an equal amount of shares to the
/// size of the short position in order to close it; and historical positions, i.e.
/// positions that where opened and closed in the past.
pub trait ShortDataProvider {
    /// Method to check if a stock has/had short positions.
    ///
    /// # Description
    ///
    /// The implementation of this method shall check whether a stock has alive
    /// short positions (when `TimeFrame::Current`), or had any short position
    /// in a time window between the time of checking and the date specified
    /// as argument to `TimeFrame::Historical`.
    ///
    /// # Returns
    ///
    /// When `Ok`, an array of [ShortPosition]s is returned. If there were no
    /// positions at the time specified by `time_frame`, an empty array is returned.
    ///
    /// When `Err`, an error of type [DataProviderError] is returned.
    fn get_positions(&self, stock: &impl Company, time_frame: TimeFrame) -> ShortResult;
}

pub trait ShortDataExtractor {
    fn get_positions(&self, stock: &impl Company, time_frame: TimeFrame) -> ShortResult;
}

// Copyright 2025 Felipe Torres González
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Module with definitions for custom error types.

/// Error types for the CNMV handler.
#[derive(Debug)]
pub enum CnmvError {
    /// Error given when the passed company is not recognized by the CNMV' API.
    UnknownCompany,
    /// Error from the external API (CNMV).
    ExternalError(String),
    /// Error for the internal methods.
    InternalError(String),
    /// CNMV identifies companies using ISIN.
    MissingIsin,
}

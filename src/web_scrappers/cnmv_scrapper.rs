// Copyright 2025 Felipe Torres González
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! cnmw_scrapper.rs
//!
//! Module that includes code related to the extraction of data from the web page
//! of the Spanish _Comisión Nacional de Mercado de Valores (CNMV)_.

use crate::{AliveShortPositions, CnmvError, ShortPosition, ShortResponse};
use chrono::{offset::LocalResult, NaiveDate, TimeZone, Utc};
use chrono_tz::Europe::Madrid;
use finance_api::Company;
use finance_ibex::IbexCompany;
use reqwest;
use scraper::{Html, Selector};
use tracing::{error, instrument, trace};

/// Handler to extract data from the CNMV web page.
///
/// # Description
///
/// This object includes several methods that extract information from the CNMV's web
/// page.
///
/// The current list of supported features is:
/// - Extraction of the active short positions of a company (`Consultas a registros oficiales>Entidades emisoras:
///   Información regulada>Posiciones cortas>Notificaciones de posiciones cortas`).
///
/// The endpoint of the web page expects a particular ID, thus using tickers or regular names
/// is not allowed. To avoid handling such type of information, this object works with
/// [IbexCompany] data objects. These are data objects that gather all the information
/// for a particular company.
pub struct CnmvProvider {
    /// The main path of the URL.
    base_url: String,
    /// Path extension for the _PosicionesCortas_ endpoint.
    short_ext: String,
}

/// `enum` to handle what endpoints of the CNMV's API are supported by this module.
#[derive(Debug)]
enum EndpointSel {
    /// EP -> `Consultas a registros oficiales>Entidades emisoras: Información
    /// regulada>Posiciones cortas>Notificaciones de posiciones cortas`
    ShortEP,
}

impl Default for CnmvProvider {
    /// Default implementation delegates to [CnmvProvider::new].
    fn default() -> Self {
        Self::new()
    }
}

impl CnmvProvider {
    /// Class constructor.
    pub fn new() -> CnmvProvider {
        CnmvProvider {
            base_url: String::from("https://www.cnmv.es"),
            short_ext: String::from("Portal/Consultas/EE/PosicionesCortas.aspx?nif="),
        }
    }

    /// Internal method that executes a GET to the CNMV's web page endpoints.
    ///
    /// # Description
    ///
    /// This method's implementation is generic, so it shall be used to retrieve data from any supported endpoint of
    /// CNMV's page. See [EndpointSel] for a full list of the supported endpoints.
    ///
    /// # Returns
    ///
    /// When the HTTP GET operation succeeded, the response will contain all the raw data in a String. Thus this
    /// method does not perform any short of error checking or content parsing beyond assuring that the HTTP request
    /// succeeds (200).
    ///
    /// The following errors might happen:
    /// - [CnmvError::MissingIsin] when the given company has no ISIN. This might happen for companies that are listed
    ///   in the Ibex35 but are not registered in Spain.
    /// - [CnmvError::ExternalError] when any error is returned from the HTTP request.
    #[instrument(
      name = "Collect data from CNMV's page"
      skip(self, stock),
      fields(stock.name=stock.name(), stock.isin=stock.extra_id())
    )]
    async fn collect_data(
        &self,
        endpoint: EndpointSel,
        stock: &IbexCompany,
    ) -> Result<ShortResponse, CnmvError> {
        // Select the endpoint that shall be used for the requested GET.
        let endpoint = match endpoint {
            EndpointSel::ShortEP => &self.short_ext[..],
        };

        // Retrieve the companie's ISIN.
        let isin = match stock.extra_id() {
            Some(isin) => isin,
            None => {
                error!("The given company ({}) has no ISIN", stock.name());
                return Err(CnmvError::MissingIsin);
            }
        };

        let resp = reqwest::get(format!("{}/{endpoint}{isin}", self.base_url))
            .await
            .map_err(|e| CnmvError::ExternalError(e.to_string()))?;

        if resp.status().as_u16() != 200 {
            let error_string = resp.status().as_str().to_string();
            error!("Error found during the request: {error_string}");
            Err(CnmvError::ExternalError(error_string))
        } else {
            let response = ShortResponse::parse(
                resp.text()
                    .await
                    .map_err(|e| CnmvError::InternalError(e.to_string()))?,
            )?;
            trace!("Response: {:?}", response);
            Ok(response)
        }
    }

    /// Method that parses the short positions from CNMV's web site.
    ///
    /// # Description
    ///
    /// This method parses CNMV's web site for checking if a stock has open short positions
    /// against it. Only alive positions are retrieved. The information is encapsulated
    /// in a [AliveShortPositions] struct.
    ///
    /// ## Arguments
    ///
    /// - _stock_: An instance of an [IbexCompany].
    ///
    /// ## Returns
    ///
    /// The method returns a `Result` enum that indicates whether there was an issue checking
    /// the web page. Regardless of the amount of short positions, the result will be `Ok` if
    /// the request to the web page was successful. Open positions are included in the
    /// [positions](AliveShortPositions::positions) field of the struct. If there is no open
    /// position at the moment of checking, an empty collection is included.
    #[instrument(
        name = "Parse data from CNMV's page"
        skip(self, stock),
        fields(stock.name=stock.name(), stock.isin=stock.extra_id())
      )]
    pub async fn short_positions(
        &self,
        stock: &IbexCompany,
    ) -> Result<AliveShortPositions, CnmvError> {
        let raw_data = self.collect_data(EndpointSel::ShortEP, stock).await?;

        let document = Html::parse_document(raw_data.as_ref());
        let selector_td = Selector::parse("td").unwrap();
        let selector_tr = Selector::parse("tr").unwrap();

        let mut positions = Vec::new();

        for element_tr in document.select(&selector_tr) {
            let mut owner: String = String::from("dummy");
            let mut weight: f32 = 0.0;
            let mut date: String = String::from("nodate");
            for td in element_tr.select(&selector_td) {
                if let Some(x) = td.attr("class") {
                    if x == "Izquierda" {
                        owner = String::from(td.text().next().unwrap().trim());
                    }
                } else if let Some(x) = td.attr("data-th") {
                    if x == "% sobre el capital" {
                        weight = td
                            .text()
                            .next()
                            .unwrap()
                            .replace(',', ".")
                            .parse::<f32>()
                            .unwrap();
                    } else if x == "Fecha de la posición" {
                        date = String::from(td.text().next().unwrap());
                    }
                }
            }

            if &owner[..] != "dummy" {
                let date = NaiveDate::parse_from_str(&date, "%d/%m/%Y").map_err(|_| {
                    CnmvError::InternalError(
                        "Failed to parse the short position open date.".to_owned(),
                    )
                })?;

                let open_date =
                    match Madrid.from_local_datetime(&date.and_hms_opt(15, 30, 0).unwrap()) {
                        LocalResult::Single(value) => value.to_utc(),
                        _ => {
                            error!("The given naive date does not convert to UTC.");
                            return Err(CnmvError::InternalError(
                                "Failed to build a valid date.".to_owned(),
                            ));
                        }
                    };

                positions.push(ShortPosition {
                    owner,
                    weight,
                    open_date,
                    ticker: stock.ticker().to_owned(),
                });
            }
        }

        let mut total = 0.0;
        positions
            .iter()
            .for_each(|position| total += position.weight);
        let date = Utc::now();

        Ok(AliveShortPositions {
            total,
            positions,
            date,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use finance_ibex::IbexCompany;
    use rstest::{fixture, rstest};

    #[fixture]
    fn a_company() -> IbexCompany {
        IbexCompany::new(
            Some("Solaria"),
            "SOLARIA",
            "SLR",
            "ES0165386014",
            Some("A83511501"),
        )
    }

    #[fixture]
    fn not_a_company() -> IbexCompany {
        IbexCompany::new(
            Some("Not A Company"),
            "NoCompany",
            "NOC",
            "0",
            Some("A44901010"),
        )
    }

    #[rstest]
    fn collect_data_existing_company(a_company: IbexCompany) {
        // Prepare the test
        let provider = CnmvProvider::new();

        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                // Send a request to the external API
                let raw_content = provider
                    .collect_data(EndpointSel::ShortEP, &a_company)
                    .await;
                assert!(raw_content.is_ok());
            })
    }

    #[rstest]
    fn collect_data_non_existing_company(not_a_company: IbexCompany) {
        // Prepare the test
        let provider = CnmvProvider::new();

        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                // Send a request to the external API
                let raw_content = provider
                    .collect_data(EndpointSel::ShortEP, &not_a_company)
                    .await;

                assert!(raw_content.is_err());
            })
    }

    #[rstest]
    fn short_position_valid_company(a_company: IbexCompany) {
        // Prepare the test
        let provider = CnmvProvider::new();

        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                // Send a request to the external API
                let short_position = provider.short_positions(&a_company).await;
                println!("{:#?}", short_position);
                assert!(short_position.is_ok());
                println!(
                    "Short position of {}:{:#?}",
                    a_company,
                    short_position.unwrap()
                );
            })
    }

    #[rstest]
    fn short_position_non_valid_company(not_a_company: IbexCompany) {
        // Prepare the test
        let provider = CnmvProvider::new();

        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                // Send a request to the external API
                let short_position = provider.short_positions(&not_a_company).await;
                assert!(short_position.is_err());
            })
    }
}

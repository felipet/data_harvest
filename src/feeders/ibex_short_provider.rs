// Copyright 2025 Felipe Torres González
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::{web_scrappers::CnmvProvider, DbError, ShortPosition};
use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone, Utc};
use finance_api::Company;
use finance_ibex::IbexCompany;
use sqlx::{prelude::FromRow, types::Uuid, Executor, PgPool};
use std::error::Error;
use std::sync::Arc;
use tracing::{debug, error, info, instrument, warn};

/// Data provider for short positions against stocks that belong to the Ibex35.
///
/// # Description
///
/// This `struct` automates the process of data harvesting related to short positions
/// against stocks that are included in the Ibex35 index. Most of the included companies
/// are registered in Spain, thus the information related to short positions is provided
/// by the [CNMV](https://www.cnmv.es). However, a few stocks are registered abroad,
/// and the information related to short positions has to be retrieved from a different
/// market regulator.
///
/// This `struct` handles such situation, offering a single entry point for consulting
/// short position of values that are included in the Ibex35. So that situation becomes
/// transparent to the end user.
///
/// Individual modules would implement the logic to extract information from different
/// places. These have to be registered in the object's constructor, which keeps a
/// look-up table that links market regulators with data extractors.
pub struct IbexShortFeeder<'a> {
    pub scrapper: Arc<CnmvProvider>,
    pub pool: &'a PgPool,
}

// Mirror data object of [IbexCompany] to interact with the DB.
#[derive(Debug, FromRow)]
pub struct IbexCompanyBd {
    pub full_name: Option<String>,
    pub name: Option<String>,
    pub ticker: Option<String>,
    pub isin: Option<String>,
    pub extra_id: Option<String>,
}

// Mirror data object of [ShortPosition] to interact with the DB.
#[derive(Debug, FromRow)]
pub struct ShortPositionBd {
    pub id: Option<Uuid>,
    pub owner: Option<String>,
    pub ticker: Option<String>,
    pub weight: Option<f32>,
    pub open_date: Option<NaiveDateTime>,
}

impl TryFrom<&IbexCompanyBd> for IbexCompany {
    type Error = DbError;

    fn try_from(value: &IbexCompanyBd) -> Result<Self, Self::Error> {
        let sname = match value.name.as_deref() {
            Some(name) => name,
            None => {
                return Err(DbError::MissingStockInfo(format!(
                    "Missing name: {:?}",
                    value
                )))
            }
        };
        let fname = value.full_name.as_deref();

        let ticker = match value.ticker.as_deref() {
            Some(ticker) => ticker,
            None => {
                return Err(DbError::MissingStockInfo(format!(
                    "Missing ticker: {:?}",
                    value
                )))
            }
        };

        let isin = match value.isin.as_deref() {
            Some(isin) => isin,
            None => {
                return Err(DbError::MissingStockInfo(format!(
                    "Missing ISIN: {:?}",
                    value
                )))
            }
        };

        let nif = value.extra_id.as_deref();

        Ok(IbexCompany::new(fname, sname, ticker, isin, nif))
    }
}

impl TryFrom<ShortPositionBd> for ShortPosition {
    type Error = DbError;

    fn try_from(value: ShortPositionBd) -> Result<Self, Self::Error> {
        let owner = match value.owner {
            Some(o) => o,
            None => return Err(DbError::MissingStockInfo("Missing owner".to_owned())),
        };

        let weight = match value.weight {
            Some(w) => w,
            None => return Err(DbError::MissingStockInfo("Missing weight".to_owned())),
        };

        let open_date = match value.open_date {
            Some(o) => {
                // Time is kept in UTC within the DB. Left the code in case this changes in the future.
                let tz_offset = FixedOffset::west_opt(0).unwrap();
                let dt_with_tz: DateTime<FixedOffset> = tz_offset.from_local_datetime(&o).unwrap();
                Utc.from_utc_datetime(&dt_with_tz.naive_utc())
            }
            None => return Err(DbError::MissingStockInfo("Missing open date".to_owned())),
        };

        let ticker = match value.ticker {
            Some(t) => t,
            None => return Err(DbError::MissingStockInfo("Missing ticker".to_owned())),
        };

        Ok(ShortPosition {
            owner,
            weight,
            open_date,
            ticker,
        })
    }
}

impl TryFrom<&ShortPositionBd> for ShortPosition {
    type Error = DbError;

    fn try_from(value: &ShortPositionBd) -> Result<Self, Self::Error> {
        let owner = match &value.owner {
            Some(o) => o.to_owned(),
            None => return Err(DbError::MissingStockInfo("Missing owner".to_owned())),
        };

        let weight = match value.weight {
            Some(w) => w,
            None => return Err(DbError::MissingStockInfo("Missing weight".to_owned())),
        };

        let open_date = match value.open_date {
            Some(o) => {
                let tz_offset = FixedOffset::west_opt(0).unwrap();
                let dt_with_tz: DateTime<FixedOffset> = tz_offset.from_local_datetime(&o).unwrap();
                Utc.from_utc_datetime(&dt_with_tz.naive_utc())
            }
            None => return Err(DbError::MissingStockInfo("Missing open date".to_owned())),
        };

        let ticker = match &value.ticker {
            Some(t) => t.to_owned(),
            None => return Err(DbError::MissingStockInfo("Missing ticker".to_owned())),
        };

        Ok(ShortPosition {
            owner,
            weight,
            open_date,
            ticker,
        })
    }
}

impl<'a> IbexShortFeeder<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        IbexShortFeeder {
            scrapper: Arc::new(CnmvProvider::new()),
            pool,
        }
    }

    #[instrument(name = "Refresh short positions", skip(self))]
    pub async fn add_today_data(&self) -> Result<Vec<String>, Box<dyn Error>> {
        // Let's get an updated listing of the Ibex35's companies.
        let companies = self.stock_listing().await?;
        // Keep an array with the tickers that get updated.
        let mut updated_tickers = Vec::new();
        debug!("{} companies listed from the IBEX35", companies.len());

        // For each company, request to the CNMV's site if there's any open short position.
        for company in companies.iter().filter(|x| x.extra_id().is_some()) {
            // Build an array of positions coming from the web (new).
            let new_positions = self
                .scrapper
                .short_positions(company)
                .await
                .map_err(Box::new)?
                .positions;

            // And an array of positions coming from the DB (stored).
            let stored_positions = self.active_positions(company.ticker()).await?;

            // Dummy case: neither record nor new positions were found.
            if stored_positions.is_empty() && new_positions.is_empty() {
                debug!(
                    "The company {} has no open short positions",
                    company.ticker()
                );
                continue;
            // First case: All the sort positions are new (there were no previously registered positions).
            } else if stored_positions.is_empty() {
                warn!(
                    "The company {} got new short positions against it",
                    company.ticker()
                );
                updated_tickers.push(company.ticker().to_owned());

                for position in new_positions {
                    // Store the new position.
                    info!(
                        "No previous short position was stored for {}, recording the new one ({})",
                        position.ticker, position.owner
                    );
                    self.insert_short_position(&position, None).await?;
                }
            // Second case: All the short positions got reduced below the threshold. We need to wipe all the current
            // active positions.
            } else if new_positions.is_empty() {
                warn!(
                    "The company {} got free of significant short positions",
                    company.ticker()
                );
                updated_tickers.push(company.ticker().to_owned());

                for position in stored_positions.iter() {
                    match &position.id {
                        Some(id) => self.wipe_short_position(id).await?,
                        None => error!("Corrupt data in the DB: {:?}", position),
                    }
                }

                info!(
                    "All the active positions got wiped for {}",
                    company.ticker()
                );
            } else {
                let mut insert_ticker = true;
                // First, let's check if any existing position got updated.
                for new_position in new_positions.iter() {
                    let mut found = false;

                    // Got updated or simply exists?
                    for old_position in stored_positions.iter() {
                        let op = ShortPosition::try_from(old_position)?;

                        if *new_position == op {
                            debug!(
                                "The position owned by {} against {} was already in the record",
                                new_position.owner, new_position.ticker
                            );
                            found = true;
                            insert_ticker = false;
                            break;
                        }
                    }

                    // If found is false, either the position is new, or is an update of an existing one.
                    if !found {
                        // Check if it is an update.
                        let previous_active_position = match self
                            .active_position(&new_position.ticker, &new_position.owner)
                            .await?
                        {
                            Some(p) => {
                                info!(
                                    "The position owned by {} against {} got updated",
                                    new_position.owner, new_position.ticker
                                );
                                p.id
                            }
                            None => {
                                warn!(
                                    "A new short position against {} owned by {} got registered",
                                    new_position.ticker, new_position.owner
                                );
                                None
                            }
                        };

                        self.insert_short_position(new_position, previous_active_position)
                            .await?;
                    }
                }

                // Second, let's check if an existing position got wiped.
                for old_position in stored_positions.iter() {
                    let mut found = false;

                    for new_position in new_positions.iter() {
                        let op = ShortPosition::try_from(old_position)?;

                        if new_position.owner == op.owner && new_position.ticker == op.ticker {
                            found = true;
                            break;
                        }
                    }

                    // If the position was not found, it must be have been wiped.
                    if !found {
                        warn!("A previous position owned by {} against {} got reduced below the threshold", old_position.owner.clone().unwrap(), old_position.ticker.clone().unwrap());
                        self.wipe_short_position(&old_position.id.unwrap()).await?;
                        debug!("Active position {} wiped", old_position.id.unwrap());
                        insert_ticker = true;
                    }
                }

                if insert_ticker {
                    updated_tickers.push(company.ticker().to_owned());
                }
            }
        }

        Ok(updated_tickers)
    }

    #[instrument(name = "List the companies of the IBEX35", skip(self))]
    async fn stock_listing(&self) -> Result<Vec<IbexCompany>, DbError> {
        let companies = sqlx::query_as!(IbexCompanyBd, "SELECT * FROM ibex35_listing",)
            .fetch_all(self.pool)
            .await
            .map_err(|e| DbError::Unknown(e.to_string()))?;

        let companies = match companies.iter().map(IbexCompany::try_from).collect() {
            Ok(c) => c,
            Err(e) => return Err(DbError::Unknown(format!("{e}"))),
        };

        Ok(companies)
    }

    #[instrument(name = "Get short position by ticker and owner", skip(self))]
    async fn active_position(
        &self,
        ticker: &str,
        owner: &str,
    ) -> Result<Option<ShortPositionBd>, DbError> {
        let position = sqlx::query_as!(
            ShortPositionBd,
            r#"
            SELECT alive_positions.id, owner, weight, open_date, ticker
            FROM alive_positions INNER JOIN ibex35_short_historic on alive_positions.id = ibex35_short_historic.id
            WHERE ibex35_short_historic.ticker = $1 AND ibex35_short_historic.owner = $2
            "#,
            ticker,
            owner
        )
        .fetch_optional(self.pool)
        .await
        .map_err(|e| DbError::Unknown(e.to_string()))?;

        Ok(position)
    }

    #[instrument(name = "Get all short positions by ticker", skip(self))]
    async fn active_positions(&self, ticker: &str) -> Result<Vec<ShortPositionBd>, DbError> {
        let positions = sqlx::query_as!(
            ShortPositionBd,
            r#"
            SELECT alive_positions.id, owner, weight, open_date, ticker
            FROM alive_positions INNER JOIN ibex35_short_historic on alive_positions.id = ibex35_short_historic.id
            WHERE ibex35_short_historic.ticker = $1
            "#,
            ticker,
        )
        .fetch_all(self.pool)
        .await
        .map_err(|e| DbError::Unknown(e.to_string()))?;

        debug!(
            "Stored active short positions for {}: {:?}",
            ticker, positions
        );

        Ok(positions)
    }

    #[instrument(name = "Insert a new short", skip(self, position), fields(ticker=position.ticker))]
    async fn insert_short_position(
        &self,
        position: &ShortPosition,
        active: Option<Uuid>,
    ) -> Result<Uuid, DbError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|e| DbError::Unknown(e.to_string()))?;

        let uuid = uuid::Uuid::new_v4();

        if active.is_some() {
            transaction
                .execute(sqlx::query!(
                    "UPDATE alive_positions set id = $2 where id = $1",
                    active.unwrap().to_string(),
                    uuid,
                ))
                .await
                .map_err(|e| DbError::Unknown(e.to_string()))?;
        } else {
            transaction
                .execute(sqlx::query!(
                    "INSERT INTO alive_positions VALUES ($1)",
                    uuid,
                ))
                .await
                .map_err(|e| DbError::Unknown(e.to_string()))?;
        }

        transaction
            .execute(sqlx::query!(
                r#"INSERT INTO ibex35_short_historic (id, owner, weight, open_date, ticker)
                VALUES ($1, $2, $3, $4, $5)"#,
                uuid,
                position.owner.as_str(),
                position.weight,
                position.open_date.naive_utc(),
                position.ticker,
            ))
            .await
            .map_err(|e| DbError::Unknown(e.to_string()))?;

        transaction
            .commit()
            .await
            .map_err(|e| DbError::Unknown(e.to_string()))?;

        info!("New position registered in the record ({uuid})");

        Ok(uuid)
    }

    #[instrument(name = "Wipe an active short", skip(self))]
    async fn wipe_short_position(&self, id: &Uuid) -> Result<(), DbError> {
        // QuestDB does not implement a DELETE logic for tables not partitioned by DATEs, so all we can
        // do is to write some dummy value.
        sqlx::query!(
            r#"UPDATE alive_positions SET id = '00000000-0000-0000-0000-000000000000' where id = $1"#,
            id.to_string(),
        )
        .execute(self.pool)
        .await
        .map_err(|e| DbError::Unknown(e.to_string()))?;

        info!("Active position wiped");

        Ok(())
    }
}

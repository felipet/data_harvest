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
use tracing::{debug, error, info, warn, instrument};

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
    pub async fn add_today_data(&self) -> Result<(), Box<dyn Error>> {
        // Let's get an updated listing of the Ibex35's companies.
        let companies = self.stock_listing().await?;
        debug!("{} companies listed from the IBEX35", companies.len());

        // For each company, request to the CNMV's site if there's any open short position.
        for company in companies.iter().filter(|x| x.extra_id().is_some()) {
            let new_positions = self
                .scrapper
                .short_positions(company)
                .await
                .map_err(Box::new)?;

            // If we got some short position
            if !new_positions.positions.is_empty() {
                // First, let's get a list of the active short positions for the company which are already registered
                // in the DB.
                let stored_position = self.active_positions(company.ticker()).await?;
                debug!("Stored positions for {}: {:?}", company.ticker(), stored_position);

                // Check whether any new position was already present in the DB.
                for new_position in new_positions.positions {
                    // The first time a short position is notified, the DB will be empty.
                    if !stored_position.is_empty() {
                        // Search the new position in the stored register.
                        let mut found = false;

                        for item in stored_position.iter() {
                            let z = ShortPosition::try_from(item)?;

                            if z == new_position {
                                info!(
                                    "The position owned by {} against {} was already in the record",
                                    new_position.owner, new_position.ticker
                                );
                                found = true;
                                break;
                            }
                        }

                        // If we haven't found the new position in the active position list, store it.
                        if !found {
                            debug!(
                                "The position owned by {} against {} was not in the record",
                                new_position.owner, new_position.ticker
                            );
                            let previous_active_position = match self
                                .active_position(&new_position.ticker, &new_position.owner)
                                .await?
                            {
                                Some(p) => p.id,
                                None => None,
                            };

                            self.insert_short_position(&new_position, previous_active_position)
                                .await?;
                        }
                    } else {
                        // Store the new position.
                        info!("No previous short position was stored for {}, recording the new one ({})", new_position.ticker, new_position.owner);
                        self.insert_short_position(&new_position, None).await?;
                    }
                }
            } else {
                // The company has no open positions at the moment. Check if there's any active position in the
                // DB to wipe it.
                info!(
                    "The company {} has no open short positions",
                    company.ticker()
                );

                let stored_active_positions = self.active_positions(company.ticker()).await?;

                if !stored_active_positions.is_empty() {
                    warn!(
                        "The company {} got free of significant short positions",
                        company.ticker()
                    );
                    for position in stored_active_positions.iter() {
                        match position.id {
                            Some(id) => self.wipe_short_position(&id).await?,
                            None => error!("Corrupt data in the DB: {:?}", position),
                        }
                    }
                }
            }
        }

        Ok(())
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

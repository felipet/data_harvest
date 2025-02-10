// Copyright 2025 Felipe Torres González
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use data_harvest::web_scrappers::CnmvProvider;
use finance_ibex::IbexCompany;

#[tokio::main]
async fn main() {
    let grifols = Box::new(IbexCompany::new(
        Some("Grifols Clase A"),
        "GRIFOLS",
        "GRF",
        "ES0171996087",
        Some("A-58389123"),
    ));

    let provider = CnmvProvider::new();
    let short_positions = provider.short_positions(&grifols).await;

    match short_positions {
        Ok(data) => println!("Short positions for Grifols: \n{:#?}", data),
        Err(e) => println!("Errosrs found: {:#?}", e),
    }
}

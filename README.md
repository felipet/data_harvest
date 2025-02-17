# Rust Finance Data Harvest Library

[![License](https://img.shields.io/github/license/felipet/data_harvest?style=flat-square)](https://github.com/felipet/data_harvest/blob/main/LICENSE)
![GitHub Actions Workflow Status](https://img.shields.io/github/actions/workflow/status/felipet/data_harvest/rust.yml?style=flat-square&label=CI%20status)

This library includes several modules that extract finance and stock related data from web sites. The main purpose
of the library is to retrieve useful data for financial quantitative analysis, and organise it into a data base.

## Supported Sources of Data

At the moment the following sources of data are supported:

### [Comisión Nacional de Valores](https://www.cnmv.es)
The CNMV is the Spanish stock exchange supervisor and regulator. Its web site includes many useful registers,
though the access is complicated due to the organisation of the web site, and certainly not friendly for
machines.
The library supports extracting data about [short positions](https://www.cnmv.es/portal/consultas/busqueda?id=29)
against companies listed in the IBEX35.

#### Short Data Provider

This `struct` [web_scrappers::CnmvProvider] includes logic to extract the active short positions against a
given company of the IBEX35. The module works together with the API defined in the
[Finance Lib](https://crates.io/crates/finance_api) crate, and uses the implementation of such API
[Finance Ibex](https://crates.io/crates/finance_ibex). So have a look at them before using this module.

## API
There is no defined API of this library because the main goal of the library is to keep a private data base with
all the harvested data. Hence if you just need to scrap data from a supported web site, look for any of the
modules within [web_scrappers].

## Data Base Management
The modules within [feeders] are meant to call modules that produce data and push the new data to the private
data base.


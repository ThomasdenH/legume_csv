//! Create a beancount ledger from an csv file.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(bare_trait_objects)]
#![deny(elided_lifetimes_in_paths)]
#![deny(missing_debug_implementations)]

use beancount_core::{Account, Amount, Flag, IncompleteAmount, Posting, Transaction};
use beancount_render::{BasicRenderer, Renderer};
use chrono::NaiveDate;
use handlebars::Handlebars;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io;
use std::io::BufWriter;
use std::path::PathBuf;
use structopt::StructOpt;
use thiserror::*;

/// An error that can occur when processing a transaction.
#[derive(Debug, Error)]
enum TransactionError {
    #[error("invalid account")]
    InvalidAccount,
    #[error("could not render template")]
    HandleBarError(#[from] handlebars::TemplateRenderError),
    #[error("invalid amount")]
    InvalidAmount,
    #[error("could not parse date")]
    DateParseError(#[from] chrono::format::ParseError),
}

/// Any error that can occur in the application.
#[derive(Debug, Error)]
enum Error {
    #[error("an error occurred processing a transaction")]
    Transaction(#[from] TransactionError),
    #[error("an io error occurred")]
    Io(#[from] std::io::Error),
    #[error("could not parse the csv")]
    Csv(#[from] csv::Error),
    #[error("could not parse the yaml")]
    Yaml(#[from] serde_yaml::Error),
}

/// A tool to convert csv to beancount files.
///
/// To work, it requires an input file (`--ledger`, `-l`) and a configuration (`--config`, `-c`).
/// Optionally, you can use `--append` to specify a file to which the new entries will be appended.
#[derive(Debug, StructOpt)]
#[structopt(
    name = "csv_to_beancount",
    about = "convert transactions in CSV to beancount format"
)]
struct Opt {
    /// The ledger in csv format to convert to Beancount.
    #[structopt(long = "ledger", short = "l")]
    csv_path: PathBuf,
    /// The configuration to use which specifies how to interpret the csv file.
    #[structopt(long = "config", short = "c")]
    yaml_path: PathBuf,
    /// If supplied, the new entries will be appended to this file.
    #[structopt(long = "append")]
    append_path: Option<PathBuf>,
}

/// The configuration used to convert the ledger entries.
#[derive(Debug, Deserialize)]
struct Configuration {
    /// The keyed inputs from the csv.
    input: HashMap<String, usize>,
    /// The settings for the Yaml.
    settings: Settings,
    output: TransactionTemplate,
}

const fn default_delimiter() -> char {
    ','
}

const fn default_quote() -> char {
    '\''
}

/// Settings for the yaml file.
#[derive(Debug, Deserialize)]
struct Settings {
    #[serde(default = "default_delimiter")]
    delimiter: char,
    #[serde(default = "default_quote")]
    quote: char,
    #[serde(default)]
    skip: usize,
    date_format: String,
}

#[derive(Debug, Deserialize)]
struct YamlPosting {
    flag: Option<String>,
    account: String,
    amount: Option<String>,
    cost: Option<String>,
    price: Option<String>,
}

fn default_transaction_flag() -> String {
    "!".into()
}

#[derive(Debug, Deserialize)]
struct TransactionTemplate {
    date: String,
    #[serde(default = "default_transaction_flag")]
    flag: String,
    payee: Option<String>,
    narration: String,
    postings: Vec<YamlPosting>,
}

/// Generate an `IncompleteAmount` from a string in the format "{{amount}} {{currency}}".
fn incomplete_amount_from_string(s: String) -> Result<IncompleteAmount<'static>, TransactionError> {
    let mut split = s.split(' ');
    let value = split
        .next()
        .ok_or(TransactionError::InvalidAmount)?
        .replace(',', ".")
        .parse::<Decimal>()
        .map_err(|_| TransactionError::InvalidAmount)?;
    let currency = split
        .next()
        .ok_or(TransactionError::InvalidAmount)?
        .to_string();
    Ok(Amount::builder()
        .num(value)
        .currency(Cow::from(currency))
        .build()
        .into())
}

fn account_from_string(s: String) -> Result<Account<'static>, TransactionError> {
    let mut parts = s.split(':');
    use beancount_core::account_types::AccountType::*;
    let account_type = match parts.next().ok_or(TransactionError::InvalidAccount)? {
        "Assets" => Assets,
        "Liabilities" => Liabilities,
        "Equity" => Equity,
        "Income" => Income,
        "Expenses" => Expenses,
        _ => return Err(TransactionError::InvalidAccount),
    };
    let parts: Vec<_> = parts.map(String::from).map(Cow::from).collect();
    Ok(Account::builder().ty(account_type).parts(parts).build())
}

fn build_posting<'a>(
    posting_template: &'a YamlPosting,
    handlebars: &Handlebars<'_>,
    data: &HashMap<&str, &str>,
) -> Result<Posting<'a>, TransactionError> {
    let account =
        account_from_string(handlebars.render_template(&posting_template.account, &data)?)?;
    let units = posting_template
        .amount
        .as_ref()
        .map(|cost| handlebars.render_template(&cost, &data))
        .transpose()?
        .map(incomplete_amount_from_string)
        .transpose()?
        .unwrap_or_else(|| IncompleteAmount::builder().build());
    let flag = posting_template
        .flag
        .as_ref()
        .map(|flag| handlebars.render_template(&flag, &data))
        .transpose()?
        .map(Flag::from);

    let price = posting_template
        .price
        .as_ref()
        .map(|price| handlebars.render_template(&price, &data))
        .transpose()?
        .map(incomplete_amount_from_string)
        .transpose()?;

    Ok(Posting::builder()
        .account(account)
        .flag(flag)
        .units(units)
        .price(price)
        .build())
}

fn build_transaction<'a>(
    record: csv::StringRecord,
    config: &'a Configuration,
    handlebars: &Handlebars<'_>,
) -> Result<Transaction<'a>, TransactionError> {
    let data: HashMap<&str, &str> = config
        .input
        .iter()
        .map(|(key, value)| -> (&str, &str) { (key, &record[*value]) })
        .collect();

    let date = NaiveDate::parse_from_str(
        &handlebars.render_template(&config.output.date, &data)?,
        &config.settings.date_format,
    )?;

    let payee = config
        .output
        .payee
        .as_ref()
        .map(|payee_template| handlebars.render_template(&payee_template, &data))
        .transpose()?
        .filter(|payee| !payee.is_empty())
        .map(Cow::from);

    let flag = Flag::from(handlebars.render_template(&config.output.flag, &data)?);

    let narration = handlebars.render_template(&config.output.narration, &data)?;

    let postings: Vec<Posting<'_>> = config
        .output
        .postings
        .iter()
        .map(|posting_template: &YamlPosting| build_posting(posting_template, handlebars, &data))
        .collect::<Result<Vec<Posting<'_>>, TransactionError>>()?;

    Ok(Transaction::builder()
        .date(date.into())
        .flag(flag)
        .payee(payee)
        .narration(narration.into())
        .postings(postings)
        .build())
}

fn main() -> Result<(), anyhow::Error> {
    let opt = Opt::from_args();

    let config: Configuration = {
        let yaml_file = std::fs::File::open(&opt.yaml_path)?;
        serde_yaml::from_reader(yaml_file)?
    };

    let csv_file = std::fs::File::open(opt.csv_path)?;

    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(config.settings.delimiter as u8)
        .quote(config.settings.quote as u8)
        .has_headers(false)
        .from_reader(csv_file);

    let mut handlebars = Handlebars::new();
    handlebars.register_escape_fn(handlebars::no_escape);

    let mut write: Box<dyn io::Write> = if let Some(append_path) = opt.append_path {
        let file = OpenOptions::new().append(true).open(append_path)?;
        Box::new(BufWriter::new(file))
    } else {
        Box::new(io::stdout())
    };

    let renderer = BasicRenderer::default();
    for record in rdr.records().skip(config.settings.skip) {
        let transaction = build_transaction(record?, &config, &handlebars)?;
        renderer.render(&transaction, &mut write)?;
    }
    Ok(())
}

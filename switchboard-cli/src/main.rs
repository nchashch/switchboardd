mod amount;
use amount::AmountBtc;
use anyhow::Result;
use clap::{Parser, Subcommand};
use futures::executor::block_on;
use hex::ToHex;
use std::path::PathBuf;
use switchboard::{config::Config, format_deposit_address};
use ureq_jsonrpc::json;
use web3::{types::U256, Transport};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long)]
    datadir: Option<PathBuf>,
    #[command(subcommand)]
    commands: Commands,
}

fn btc_amount_parser(s: &str) -> Result<bitcoin::Amount, bitcoin::util::amount::ParseAmountError> {
    bitcoin::Amount::from_str_in(s, bitcoin::Denomination::Bitcoin)
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Generate a mainchain block
    Generate {
        number: usize,
        #[arg(value_parser = btc_amount_parser)]
        amount: Option<bitcoin::Amount>,
    },
    /// Call zcash RPC directly
    Zcash {
        method: String,
        params: Option<Vec<String>>,
    },
    /// Call mainchain RPC directly
    Main {
        method: String,
        params: Option<Vec<String>>,
    },
    GethConsole,
    /// Get balances for mainchain and all sidechains
    Getbalances,
    /// Get block counts for mainchain and all sidechains
    Getblockcounts,
    /// Create a deposit to a sidechain
    Deposit {
        /// Sidechain to deposit to
        sidechain: Sidechain,
        /// Amount of BTC to deposit
        #[arg(value_parser = btc_amount_parser)]
        amount: bitcoin::Amount,
        /// Deposit fee in BTC
        #[arg(value_parser = btc_amount_parser)]
        fee: Option<bitcoin::Amount>,
    },
    /// Withdraw funds from a sidechain
    Withdraw {
        /// Sidechain to withdraw from
        sidechain: Sidechain,
        /// Amount of BTC to withdraw
        #[arg(value_parser = btc_amount_parser)]
        amount: bitcoin::Amount,
        /// Withdrawal fee in BTC, determines withdrawal's priority in the bundle
        #[arg(value_parser = btc_amount_parser)]
        fee: Option<bitcoin::Amount>,
    },
    /// Refund funds pending withdrawal back to a sidechain
    Refund {
        /// Sidechain to refund to
        sidechain: Sidechain,
        /// Amount of BTC to refund
        #[arg(value_parser = btc_amount_parser)]
        amount: bitcoin::Amount,
        /// Withdrawal fee in BTC, determines change withdrawal's priority in the bundle
        #[arg(value_parser = btc_amount_parser)]
        fee: Option<bitcoin::Amount>,
    },
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let home_dir = dirs::home_dir().unwrap();
    let datadir = args
        .datadir
        .unwrap_or_else(|| home_dir.join(".switchboard"));
    let config: Config = confy::load_path(datadir.join("config.toml"))?;

    let main = ureq_jsonrpc::Client {
        host: "localhost".to_string(),
        port: config.main.port,
        user: config.switchboard.rpcuser.clone(),
        password: config.switchboard.rpcpassword.clone(),
        id: "switchboard-cli".to_string(),
    };

    let zcash = ureq_jsonrpc::Client {
        host: "localhost".to_string(),
        port: config.zcash.port,
        user: config.switchboard.rpcuser.clone(),
        password: config.switchboard.rpcpassword.clone(),
        id: "switchboard-cli".to_string(),
    };

    let eth_transport =
        web3::transports::Http::new(&format!("http://localhost:{}", config.ethereum.port))?;
    let web3 = web3::Web3::new(eth_transport.clone());

    match args.commands {
        Commands::Generate { number, amount } => {
            let amount = amount.unwrap_or(bitcoin::Amount::from_btc(0.0001)?);
            let hashes = zcash.send_request::<Vec<bitcoin::BlockHash>>(
                "generate",
                &[json!(number), json!(AmountBtc(amount))],
            )?;
            for hash in &hashes {
                println!("{}", hash);
            }
        }
        Commands::Zcash { method, params } => {
            let params: Vec<ureq_jsonrpc::Value> =
                params.iter().map(|param| json!(param)).collect();
            let result = zcash.send_request::<ureq_jsonrpc::Value>(&method, &params)?;
            match result.as_str() {
                Some(result) => println!("{}", result),
                None => println!("{}", result),
            };
        }
        Commands::Main { method, params } => {
            let params: Vec<ureq_jsonrpc::Value> =
                params.iter().map(|param| json!(param)).collect();
            let result = main.send_request::<ureq_jsonrpc::Value>(&method, &params)?;
            match result.as_str() {
                Some(result) => println!("{}", result),
                None => println!("{}", result),
            };
        }
        Commands::GethConsole => {
            let ipc_file = datadir.join("data/ethereum/geth.ipc");
            std::process::Command::new(datadir.join("bin/geth"))
                .arg("attach")
                .arg(format!("{}", ipc_file.display()))
                .spawn()?
                .wait()?;
        }
        Commands::Getbalances => {
            let main = *main.send_request::<AmountBtc>("getbalance", &[])?;
            let zcash = *zcash.send_request::<AmountBtc>("getbalance", &[])?;
            let ethereum = {
                pub const SATOSHI: u64 = 10_000_000_000;
                let accounts = block_on(web3.eth().accounts())?;
                let mut balance = U256::zero();
                for account in accounts.iter() {
                    balance += block_on(web3.eth().balance(*account, None))?;
                }
                let sat = (balance / SATOSHI).as_u64();
                bitcoin::Amount::from_sat(sat)
            };
            println!("main:     {:>24}", format!("{}", main));
            println!("zcash:    {:>24}", format!("{}", zcash));
            println!("ethereum: {:>24}", format!("{}", ethereum));
        }
        Commands::Getblockcounts => {
            let main = main.send_request::<usize>("getblockcount", &[])?;
            let zcash = zcash.send_request::<usize>("getblockcount", &[])?;
            let ethereum = block_on(web3.eth().block_number())?.as_usize();
            println!("main:     {:>24}", format!("{}", main));
            println!("zcash:    {:>24}", format!("{}", zcash));
            println!("ethereum: {:>24}", format!("{}", ethereum));
        }
        Commands::Deposit {
            sidechain,
            amount,
            fee,
        } => {
            let fee = fee.unwrap_or(bitcoin::Amount::from_btc(0.0001)?);
            let address = match sidechain {
                Sidechain::Zcash => zcash.send_request::<String>("getnewaddress", &[])?,
                Sidechain::Ethereum => {
                    let accounts = block_on(web3.eth().accounts())?;
                    let account = accounts
                        .first()
                        .ok_or(anyhow::Error::msg("No available Ethereum addresses"))?;
                    format!("0x{}", account.encode_hex::<String>())
                }
            };
            let address = format_deposit_address(sidechain.number(), address);
            let txid = main.send_request::<bitcoin::Txid>(
                "createsidechaindeposit",
                &[
                    json!(sidechain.number()),
                    json!(address),
                    json!(AmountBtc(amount)),
                    json!(AmountBtc(fee)),
                ],
            )?;
            println!(
                "created deposit of {} to {} with fee {} and txid = {}",
                amount, sidechain, fee, txid
            );
        }
        Commands::Withdraw {
            sidechain,
            amount,
            fee,
        } => {
            let fee = fee.unwrap_or(bitcoin::Amount::from_btc(0.0001)?);
            match sidechain {
                Sidechain::Zcash => {
                    zcash.send_request::<String>(
                        "withdraw",
                        &[json!(AmountBtc(amount)), json!(AmountBtc(fee))],
                    )?;
                }
                Sidechain::Ethereum => {
                    let accounts = block_on(web3.eth().accounts())?;
                    let account = accounts
                        .first()
                        .ok_or(anyhow::Error::msg("No available Ethereum addresses"))?;
                    let account = format!("0x{}", account.encode_hex::<String>());
                    let amount: U256 = (amount.to_sat()).into();
                    let fee: U256 = (fee.to_sat()).into();
                    block_on(eth_transport.execute(
                        "eth_withdraw",
                        vec![json!(account), json!(amount), json!(fee)],
                    ))?;
                }
            };
            println!(
                "created withdrawal of {} from {} with fee {}",
                amount, sidechain, fee
            );
        }
        Commands::Refund {
            sidechain,
            amount,
            fee,
        } => {
            let fee = fee.unwrap_or(bitcoin::Amount::from_btc(0.0001)?);
            match sidechain {
                Sidechain::Zcash => zcash
                    .send_request("refund", &[json!(AmountBtc(amount)), json!(AmountBtc(fee))])?,
                Sidechain::Ethereum => {
                    println!("ATTENTION: Automatic refunds are not supported for ethereum, use geth-console to make a refund");
                    return Ok(());
                }
            }
            println!(
                "refunded {} to {} with change withdrawal fee {}",
                amount, sidechain, fee
            );
        }
    }
    Ok(())
}

use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, clap::ValueEnum, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Chain {
    Main,
    Zcash,
    Ethereum,
}

impl std::fmt::Display for Chain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Chain::Main => write!(f, "main"),
            Chain::Zcash => write!(f, "zcash"),
            Chain::Ethereum => write!(f, "ethereum"),
        }
    }
}

#[derive(Copy, Clone, Debug, clap::ValueEnum, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sidechain {
    Zcash,
    Ethereum,
}

impl std::fmt::Display for Sidechain {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.chain().fmt(f)
    }
}

impl Sidechain {
    pub fn chain(&self) -> Chain {
        match self {
            Sidechain::Zcash => Chain::Zcash,
            Sidechain::Ethereum => Chain::Ethereum,
        }
    }

    pub fn number(&self) -> usize {
        match self {
            Sidechain::Zcash => 0,
            Sidechain::Ethereum => 1,
        }
    }
}

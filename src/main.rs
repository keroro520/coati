use ckb_types::packed::{Block, CellOutput, OutPoint};
use ckb_types::{core::ScriptHashType, h256, packed::Script, prelude::{Entity, Pack, Reader, Unpack}};
use gw_config::{ChainConfig, Config as GwConfig, GenesisConfig, RPCClientConfig};
use gw_rpc_client::ckb_client::CKBClient;
use gw_rpc_client::indexer_client::CKBIndexerClient;
use gw_rpc_client::rpc_client::RPCClient;
use gw_types::offchain::RollupContext;
use gw_types::packed::{WithdrawalLockArgs, WithdrawalLockArgsReader};
use std::collections::HashMap;
use std::env::var;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};
use anyhow::Context;
use tokio::sync::mpsc::channel;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let gw_config = GwConfig {
        rpc_client: RPCClientConfig {
            // indexer_url: "http://127.0.0.1:8114".to_string(),
            indexer_url: "https://testnet.ckb.dev/indexer".to_string(),
            ckb_url: "http://127.0.0.1:8114".to_string(),
            // ckb_url: "https://testnet.ckb.dev".to_string(),
        },
        genesis: GenesisConfig{
            rollup_type_hash: h256!("0x4940246f168f4106429dc641add3381a44b5eef61e7754142f594e986671a575"),
            rollup_config: Default::default(),
            ..Default::default()
        },
        chain: ChainConfig {
            rollup_type_script: gw_jsonrpc_types::blockchain::Script {
                code_hash: h256!("0x1e44736436b406f8e48a30dfbddcf044feb0c9eebfe63b0f81cb5bb727d84854"),
                hash_type: gw_jsonrpc_types::blockchain::ScriptHashType::Type,
                args: gw_jsonrpc_types::ckb_jsonrpc_types::JsonBytes::from_bytes(
                    "0x86c7429247beba7ddd6e4361bcdfc0510b0b644131e2afb7e486375249a01802".pack().as_bytes()
                )
            },
            ..Default::default()
        },
        ..Default::default()
    };

    let rollup_type_script = Script::new_unchecked(
        Into::<gw_types::packed::Script>::into(gw_config.chain.rollup_type_script.clone())
            .as_bytes(),
    );
    let rollup_script_hash = rollup_type_script.calc_script_hash().unpack();
    let rpc = RPCClient::new(
        rollup_type_script,
        RollupContext {
            rollup_config: gw_config.genesis.rollup_config.clone().into(),
            rollup_script_hash: rollup_script_hash.0.into(),
        },
        CKBClient::with_url(&gw_config.rpc_client.ckb_url)?,
        CKBIndexerClient::with_url(&gw_config.rpc_client.indexer_url)?,
    );


    let tip_number = gw_types::prelude::Unpack::unpack(
        &rpc.get_tip().await?.number()
    );
    let mut block_number : u64 = var("START_BLOCK_NUMBER")
        .map(|raw| raw.parse().unwrap())
        .unwrap_or(0);
    let (sender, mut receiver) = channel(1000);
    tokio::spawn(async move {
        let mut last_start_time = Instant::now();
        let mut live_withdrawal_cells = HashMap::new();
        while let Some(block) =  receiver.recv().await {
            handle_block(
                &gw_config,
                &mut live_withdrawal_cells,
                &block,
            ).unwrap();
            let block_number : u64 = block.header().raw().number().unpack();
            if block_number % 10000 == 9999 {
                println!("Processing {}/{} last elapsed: {:?}", block_number, tip_number, last_start_time.elapsed());
                last_start_time = Instant::now();
            }
        }
    });


    loop {
        match rpc.get_block_by_number(block_number).await? {
            None => {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Some(block) => {
                sender.send(Block::new_unchecked(block.as_bytes())).await.unwrap();
                block_number += 1;
            }
        }
    }
}

fn handle_block(
    gw_config: &GwConfig,
    live_withdrawal_cells: &mut HashMap<OutPoint, CellOutput>,
    block: &Block,
) -> Result<(), anyhow::Error> {
    let current_block_number: u64 = block.header().raw().number().unpack();
    let current_timestamp: u64 = block.header().raw().timestamp().unpack();
    for tx in block.transactions() {
        for (index, output) in tx.raw().outputs().into_iter().enumerate() {
            if is_withdrawal_cell(&gw_config, &output) {
                let out_point = OutPoint::new(tx.calc_tx_hash(), index as u32);
                live_withdrawal_cells.insert(out_point, output);
            }
        }

        for input in tx.raw().inputs() {
            let out_point = input.previous_output();

            // Consuming this withdrawal cell
            if let Some(withdrawal_cell) = live_withdrawal_cells.remove(&out_point) {
                let raw_withdrawal_lock_args = withdrawal_cell
                    .lock()
                    .args()
                    .as_bytes()
                    .slice(32..WithdrawalLockArgs::TOTAL_SIZE);
                WithdrawalLockArgsReader::verify(&raw_withdrawal_lock_args, false)?;
                let withdrawal_lock_args =
                    WithdrawalLockArgs::new_unchecked(raw_withdrawal_lock_args);
                let withdrawal_timepoint: u64 = gw_types::prelude::Unpack::unpack(
                    &withdrawal_lock_args.withdrawal_block_number(),
                );

                const MASK: u64 = 1 << 63;
                let is_block_number = (MASK & withdrawal_timepoint) == 0;
                if is_block_number {
                    println!(
                        "unlock withdrawal by block number, withdrawal_gap: {}, out_point: {} {:?}, tx_hash: {:x}",
                        current_block_number.saturating_sub(withdrawal_timepoint),
                        out_point,
                        out_point,
                        tx.calc_tx_hash()
                    );
                    // assert_eq!(
                    //     current_block_number . saturating_sub( withdrawal_timepoint)
                    // );
                } else {
                    let withdrawal_elapsed = current_timestamp - (withdrawal_timepoint ^ MASK);
                    println!(
                        "unlock withdrawal by timestamp, withdrawal_elapsed: {}, out_point: {} {:?}, tx_hash: {:x}",
                        withdrawal_elapsed,
                        out_point,
                        out_point,
                        tx.calc_tx_hash()
                    );
                };
            }
        }
    }

    Ok(())
}

fn is_withdrawal_cell(gw_config: &GwConfig, output: &CellOutput) -> bool {
    output.lock().hash_type() == ScriptHashType::Type.into()
        && output.lock().code_hash()
            == gw_config
                .genesis
                .rollup_config
                .withdrawal_script_type_hash
                .pack()
        && output
            .lock()
            .args()
            .as_bytes()
            .starts_with(gw_config.chain.rollup_type_script.hash().as_bytes())
}

#[allow(unused)]
fn read_config<P: AsRef<Path>>(path: P) -> Result<GwConfig, anyhow::Error> {
    let content = fs::read(&path)
        .with_context(|| format!("read config file from {}", path.as_ref().to_string_lossy()))?;
    let config = toml::from_slice(&content).with_context(|| "parse config file")?;
    Ok(config)
}
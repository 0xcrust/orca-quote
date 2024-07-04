use crate::api;
use crate::utils::deserialize_anchor_account;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ops::{Add, Div, Mul};
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use log::{info, warn};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;

use whirlpool::manager::swap_manager::swap;
use whirlpool::math::tick_math::{MAX_SQRT_PRICE_X64, MIN_SQRT_PRICE_X64};
use whirlpool::state::{
    tick::{MAX_TICK_INDEX, MIN_TICK_INDEX, TICK_ARRAY_SIZE},
    TickArray, Whirlpool,
};
use whirlpool::util::SwapTickSequence;

#[derive(Clone, Debug)]
pub struct WhirlpoolArbState {
    pub override_cache: bool,
    pub http_url: String,
    pub amount: u64,
    pub in_token: Pubkey,
    pub out_token: Pubkey,
    pub whirlpool_program: Pubkey,
    pub slippage: f64,
}

fn get_environment_variables() -> Result<WhirlpoolArbState> {
    let override_cache = std::env::var("OVERRIDE_CACHE")?.parse::<bool>()?;
    let http_url = std::env::var("HTTP_URL")?;
    let amount = std::env::var("AMOUNT")?.parse::<u64>()?;
    let in_token = Pubkey::from_str(&std::env::var("INPUT_TOKEN")?)?;
    let out_token = Pubkey::from_str(&std::env::var("OUTPUT_TOKEN")?)?;
    let whirlpool_program = Pubkey::from_str(&std::env::var("WHIRLPOOL_PROGRAM_ID")?)?;
    let slippage = std::env::var("SLIPPAGE")?.parse::<f64>()?;
    Ok(WhirlpoolArbState {
        override_cache,
        http_url,
        amount,
        in_token,
        out_token,
        whirlpool_program,
        slippage,
    })
}

/// Returns `(quote, slippage_adjusted_quote)`
pub async fn get_quote() -> anyhow::Result<(u64, u64)> {
    let arb_state: WhirlpoolArbState = get_environment_variables().unwrap();
    let pools = api::get_whirlpools(arb_state.override_cache).await?;

    let in_token = arb_state.in_token;
    let out_token = arb_state.out_token;
    let amount = arb_state.amount;

    info!("Initiating swap. Input={}. Output={}", in_token, out_token);

    let pool_info = pools
        .whirlpools
        .into_iter()
        .find(|pool| {
            pool.token_a.mint == in_token && pool.token_b.mint == out_token
                || pool.token_a.mint == out_token && pool.token_b.mint == in_token
        })
        .expect("Failed to get pool information for pair");
    info!(
        "Found pool for swap. Mint0={}. Mint1={}. Tick-spacing={}",
        pool_info.token_a.mint, pool_info.token_b.mint, pool_info.tick_spacing
    );
    let a_to_b = pool_info.token_a.mint == in_token && pool_info.token_b.mint == out_token;
    let amount_specified_is_input = true;

    let client = Arc::new(RpcClient::new(arb_state.http_url));
    let whirlpool_account = client.get_account(&pool_info.address).await?;
    let whirlpool = deserialize_anchor_account::<Whirlpool>(&whirlpool_account)?;
    let tick_arrays = get_tick_arrays(
        &client,
        whirlpool.tick_current_index,
        whirlpool.tick_spacing as i32,
        a_to_b,
        &arb_state.whirlpool_program,
        &pool_info.address,
    )
    .await?;
    let mut tick_arrays = tick_arrays
        .into_iter()
        .map(|a| Rc::new(RefCell::new(a)))
        .collect::<VecDeque<_>>();

    let tick_array_0 = tick_arrays.pop_front().unwrap();
    let tick_array_1 = tick_arrays.pop_front().unwrap();
    let tick_array_2 = tick_arrays.pop_front().unwrap();

    let mut swap_tick_sequence = SwapTickSequence::new(
        tick_array_0.try_borrow_mut().ok().expect("not borrowed"),
        tick_array_1.try_borrow_mut().ok(),
        tick_array_2.try_borrow_mut().ok(),
    );

    let sqrt_price_limit = get_default_sqrt_price_limit(a_to_b);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let swap_result = swap(
        &whirlpool,
        &mut swap_tick_sequence,
        amount,
        sqrt_price_limit,
        amount_specified_is_input,
        a_to_b,
        timestamp,
    )?;

    info!("Swap update: {:#?}", swap_result);
    let quote = if a_to_b {
        swap_result.amount_b
    } else {
        swap_result.amount_a
    };
    let (amount_in, amount_out) = if a_to_b == amount_specified_is_input {
        (swap_result.amount_a, swap_result.amount_b)
    } else {
        (swap_result.amount_b, swap_result.amount_a)
    };

    let slippage_adjusted_quote = calculate_swap_amounts_from_quote(
        amount_in,
        amount_out,
        arb_state.slippage,
        amount_specified_is_input,
    );
    Ok((quote, slippage_adjusted_quote))
}

/// The maximum number of tick-arrays that can traversed across in a swap
const MAX_SWAP_TICK_ARRAYS: u16 = 3;
const PDA_TICK_ARRAY_SEED: &str = "tick_array";

fn get_default_sqrt_price_limit(a_to_b: bool) -> u128 {
    if a_to_b {
        MIN_SQRT_PRICE_X64
    } else {
        MAX_SQRT_PRICE_X64
    }
}

async fn get_tick_arrays(
    client: &RpcClient,
    tick_current_index: i32,
    tick_spacing: i32,
    a_to_b: bool,
    program_id: &Pubkey,
    whirlpool_address: &Pubkey,
) -> Result<Vec<TickArray>> {
    let keys = get_tick_array_keys(
        tick_current_index,
        tick_spacing,
        a_to_b,
        program_id,
        whirlpool_address,
    );
    info!("Tick array keys: {:?}", keys);
    let accounts = client.get_multiple_accounts(&keys).await?;
    let mut tick_arrays = Vec::with_capacity(accounts.len());
    for account in accounts {
        let tick_array =
            deserialize_anchor_account::<TickArray>(account.as_ref().expect("No account data"))?;
        tick_arrays.push(tick_array);
    }

    Ok(tick_arrays)
}

fn get_tick_array_keys(
    tick_current_index: i32,
    tick_spacing: i32,
    a_to_b: bool,
    program_id: &Pubkey,
    whirlpool_address: &Pubkey,
) -> Vec<Pubkey> {
    let shift = if a_to_b { 0 } else { tick_spacing };

    let mut offset = 0;
    let mut addresses = Vec::with_capacity(MAX_SWAP_TICK_ARRAYS as usize);
    for i in 0..MAX_SWAP_TICK_ARRAYS {
        if let Ok(start_index) =
            get_start_tick_index(tick_current_index + shift, tick_spacing, offset)
        {
            let address = get_tick_array_address(program_id, whirlpool_address, start_index);
            addresses.push(address);
            if a_to_b {
                offset -= 1;
            } else {
                offset += 1;
            }
        } else {
            warn!("Failed to get start-tick-index. i={}", i);
            break;
        }
    }

    addresses
}

fn get_start_tick_index(tick_index: i32, tick_spacing: i32, offset: i32) -> Result<i32> {
    info!("Getting startTickIndex with tickIndex={tick_index}, tickSpacing={tick_spacing}, offset={offset}");

    let real_index =
        ((tick_index as f64 / tick_spacing as f64 / TICK_ARRAY_SIZE as f64).floor()) as i32;
    info!("Real index: {}", real_index);
    let start_tick_index = (real_index + offset) * tick_spacing * TICK_ARRAY_SIZE;

    let ticks_in_array = TICK_ARRAY_SIZE * tick_spacing;
    let min_tick_index = MIN_TICK_INDEX - ((MIN_TICK_INDEX % ticks_in_array) + ticks_in_array);
    info!("start_tick_index={}", start_tick_index);
    if start_tick_index <= min_tick_index {
        warn!(
            "start_tick_index is less than min_tick_index={}",
            min_tick_index
        );
        return Err(anyhow!(
            "startTickIndex is too small - - ${start_tick_index}"
        ));
    }
    if start_tick_index >= MAX_TICK_INDEX {
        warn!(
            "start_tick_index is greater than max_tick_index={}",
            MAX_TICK_INDEX
        );
        return Err(anyhow!(
            "startTickIndex is too large - - ${start_tick_index}"
        ));
    }

    Ok(start_tick_index)
}

fn get_tick_array_address(program_id: &Pubkey, whirlpool: &Pubkey, start_tick: i32) -> Pubkey {
    Pubkey::find_program_address(
        &[
            PDA_TICK_ARRAY_SEED.as_bytes().as_ref(),
            whirlpool.to_bytes().as_ref(),
            start_tick.to_string().as_bytes(),
        ],
        &program_id,
    )
    .0
}

/// Returns the other_amount_threshhold
fn calculate_swap_amounts_from_quote(
    est_amount_in: u64,
    est_amount_out: u64,
    slippage: f64,
    amount_specified_is_input: bool,
) -> u64 {
    if amount_specified_is_input {
        adjust_for_slippage(est_amount_out, slippage, false)
    } else {
        adjust_for_slippage(est_amount_in, slippage, false)
    }
}

// todo: slippage in form of percentage numerator and denominator. Currently we just
// specify a numerator and assume a denominator of 100
fn adjust_for_slippage(amount: u64, slippage: f64, adjust_up: bool) -> u64 {
    if adjust_up {
        ((amount as f64).mul(slippage.add(100.0)).div(100.0)) as u64
    } else {
        ((amount as f64).mul(100.0).div(slippage.add(100.0))) as u64
    }
}

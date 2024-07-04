use std::io::ErrorKind;
use std::path::Path;

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

const ORCA_API_ENDPOINT: &str = "https://api.mainnet.orca.so/v1/whirlpool/list";
const CACHE_DIR: &str = "artifacts";
const CACHE_PATH: &str = "artifacts/orca_pools.json";

pub async fn get_whirlpools(override_cache: bool) -> anyhow::Result<WhirlPoolList> {
    match std::fs::create_dir(CACHE_DIR) {
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e)?,
    }

    let cache_exists = Path::new(CACHE_PATH).try_exists()?;
    if cache_exists && !override_cache {
        serde_json::from_str(&std::fs::read_to_string(CACHE_PATH)?).map_err(Into::into)
    } else {
        fetch_pools_from_api().await
    }
}

async fn fetch_pools_from_api() -> anyhow::Result<WhirlPoolList> {
    let pools = reqwest::get(ORCA_API_ENDPOINT)
        .await?
        .json::<WhirlPoolList>()
        .await?;
    std::fs::write(CACHE_PATH, serde_json::to_string_pretty(&pools)?)?;
    Ok(pools)
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WhirlPoolList {
    pub whirlpools: Vec<WhirlPool>,
    #[serde(rename = "hasMore")]
    pub has_more: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WhirlPool {
    #[serde(with = "serde_pubkey")]
    pub address: Pubkey,
    #[serde(rename = "tokenA")]
    pub token_a: Token,
    #[serde(rename = "tokenB")]
    pub token_b: Token,
    pub whitelisted: bool,
    #[serde(rename = "tickSpacing")]
    pub tick_spacing: u64,
    pub price: f64,
    #[serde(rename = "lpFeeRate")]
    pub lp_fee_rate: f64,
    #[serde(rename = "protocolFeeRate")]
    pub protocol_fee_rate: f64,
    #[serde(rename = "whirlpoolsConfig", with = "serde_pubkey")]
    pub whirlpools_config: Pubkey,
    #[serde(rename = "modifiedTimeMs")]
    pub modified_time_ms: Option<u64>,
    pub tvl: Option<f64>,
    pub volume: Option<Volume>,
    #[serde(rename = "volumeDenominatedA")]
    pub volume_denominated_a: Option<Volume>,
    #[serde(rename = "volumeDenominatedB")]
    pub volume_denominated_b: Option<Volume>,
    #[serde(rename = "priceRange")]
    pub price_range: Option<PriceRange>,
    #[serde(rename = "feeApr")]
    pub fee_apr: Option<Volume>,
    #[serde(rename = "reward0Apr")]
    pub reward0_apr: Option<Volume>,
    #[serde(rename = "reward1Apr")]
    pub reward1_apr: Option<Volume>,
    #[serde(rename = "reward2Apr")]
    pub reward2_apr: Option<Volume>,
    #[serde(rename = "totalApr")]
    pub total_apr: Option<Volume>,
}

impl PartialEq for WhirlPool {
    fn eq(&self, other: &Self) -> bool {
        self.token_a.symbol == other.token_a.symbol && self.token_b.symbol == other.token_b.symbol
    }
}

impl Eq for WhirlPool {}

#[derive(Debug, Deserialize, Serialize)]
pub struct Token {
    #[serde(with = "serde_pubkey")]
    pub mint: Pubkey,
    pub symbol: String,
    pub name: String,
    pub decimals: u64,
    #[serde(rename = "logoURI")]
    pub logo_uri: Option<String>,
    #[serde(rename = "coingeckoId")]
    pub coingecko_id: Option<String>,
    pub whitelisted: bool,
    #[serde(rename = "poolToken")]
    pub pool_token: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Volume {
    pub day: f64,
    pub week: f64,
    pub month: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MinMax {
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PriceRange {
    pub day: MinMax,
    pub week: MinMax,
    pub month: MinMax,
}

pub mod serde_pubkey {
    use std::str::FromStr;

    use serde::de::{Deserializer, Visitor};
    use solana_sdk::pubkey::Pubkey;

    struct PubkeyVisitor;
    impl<'de> Visitor<'de> for PubkeyVisitor {
        type Value = Pubkey;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str(r#"a pubkey string"#)
        }
        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Pubkey::from_str(v).map_err(|_| E::custom("failed parsing pubkey from str"))
        }
    }

    pub fn serialize<S>(key: &Pubkey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&key.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Pubkey, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(PubkeyVisitor)
    }
}

mod api;
mod quote;
mod utils;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv()?;
    env_logger::init();

    let (quote, slippage_adjusted_quote) = quote::get_quote().await?;
    println!("Quote: {}", quote);
    println!("Slippage_adjusted_quote: {}", slippage_adjusted_quote);

    Ok(())
}

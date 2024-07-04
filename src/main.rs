mod api;
mod quote;
mod utils;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv()?;
    env_logger::init();

    let output = quote::get_quote().await?;
    println!("Output: {}", output);

    Ok(())
}

use sea_orm_migration::prelude::*;

#[async_std::main]
async fn main() {
    let _ = dotenvy::dotenv();
    cli::run_cli(migration::Migrator).await;
}

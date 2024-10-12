pub use sea_orm_migration::prelude::*;

mod m20240703_005741_create_tables;
mod m20240905_125904_refresh_matches_index;
mod m20240908_140157_create_stats_history;
mod m20240920_235707_create_work_items;
mod m20241001_210358_create_files_table;
mod m20241006_193744_create_acls_table;
mod m20241012_214559_populate_assets;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20240703_005741_create_tables::Migration),
            Box::new(m20240905_125904_refresh_matches_index::Migration),
            Box::new(m20240908_140157_create_stats_history::Migration),
            Box::new(m20240920_235707_create_work_items::Migration),
            Box::new(m20241001_210358_create_files_table::Migration),
            Box::new(m20241006_193744_create_acls_table::Migration),
            Box::new(m20241012_214559_populate_assets::Migration),
        ]
    }
}

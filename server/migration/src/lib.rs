pub use sea_orm_migration::prelude::*;

mod m20240703_005741_create_tables;
mod m20240905_125904_refresh_matches_index;
mod m20240908_140157_create_stats_history;
mod m20240920_235707_create_work_items;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20240703_005741_create_tables::Migration),
            Box::new(m20240905_125904_refresh_matches_index::Migration),
            Box::new(m20240908_140157_create_stats_history::Migration),
            Box::new(m20240920_235707_create_work_items::Migration),
        ]
    }
}

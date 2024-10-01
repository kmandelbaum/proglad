use derive_more::Display;
use proglad_db as db;
use sea_orm::strum::IntoEnumIterator;
use sea_orm::Iden;
use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, DbErr, EntityTrait, IntoActiveModel, QueryFilter,
};

#[derive(Clone)]
pub struct FileStore {}

#[derive(Clone, Copy)]
pub enum Requester {
    Unauthenticated,
    System,
    Account(i64),
}

#[derive(Debug, Display, Eq, PartialEq)]
pub enum Error {
    PermissionDenied,
    NotFound,
    FileMissingContent,
    CompressionError(String),
    EncodingError,
    InvalidArgument(String),
    DbErr(DbErr),
}

impl std::error::Error for Error {}

impl FileStore {
    pub async fn write<C: ConnectionTrait>(
        &self,
        db: &C,
        _requester: Requester,
        file: db::files::Model,
    ) -> Result<(), Error> {
        // TODO: ACL
        // TODO: figure out if we want to change last_update here.
        if file.owning_entity != db::files::OwningEntity::None && file.owning_id.is_none() {
            return Err(Error::InvalidArgument(format!(
                "Owning entity {:?} is specified file name={}, but not owning ID",
                file.owning_entity, file.name
            )));
        }
        let mut update = file.into_active_model();
        update.id = sea_orm::ActiveValue::NotSet;
        db::files::Entity::insert(update)
            .on_conflict(
                sea_orm::sea_query::OnConflict::columns([
                    db::files::Column::Name,
                    db::files::Column::OwningEntity,
                    db::files::Column::OwningId,
                ])
                .update_columns(
                    db::files::Column::iter()
                        .filter(|c| c.to_string() != db::files::Column::Id.to_string()),
                )
                .to_owned(),
            )
            .exec(db)
            .await
            .map_err(Error::DbErr)?;
        Ok(())
    }
    pub async fn read<C: ConnectionTrait>(
        &self,
        db: &C,
        _requester: Requester,
        owning_entity: db::files::OwningEntity,
        owning_id: Option<i64>,
        name: &str,
    ) -> Result<db::files::Model, Error> {
        // TODO: ACL
        let file = db::files::Entity::find()
            .filter(
                Condition::all()
                    .add(db::files::Column::Name.eq(name))
                    .add(db::files::Column::OwningEntity.eq(owning_entity))
                    .add(db::files::Column::OwningId.eq(owning_id)),
            )
            .one(db)
            .await
            .map_err(Error::DbErr)?
            .ok_or(Error::NotFound)?;
        if file.content.is_none() {
            return Err(Error::FileMissingContent);
        }
        Ok(file)
    }
    pub async fn delete<C: ConnectionTrait>(
        &self,
        db: &C,
        _requester: Requester,
        owning_entity: db::files::OwningEntity,
        owning_id: Option<i64>,
        name: &str,
    ) -> Result<(), Error> {
        db::files::Entity::delete_many()
            .filter(
                Condition::all()
                    .add(db::files::Column::Name.eq(name))
                    .add(db::files::Column::OwningEntity.eq(owning_entity))
                    .add(db::files::Column::OwningId.eq(owning_id)),
            )
            .exec(db)
            .await
            .map_err(Error::DbErr)?;
        Ok(())
    }

    pub fn compress(mut file: db::files::Model) -> Result<db::files::Model, Error> {
        if file.compression != db::files::Compression::Uncompressed {
            return Ok(file);
        }
        let Some(content) = file.content else {
            return Ok(file);
        };
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        use std::io::prelude::Write;
        encoder.write_all(&content).map_err(compression_error)?;
        file.content = Some(encoder.finish().map_err(compression_error)?);
        file.compression = db::files::Compression::Gzip;
        Ok(file)
    }
    pub fn decompress(mut file: db::files::Model) -> Result<db::files::Model, Error> {
        match file.compression {
            db::files::Compression::Uncompressed => Ok(file),
            db::files::Compression::Gzip => {
                let Some(content) = file.content else {
                    return Ok(file);
                };
                use std::io::prelude::Read;
                let mut gz = flate2::read::GzDecoder::new(content.as_slice());
                // TODO: protect against zip bombs.
                // Normally these APIs are not exposed to the user
                // but it wouldn't hurt to have some defensive code here.
                let mut result = Vec::new();
                gz.read_to_end(&mut result).map_err(compression_error)?;
                file.content = Some(result);
                file.compression = db::files::Compression::Uncompressed;
                Ok(file)
            }
        }
    }
}

fn compression_error<D: ToString>(e: D) -> Error {
    Error::CompressionError(e.to_string())
}

#[cfg(test)]
mod test {
    use super::*;
    use sea_orm_migration::MigratorTrait;
    #[tokio::test]
    async fn test_read_write_delete() {
        let db = sea_orm::Database::connect("sqlite::memory:")
            .await
            .expect("Failed to open in-memory sqlite DB.");
        migration::Migrator::up(&db, None)
            .await
            .expect("Applying initial DB migrations failed");
        let store = FileStore {};
        let requester = Requester::System;
        assert_eq!(
            store
                .read(
                    &db,
                    requester,
                    db::files::OwningEntity::Program,
                    Some(123),
                    "myfile-4"
                )
                .await,
            Err(Error::NotFound)
        );
        let file1 = db::files::Model {
            owning_entity: db::files::OwningEntity::Game,
            owning_id: Some(123),
            content: Some(b"abradabra-1".to_vec()),
            name: "myfile".to_owned(),
            ..Default::default()
        };
        let file2 = db::files::Model {
            owning_entity: db::files::OwningEntity::Game,
            owning_id: Some(124),
            content: Some(b"abracaabra-2".to_vec()),
            name: "myfile".to_owned(),
            ..Default::default()
        };
        let file3 = db::files::Model {
            owning_entity: db::files::OwningEntity::Program,
            owning_id: Some(123),
            content: Some(b"abracadabra-3-xxx".to_vec()),
            name: "myfile".to_owned(),
            ..Default::default()
        };
        let file4 = db::files::Model {
            owning_entity: db::files::OwningEntity::Program,
            owning_id: Some(123),
            content: Some(b"bracadabra-4".to_vec()),
            name: "myfile-4".to_owned(),
            ..Default::default()
        };
        store.write(&db, requester, file1.clone()).await.unwrap();
        store.write(&db, requester, file2.clone()).await.unwrap();
        store.write(&db, requester, file3.clone()).await.unwrap();
        store.write(&db, requester, file4.clone()).await.unwrap();
        let mut got1 = store
            .read(
                &db,
                requester,
                db::files::OwningEntity::Game,
                Some(123),
                "myfile",
            )
            .await
            .expect("Failed to read file 1");
        let mut got2 = store
            .read(
                &db,
                requester,
                db::files::OwningEntity::Game,
                Some(124),
                "myfile",
            )
            .await
            .expect("Failed to read file 2");
        let mut got3 = store
            .read(
                &db,
                requester,
                db::files::OwningEntity::Program,
                Some(123),
                "myfile",
            )
            .await
            .expect("Failed to read file 3");
        let mut got4 = store
            .read(
                &db,
                requester,
                db::files::OwningEntity::Program,
                Some(123),
                "myfile-4",
            )
            .await
            .expect("Failed to read file 4");
        let mut file5 = file4.clone();
        file5.content = Some(b"abracadabra-5-new-content".to_vec());
        store.write(&db, requester, file5.clone()).await.unwrap();
        let mut got5 = store
            .read(
                &db,
                requester,
                db::files::OwningEntity::Program,
                Some(123),
                "myfile-4",
            )
            .await
            .expect("Failed to read file 4");
        assert_eq!(got4.id, got5.id);
        got1.id = 0;
        got2.id = 0;
        got3.id = 0;
        got4.id = 0;
        got5.id = 0;
        assert_eq!(got1, file1);
        assert_eq!(got2, file2);
        assert_eq!(got3, file3);
        assert_eq!(got4, file4);
        assert_eq!(got5, file5);
        store
            .delete(
                &db,
                requester,
                db::files::OwningEntity::Program,
                Some(123),
                "myfile-4",
            )
            .await
            .expect("Failed to delete file 4");
        assert_eq!(
            store
                .read(
                    &db,
                    requester,
                    db::files::OwningEntity::Program,
                    Some(123),
                    "myfile-4"
                )
                .await,
            Err(Error::NotFound)
        );
        // Other files should be intact after deleting one.
        let mut got6 = store
            .read(
                &db,
                requester,
                db::files::OwningEntity::Game,
                Some(123),
                "myfile",
            )
            .await
            .expect("Failed to read file 4");
        got6.id = 0;
        assert_eq!(got6, file1);
    }

    #[test]
    fn test_compress_decompress() {
        let text = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let file1 = db::files::Model {
            content: Some(text.to_vec()),
            name: "myfile".to_owned(),
            ..Default::default()
        };
        let file2 = FileStore::compress(file1.clone()).expect("Failed to compress");
        assert_eq!(file2.compression, db::files::Compression::Gzip);
        assert!(
            file2.content.as_ref().unwrap().len() <= text.len() / 2,
            "Expected compression factor to be at least 2; compressed={} plain={}",
            file2.content.unwrap().len(),
            text.len()
        );
        let file3 = FileStore::decompress(file2).expect("Failed to decompress");
        assert_eq!(file1, file3);
    }
}

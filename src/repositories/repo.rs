use crate::models::data::Data;
use crate::postgres::PostgresPool;
use crate::repositories::error::Error;
use async_trait::async_trait;
use coi::Inject;
use mobc_postgres::tokio_postgres::NoTls;
use std::sync::Arc;

pub struct DbData {
    id: i64,
    name: String,
}

impl From<DbData> for Data {
    fn from(db_data: DbData) -> Data {
        Data {
            id: db_data.id,
            name: db_data.name,
        }
    }
}

#[async_trait]
pub trait IRepository: Inject {
    async fn get(&self, id: i64) -> Result<DbData, Error>;
    async fn get_all(&self) -> Result<Vec<DbData>, Error>;
}

#[derive(Inject)]
#[coi(provides pub dyn IRepository with Repository::new(pool))]
struct Repository {
    #[coi(inject)]
    pool: Arc<PostgresPool<NoTls>>,
}

#[async_trait]
impl IRepository for Repository {
    async fn get(&self, id: i64) -> Result<DbData, Error> {
        let client = self.pool.get().await?;
        let statement = client
            .prepare("SELECT id, name FROM data WHERE id=$1::BIGINT")
            .await?;
        let row = client.query_one(&statement, &[&id]).await?;
        let data = DbData {
            id: row.get(0),
            name: row.get(1),
        };
        Ok(data)
    }

    async fn get_all(&self) -> Result<Vec<DbData>, Error> {
        let client = self.pool.get().await?;
        let statement = client.prepare("SELECT id, name FROM data LIMIT 50").await?;
        let rows = client.query(&statement, &[]).await?;
        let data = rows
            .into_iter()
            .map(|row| DbData {
                id: row.get(0),
                name: row.get(1),
            })
            .collect::<Vec<_>>();
        Ok(data)
    }
}

impl Repository {
    fn new(pool: Arc<PostgresPool<NoTls>>) -> Self {
        Self { pool }
    }
}

use crate::dtos::data::DataDto;
use crate::services::service::IService;
use actix_web::web::{self, ServiceConfig};
use actix_web::{Error, HttpResponse, Responder};
use coi_actix_web::inject;

#[inject]
async fn get(
    id: web::Path<i64>,
    #[inject] service: Arc<dyn IService>,
) -> Result<impl Responder, Error> {
    let data = service.get(*id).await?;
    Ok(HttpResponse::Ok().json(DataDto::from(data)))
}

#[inject]
async fn use_two_deps(
    path: web::Path<(i64, i64)>,
    #[inject] service: Arc<dyn IService>,
    #[inject] service2: Arc<dyn IService>,
) -> Result<impl Responder, Error> {
    let data = service.get(path.0).await?;
    let data2 = service2.get(path.1).await?;
    Ok(HttpResponse::Ok().json([DataDto::from(data), DataDto::from(data2)]))
}

#[inject]
async fn get_all(#[inject] service: Arc<dyn IService>) -> Result<impl Responder, Error> {
    let data = service.get_all().await?;
    Ok(HttpResponse::Ok().json(data.into_iter().map(DataDto::from).collect::<Vec<_>>()))
}

pub fn route_config(config: &mut ServiceConfig) {
    config.service(
        web::scope("/data")
            .route("", web::get().to(get_all))
            .route("/", web::get().to(get_all))
            .route("/{id}", web::get().to(get))
            .route("/{id}/{id2}", web::get().to(use_two_deps)),
    );
}

use bollard::Docker;
use std::io::{self, ErrorKind};
use std::process::{Child, Command, ExitStatus};
use std::time::Duration;
use structopt::StructOpt;
use thiserror::Error;
use tokio_postgres::{connect, Client, Error as PostgresError, NoTls};

#[derive(StructOpt)]
#[structopt(
    about = "Each subcommand listed below will run the subcommand before it, in this order:
build, run, init, seed

Once init or seed has been run, you can just call the run subcommand and reuse
the existing data."
)]
enum Step {
    #[structopt(about = "Build the postgres docker image")]
    Build,
    #[structopt(about = "Run the postgres docker image")]
    Run,
    #[structopt(about = "Initialize the database in the postgres docker image")]
    Init,
    #[structopt(about = "Seed dummy data to the postgres docker image")]
    Seed,
}

#[derive(Error, Debug)]
enum XtaskError {
    #[error("Io Error: {0}")]
    Io(#[from] io::Error),

    #[error("Docker Error: {0}")]
    Docker(#[from] bollard::errors::Error),

    #[error("Postgres error: {0}")]
    Postgres(#[from] PostgresError),

    #[error("Command `{0}` did not exit: {1}")]
    Exit(String, ExitStatus),

    #[error("Uknown error: {0}")]
    Unknown(String),
}

type Result<T> = std::result::Result<T, XtaskError>;

const DOCKER_COMMAND: &str = "docker";
// const DOCKER_URI: &str = "unix:///var/run/docker.sock";
const DOCKER_IMAGE_NAME: &str = "coi-actix-sample-postgres";

fn build_step() -> Result<()> {
    let mut command = build()?;
    success_check(command.wait(), DOCKER_COMMAND)
}

async fn run_step() -> Result<()> {
    let docker = Docker::connect_with_local_defaults()?;
    let images = docker.list_images::<String>(None).await?;
    if !images.iter().any(|i| i.id == DOCKER_IMAGE_NAME) {
        build_step()?;
    }
    let mut command = run()?;
    success_check(command.wait(), DOCKER_COMMAND)
}

async fn init_step() -> Result<()> {
    let docker = Docker::connect_with_local_defaults()?;
    let containers = docker.list_containers::<String>(None).await?;
    let container = containers
        .iter()
        .find(|c| c.image.as_deref() == Some(DOCKER_IMAGE_NAME));
    if let Some(_container) = container {
        let mut client = make_client().await?;
        init_db(&mut client).await
    } else {
        let images = docker.list_images::<String>(None).await?;
        if !images.iter().any(|i| {
            i.repo_tags
                .iter()
                .any(|t| t == &format!("{}:latest", DOCKER_IMAGE_NAME))
        }) {
            build_step()?;
        }
        let mut command = run()?;
        if let Err(e) = async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let mut client = make_client().await?;
            init_db(&mut client).await
        }
        .await
        {
            command.kill().map_err(Into::<XtaskError>::into)?;
            Err(XtaskError::Unknown(format!("{:?}", e)))
        } else {
            Ok(())
        }
    }
}

async fn seed_step() -> Result<()> {
    let docker = Docker::connect_with_local_defaults()?;
    let containers = docker.list_containers::<String>(None).await?;
    let container = containers
        .iter()
        .find(|c| c.image.as_deref() == Some(DOCKER_IMAGE_NAME));
    if let Some(_container) = container {
        let mut client = make_client().await?;
        init_db(&mut client).await?;
        seed(&mut client).await
    } else {
        let images = docker.list_images::<String>(None).await?;
        if !images.iter().any(|i| i.id == DOCKER_IMAGE_NAME) {
            build_step()?;
        }
        let mut command = run()?;
        if let Err(e) = async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let mut client = make_client().await?;
            init_db(&mut client).await?;
            seed(&mut client).await
        }
        .await
        {
            command.kill().map_err(Into::<XtaskError>::into)?;
            Err(e)
        } else {
            Ok(())
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let step = Step::from_args();
    match step {
        Step::Build => build_step(),
        Step::Run => run_step().await,
        Step::Init => init_step().await,
        Step::Seed => seed_step().await,
    }
}

fn check_not_found(command: &str) -> impl Fn(io::Error) -> io::Error + '_ {
    move |e| {
        if e.kind() == ErrorKind::NotFound {
            io::Error::new(
                ErrorKind::NotFound,
                format!("{} not found on this system: {}", command, e),
            )
        } else {
            e
        }
    }
}

fn success_check(res: io::Result<ExitStatus>, command: &str) -> Result<()> {
    let status = res?;
    if status.success() {
        Ok(())
    } else {
        Err(XtaskError::Exit(command.to_owned(), status))
        // Err(io::Error::new(
        //     ErrorKind::Other,
        //     format!(
        //         "{} could not run successfully: exit code {:?}",
        //         command,
        //         status.code()
        //     ),
        // ).into())
    }
}

fn build() -> Result<Child> {
    Command::new("docker")
        .arg("build")
        .arg(".")
        .arg("-t")
        .arg(DOCKER_IMAGE_NAME)
        .spawn()
        .map_err(check_not_found("docker"))
        .map_err(Into::into)
}

fn run() -> Result<Child> {
    println!("Running docker");
    Command::new("docker")
        .arg("run")
        .arg("-p")
        .arg("45432:5432")
        .arg(DOCKER_IMAGE_NAME)
        .spawn()
        .map_err(check_not_found("docker"))
        .map_err(Into::into)
}

async fn make_client() -> Result<Client> {
    let (client, connection) = connect(
        "host=127.0.0.1 dbname=docker port=45432 user=docker password=docker",
        NoTls,
    )
    .await?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });
    Ok(client)
}

async fn init_db(client: &mut Client) -> Result<()> {
    client
        .batch_execute(include_str!("sql/init.sql"))
        .await
        .map_err(Into::into)
}

async fn seed(client: &mut Client) -> Result<()> {
    client
        .batch_execute(include_str!("sql/seed.sql"))
        .await
        .map_err(Into::into)
}

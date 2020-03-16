use postgres::{Client, NoTls, Error as PostgresError};
use rs_docker::Docker;
use std::{
    io::{self, ErrorKind},
    process::{Child, Command, ExitStatus},
    thread,
    time::Duration,
};
use structopt::StructOpt;
use thiserror::Error;

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
    #[error("Io Error")]
    Io(#[from] io::Error),

    #[error("Postgres error: {0}")]
    Postgres(#[from] PostgresError),

    #[error("Command `{0}` did not exit: {1}")]
    Exit(String, ExitStatus),

    #[error("Uknown error: {0}")]
    Unknown(String),
}

type Result<T> = std::result::Result<T, XtaskError>;

const DOCKER_COMMAND: &str = "docker";
const DOCKER_URI: &str = "unix:///var/run/docker.sock";
const DOCKER_IMAGE_NAME: &str = "coi-actix-sample-postgres";

fn build_step() -> Result<()> {
    let mut command = build()?;
    success_check(command.wait(), DOCKER_COMMAND)
}

fn run_step() -> Result<()> {
    let mut docker = Docker::connect(DOCKER_URI)?;
    let images = docker.get_images(false)?;
    if !images.iter().any(|i| i.Id == DOCKER_IMAGE_NAME) {
        build_step()?;
    }
    let mut command = run()?;
    success_check(command.wait(), DOCKER_COMMAND)
}

fn init_step() -> Result<()> {
    let mut docker = Docker::connect(DOCKER_URI)?;
    let containers = docker.get_containers(false)?;
    let container = containers
        .iter()
        .filter(|c| c.Image == DOCKER_IMAGE_NAME)
        .next();
    if let Some(_container) = container {
        let mut client = make_client()?;
        init_db(&mut client)
    } else {
        let images = docker.get_images(false)?;
        if !images.iter().any(|i| {
            i.RepoTags
                .iter()
                .any(|t| t == &format!("{}:latest", DOCKER_IMAGE_NAME))
        }) {
            build_step()?;
        }
        let mut command = run()?;
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_secs(5));
            let mut client = make_client()?;
            init_db(&mut client)
        });
        if let Err(e) = handle.join() {
            command.kill().map_err(Into::<XtaskError>::into)?;
            match e.downcast::<XtaskError>() {
                Ok(e) => Err(*e),
                Err(e) => Err(XtaskError::Unknown(format!("{:?}", e)))
            }
        } else {
            Ok(())
        }
    }
}

fn seed_step() -> Result<()> {
    let mut docker = Docker::connect(DOCKER_URI)?;
    let containers = docker.get_containers(false)?;
    let container = containers
        .iter()
        .filter(|c| c.Image == DOCKER_IMAGE_NAME)
        .next();
    if let Some(_container) = container {
        let mut client = make_client()?;
        init_db(&mut client)?;
        seed(&mut client)
    } else {
        let images = docker.get_images(false)?;
        if !images.iter().any(|i| i.Id == DOCKER_IMAGE_NAME) {
            build_step()?;
        }
        let mut command = run()?;
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_secs(5));
            let mut client = make_client()?;
            init_db(&mut client)?;
            seed(&mut client)
        });
        if let Err(e) = handle.join() {
            command.kill().map_err(Into::<XtaskError>::into)?;
            match e.downcast::<XtaskError>() {
                Ok(e) => Err(*e),
                Err(e) => Err(XtaskError::Unknown(format!("{:?}", e)))
            }
        } else {
            Ok(())
        }
    }
}

fn main() {
    let step = Step::from_args();
    if let Err(e) = match step {
        Step::Build => build_step(),
        Step::Run => run_step(),
        Step::Init => init_step(),
        Step::Seed => seed_step(),
    } {
        eprintln!("Failed to run step: {}", e);
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
    Command::new("docker")
        .arg("run")
        .arg("-p")
        .arg("45432:5432")
        .arg(DOCKER_IMAGE_NAME)
        .spawn()
        .map_err(check_not_found("docker"))
        .map_err(Into::into)
}

fn make_client() -> Result<Client> {
    Client::connect("host=127.0.0.1 dbname=docker port=45432 user=docker password=docker", NoTls).map_err(Into::into)
}

fn init_db(client: &mut Client) -> Result<()> {
    client.batch_execute(include_str!("sql/init.sql")).map_err(Into::into)
}

fn seed(client: &mut Client) -> Result<()> {
    client.batch_execute(include_str!("sql/seed.sql")).map_err(Into::into)
}

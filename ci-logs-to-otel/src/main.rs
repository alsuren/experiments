use std::{
    fs::File,
    io::{Cursor, Read, Write},
};

use bytes::Bytes;
use octocrab::models::workflows::Run;
use std::sync::Arc;
use tracing::instrument;
use zip::ZipArchive;

use async_executors::TokioTpBuilder;
use opentelemetry::trace::{FutureExt, TraceContextExt, Tracer};
use opentelemetry::Context;
use opentelemetry::{
    global::{shutdown_tracer_provider, tracer},
    trace::Span,
};
use opentelemetry_honeycomb::HoneycombApiKey;

fn main() -> Result<(), anyhow::Error> {
    let owner = std::env::var("GITHUB_OWNER").expect("GITHUB_OWNER env variable is required");
    let repo = std::env::var("GITHUB_REPO").expect("GITHUB_REPO env variable is required");
    let token = std::env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN env variable is required");

    let mut builder = TokioTpBuilder::new();
    builder.tokio_builder().enable_io().enable_time();
    let executor = Arc::new(builder.build().expect("Failed to build Tokio executor"));

    register_otel_honeycomb(executor.clone())?;

    octocrab::initialise(octocrab::Octocrab::builder().personal_token(token)).unwrap();

    // tracer.in_span("doing_work", |cx| {
    //     cx
    //     // Traced app logic here...
    // });
    let res = executor.block_on(analyze_logs(owner, repo));

    shutdown_tracer_provider();
    res
}

fn register_otel_honeycomb(executor: Arc<async_executors::TokioTp>) -> Result<(), anyhow::Error> {
    let (_flusher, _tracer) = opentelemetry_honeycomb::new_pipeline(
        HoneycombApiKey::new(
            std::env::var("HONEYCOMB_API_KEY")
                .expect("Missing or invalid environment variable HONEYCOMB_API_KEY"),
        ),
        std::env::var("HONEYCOMB_DATASET")
            .expect("Missing or invalid environment variable HONEYCOMB_DATASET"),
        executor.clone(),
        {
            let executor = executor.clone();
            move |fut| executor.block_on(fut)
        },
    )
    .install()?;
    Ok(())
}

#[instrument]
async fn analyze_logs(owner: String, repo: String) -> Result<(), anyhow::Error> {
    let runs = octocrab::instance()
        .workflows(&owner, &repo)
        .list_all_runs()
        .per_page(10)
        // .branch("master")
        // .event("push")
        // .status("success")
        .send()
        .await?;
    for run in runs {
        if run.name != "test" || run.status != "completed" {
            continue;
        }

        dbg!((&run.id, &run.head_branch, &run.conclusion));

        let tracer = tracer("");
        let span = tracer
            .span_builder("run")
            .with_parent_context(Context::current())
            .with_start_time(run.created_at.clone())
            .with_end_time(run.updated_at.clone())
            .start(&tracer);
        let context = Context::current_with_span(span);

        let zip = get_run_log_zipfile(&owner, &repo, run).await?;
        let mut zip = match ZipArchive::new(Cursor::new(zip)) {
            Ok(zip) => zip,
            Err(e) => {
                eprintln!("ERROR: could not read zip: {e:?}");
                continue;
            }
        };
        let mut log_file_contents = String::new();
        for i in 0..zip.len() {
            let mut log_file = zip.by_index(i)?;
            log_file.read_to_string(&mut log_file_contents)?;
            let log_name = log_file.name();
            if let Some((first_line, last_line)) = parse_log(log_name, &log_file_contents)
                .with_context(context.clone())
                .await
            {
                dbg!((log_name, first_line, last_line));
            } else {
                eprintln!("{log_name} not valid");
            }
        }
    }
    Ok(())
}

async fn parse_log<'a>(
    log_name: &'a str,
    log_file_contents: &'a String,
) -> Option<(&'a str, &'a str)> {
    let (first_line, rest) = log_file_contents.split_once('\n')?;
    let (rest, _trailing_newline) = rest.rsplit_once('\n')?;
    let (_, last_line) = rest.rsplit_once('\n')?;

    let (start_time, _) = first_line.split_once(' ')?;
    let (end_time, _) = last_line.split_once(' ')?;

    let tracer = tracer("");
    let mut span = tracer
        .span_builder(log_name.to_owned())
        .with_parent_context(Context::current())
        .with_start_time(chrono::DateTime::parse_from_rfc3339(start_time).ok()?)
        .with_end_time(chrono::DateTime::parse_from_rfc3339(end_time).ok()?)
        .start(&tracer);
    span.end_with_timestamp(chrono::DateTime::parse_from_rfc3339(end_time).ok()?.into());
    Some((first_line, last_line))
}

// FIXME: can we infer owner and repo from Run?
#[instrument]
async fn get_run_log_zipfile(
    owner: &String,
    repo: &String,
    run: Run,
) -> Result<Bytes, anyhow::Error> {
    // cache in ~/tmp/logs
    let log_dir = home::home_dir()
        .unwrap()
        .join("tmp/logs")
        .join(owner)
        .join(repo);
    let branch_dir = log_dir.join(&run.head_branch);
    std::fs::create_dir_all(&branch_dir).unwrap();
    let mut zipfile_path = branch_dir.join(&run.id.to_string());
    assert!(zipfile_path.set_extension("zip"));

    if zipfile_path.exists() {
        let mut logs = Vec::new();
        File::open(zipfile_path)?.read_to_end(&mut logs)?;
        Ok(logs.into())
    } else {
        let logs = octocrab::instance()
            .actions()
            .download_workflow_run_logs(owner, repo, run.id)
            .await?;
        // FIXME: this is not atomic. Write to a tempfile and mv instead.
        File::options()
            .write(true)
            .truncate(true)
            .create(true)
            .open(zipfile_path)?
            .write_all(&logs)?;
        Ok(logs)
    }
}

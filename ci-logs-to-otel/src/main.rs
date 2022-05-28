// mod honeycomb;

// use std::{
//     fs::File,
//     io::{Cursor, Read, Write},
// };

// use bytes::Bytes;
// use octocrab::models::workflows::Run;
// use tracing::instrument;
// use zip::ZipArchive;

// #[tokio::main]
// async fn main() -> Result<(), Box<dyn std::error::Error>> {
//     let owner = std::env::var("GITHUB_OWNER").expect("GITHUB_OWNER env variable is required");
//     let repo = std::env::var("GITHUB_REPO").expect("GITHUB_REPO env variable is required");
//     let token = std::env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN env variable is required");
//     let honeycomb_key =
//         std::env::var("HONEYCOMB_KEY").expect("GITHUB_TOKEN env variable is required");

//     honeycomb::register_global_subscriber(honeycomb_key);
//     octocrab::initialise(octocrab::Octocrab::builder().personal_token(token)).unwrap();
//     println!("Hello, world!");

//     analyze_logs(owner, repo).await?;

//     Ok(())
// }

// #[instrument]
// async fn analyze_logs(owner: String, repo: String) -> Result<(), anyhow::Error> {
//     let runs = octocrab::instance()
//         .workflows(&owner, &repo)
//         .list_all_runs()
//         .per_page(100)
//         // .branch("master")
//         // .event("push")
//         // .status("success")
//         .send()
//         .await?;
//     for run in runs {
//         if run.name != "test" && run.status != "completed" {
//             continue;
//         }
//         dbg!((&run.id, &run.head_branch, &run.conclusion));

//         let zip = get_run_log_zipfile(&owner, &repo, run).await?;
//         let mut zip = match ZipArchive::new(Cursor::new(zip)) {
//             Ok(zip) => zip,
//             Err(e) => {
//                 eprintln!("ERROR: could not read zip: {e:?}");
//                 continue;
//             }
//         };
//         let mut log_file_contents = String::new();
//         for i in 0..zip.len() {
//             let mut log_file = zip.by_index(i)?;
//             log_file.read_to_string(&mut log_file_contents)?;
//             let log_name = log_file.name();
//             if let Some((first_line, last_line)) = parse_log(&log_file_contents) {
//                 dbg!((log_name, first_line, last_line));
//             } else {
//                 eprintln!("{log_name} not valid");
//             }
//         }
//     }
//     Ok(())
// }

// fn parse_log(log_file_contents: &String) -> Option<(&str, &str)> {
//     let (first_line, rest) = log_file_contents.split_once('\n')?;
//     let (rest, _trailing_newline) = rest.rsplit_once('\n')?;
//     let (_, last_line) = rest.rsplit_once('\n')?;
//     Some((first_line, last_line))
// }

// // FIXME: can we infer owner and repo from Run?
// #[instrument]
// async fn get_run_log_zipfile(
//     owner: &String,
//     repo: &String,
//     run: Run,
// ) -> Result<Bytes, anyhow::Error> {
//     // cache in ~/tmp/logs
//     let log_dir = home::home_dir()
//         .unwrap()
//         .join("tmp/logs")
//         .join(owner)
//         .join(repo);
//     let branch_dir = log_dir.join(&run.head_branch);
//     std::fs::create_dir_all(&branch_dir).unwrap();
//     let mut zipfile_path = branch_dir.join(&run.id.to_string());
//     assert!(zipfile_path.set_extension("zip"));

//     if zipfile_path.exists() {
//         let mut logs = Vec::new();
//         File::open(zipfile_path)?.read_to_end(&mut logs)?;
//         Ok(logs.into())
//     } else {
//         let logs = octocrab::instance()
//             .actions()
//             .download_workflow_run_logs(owner, repo, run.id)
//             .await?;
//         // FIXME: this is not atomic. Write to a tempfile and mv instead.
//         File::options()
//             .write(true)
//             .truncate(true)
//             .create(true)
//             .open(zipfile_path)?
//             .write_all(&logs)?;
//         Ok(logs)
//     }
// }
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_executors::TokioTpBuilder;
use opentelemetry::global::{shutdown_tracer_provider, tracer};
use opentelemetry::trace::{FutureExt, TraceContextExt, Tracer};
use opentelemetry::Context;
use opentelemetry_honeycomb::HoneycombApiKey;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let mut builder = TokioTpBuilder::new();
    builder.tokio_builder().enable_io().enable_time();
    let executor = Arc::new(builder.build().expect("Failed to build Tokio executor"));

    // Create a new instrumentation pipeline.
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

    // tracer.in_span("doing_work", |cx| {
    //     cx
    //     // Traced app logic here...
    // });
    executor.block_on(recurse(5));

    shutdown_tracer_provider();
    Ok(())
}

#[async_recursion::async_recursion]
async fn recurse(depth: u64) {
    if depth == 0 {
        return;
    }

    let tracer = tracer("");
    let span = tracer
        .span_builder("recurse")
        .with_parent_context(Context::current())
        .with_start_time(SystemTime::now() - Duration::from_secs(depth + 10))
        .with_end_time(SystemTime::now() - Duration::from_secs(10))
        .start(&tracer);

    recurse(depth - 1)
        .with_context(Context::current_with_span(span))
        .await;

    // ??? does the span automatically get ended when the context dies?
    // span.end_with_timestamp(SystemTime::now());
}

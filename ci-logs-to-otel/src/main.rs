use std::{
    fs::File,
    io::{Cursor, Read, Write},
};

use bytes::Bytes;
use octocrab::models::workflows::Run;
use zip::ZipArchive;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let owner = std::env::var("GITHUB_OWNER").expect("GITHUB_OWNER env variable is required");
    let repo = std::env::var("GITHUB_REPO").expect("GITHUB_REPO env variable is required");
    let token = std::env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN env variable is required");

    octocrab::initialise(octocrab::Octocrab::builder().personal_token(token)).unwrap();
    println!("Hello, world!");

    analyze_logs(owner, repo).await?;

    Ok(())
}

async fn analyze_logs(owner: String, repo: String) -> Result<(), anyhow::Error> {
    let runs = octocrab::instance()
        .workflows(&owner, &repo)
        .list_all_runs()
        .per_page(100)
        // .branch("master")
        // .event("push")
        // .status("success")
        .send()
        .await?;
    for run in runs {
        if run.name != "test" && run.status != "completed" {
            continue;
        }
        dbg!((&run.id, &run.head_branch, &run.conclusion));

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
            if let Some((first_line, last_line)) = parse_log(&log_file_contents) {
                dbg!((log_name, first_line, last_line));
            } else {
                eprintln!("{log_name} not valid");
            }
        }
    }
    Ok(())
}

fn parse_log(log_file_contents: &String) -> Option<(&str, &str)> {
    let (first_line, rest) = log_file_contents.split_once('\n')?;
    let (rest, _trailing_newline) = rest.rsplit_once('\n')?;
    let (_, last_line) = rest.rsplit_once('\n')?;
    Some((first_line, last_line))
}

// FIXME: can we infer owner and repo from Run?
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

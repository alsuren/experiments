use std::{error::Error, fs::File, io::Write};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let owner = std::env::var("GITHUB_OWNER").expect("GITHUB_OWNER env variable is required");
    let repo = std::env::var("GITHUB_REPO").expect("GITHUB_REPO env variable is required");
    let token = std::env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN env variable is required");

    octocrab::initialise(octocrab::Octocrab::builder().personal_token(token)).unwrap();
    println!("Hello, world!");

    download_logs(owner, repo).await?;

    Ok(())
}

async fn download_logs(owner: String, repo: String) -> Result<(), Box<dyn Error>> {
    let runs = octocrab::instance()
        .workflows(&owner, &repo)
        .list_all_runs()
        .per_page(100)
        // .branch("master")
        // .event("push")
        // .status("success")
        .send()
        .await?;
    let log_dir = home::home_dir()
        .unwrap()
        .join("tmp/logs")
        .join(&owner)
        .join(&repo);
    std::fs::create_dir_all(&log_dir).unwrap();
    Ok(for run in runs {
        if run.name != "test" && run.status != "completed" {
            continue;
        }
        dbg!((&run.id, &run.head_branch, &run.conclusion));
        let branch_dir = log_dir.join(&run.head_branch);
        std::fs::create_dir_all(&branch_dir).unwrap();
        let mut zipfile_path = branch_dir.join(&run.id.to_string());

        assert!(zipfile_path.set_extension("zip"));

        if !zipfile_path.exists() {
            let logs = octocrab::instance()
                .actions()
                .download_workflow_run_logs(&owner, &repo, run.id)
                .await?;
            File::options()
                .write(true)
                .truncate(true)
                .create(true)
                .open(zipfile_path)?
                .write_all(&logs)?;
        }
    })
}

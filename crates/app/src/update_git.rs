//! Read public commit metadata when the REST API is unavailable or rate limited.
use super::{
    BuildIdentity, GITHUB_OWNER, GITHUB_REPO, UpdateCheckResult, UpdateCommit, UpdateError,
    UpdateRelation,
};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(super) fn is_commit_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn check(identity: &BuildIdentity) -> Result<UpdateCheckResult, UpdateError> {
    let platform = platform::native_platform();
    let work = platform.create_temp_dir("update-check").map_err(|error| {
        UpdateError::new(format!("could not create Git check directory: {error}"))
    })?;
    let result = check_in(
        identity,
        &format!("https://github.com/{GITHUB_OWNER}/{GITHUB_REPO}.git"),
        &work,
    );
    let _ = platform.cleanup_temp_path(&work);
    result
}

fn check_in(
    identity: &BuildIdentity,
    remote: &str,
    work: &Path,
) -> Result<UpdateCheckResult, UpdateError> {
    let git = Git {
        work: work.to_owned(),
        deadline: Instant::now() + Duration::from_secs(60),
    };
    // No checkout, hooks or source files are needed. Fetch complete commit history
    // so an old installed build cannot be mistaken for an unrelated shallow tip.
    git.run(&[
        "clone",
        "--bare",
        "--filter=tree:0",
        "--single-branch",
        "--no-tags",
        "--",
        remote,
        "history",
    ])?;
    let default_branch = git.repo(&["symbolic-ref", "--short", "HEAD"])?;
    let head_sha = git.repo(&["rev-parse", "HEAD"])?;
    if !is_commit_sha(&head_sha) {
        return Err(UpdateError::new(
            "Git returned an invalid default branch commit",
        ));
    }
    let (relation, range) = match identity.commit_sha.as_deref() {
        Some(local) if local == head_sha => (UpdateRelation::Identical, None),
        Some(local) if is_commit_sha(local) => {
            // Only infer ancestry when the installed commit is reachable in the
            // complete default-branch history. Ahead/diverged builds stay Unknown.
            let history = git.repo(&["rev-list", &head_sha])?;
            if history.lines().any(|sha| sha == local)
                && git.repo(&["rev-parse", "--is-shallow-repository"])? == "false"
            {
                let range = format!("{local}..{head_sha}");
                let count = git
                    .repo(&["rev-list", "--count", &range])?
                    .parse()
                    .map_err(|_| UpdateError::new("Git returned an invalid commit count"))?;
                (
                    UpdateRelation::Behind {
                        remote_ahead: count,
                    },
                    Some(range),
                )
            } else {
                (UpdateRelation::Unknown, Some(head_sha.clone()))
            }
        }
        _ => (UpdateRelation::Unknown, Some(head_sha.clone())),
    };
    let commits = if let Some(range) = range {
        let mut args = vec!["log", "--format=%H%x00%B", "-z"];
        if relation == UpdateRelation::Unknown {
            args.push("--max-count=20");
        } else {
            args.push("--reverse");
        }
        args.extend([&range, "--"]);
        parse_commits(&git.repo(&args)?)?
    } else {
        Vec::new()
    };
    Ok(UpdateCheckResult {
        default_branch,
        head_sha,
        relation,
        commits,
    })
}

fn parse_commits(output: &str) -> Result<Vec<UpdateCommit>, UpdateError> {
    let mut parts = output.split('\0');
    let mut commits = Vec::new();
    while let Some(sha) = parts.next() {
        if sha.is_empty() {
            break;
        }
        let message = parts
            .next()
            .ok_or_else(|| UpdateError::new("Git returned an incomplete commit"))?;
        if !is_commit_sha(sha) {
            return Err(UpdateError::new("Git returned an invalid commit SHA"));
        }
        commits.push(UpdateCommit {
            sha: sha.to_owned(),
            message: message.trim_end().to_owned(),
        });
    }
    Ok(commits)
}

struct Git {
    work: PathBuf,
    deadline: Instant,
}

impl Git {
    fn repo(&self, args: &[&str]) -> Result<String, UpdateError> {
        let mut command = vec!["--git-dir=history"];
        command.extend_from_slice(args);
        self.run(&command)
    }

    fn run(&self, args: &[&str]) -> Result<String, UpdateError> {
        if Instant::now() >= self.deadline {
            return Err(UpdateError::new("Git update check timed out"));
        }
        // Files avoid pipe backpressure while polling the process deadline.
        let stdout = self.work.join("git.stdout");
        let stderr = self.work.join("git.stderr");
        let mut child = Command::new("git")
            .args([
                "-c",
                "credential.helper=",
                "-c",
                "http.lowSpeedLimit=1",
                "-c",
                "http.lowSpeedTime=20",
            ])
            .args(args)
            .current_dir(&self.work)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .stdin(Stdio::null())
            .stdout(File::create(&stdout)?)
            .stderr(File::create(&stderr)?)
            .spawn()
            .map_err(|error| {
                UpdateError::new(format!(
                    "could not run git (install Git to enable the fallback): {error}"
                ))
            })?;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        return Err(UpdateError::new(format!(
                            "git failed: {}",
                            super::tail(&fs::read_to_string(stderr).unwrap_or_default(), 1000)
                        )));
                    }
                    return Ok(String::from_utf8_lossy(&fs::read(stdout)?)
                        .trim_end_matches(['\r', '\n'])
                        .to_owned());
                }
                Ok(None) if Instant::now() < self.deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                result => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(UpdateError::new(match result {
                        Err(error) => format!("could not wait for git: {error}"),
                        _ => "Git update check timed out".to_owned(),
                    }));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_git_reads_history_without_a_checkout_or_api() {
        let root = super::super::tests::update_test_root("git-history");
        fs::create_dir_all(&root).unwrap();
        let git = Git {
            work: root.clone(),
            deadline: Instant::now() + Duration::from_secs(60),
        };
        git.run(&["init", "--bare", "--initial-branch=master", "history"])
            .unwrap();
        let tree = git.repo(&["mktree"]).unwrap();
        let commit = |message: &str, parent: Option<&str>| {
            let mut args = vec![
                "-c",
                "user.name=Codex",
                "-c",
                "user.email=codex@local",
                "commit-tree",
                &tree,
                "-m",
                message,
            ];
            if let Some(parent) = parent {
                args.extend(["-p", parent]);
            }
            git.repo(&args).unwrap()
        };
        let first = commit("first", None);
        let second = commit("second\n\nDetailed body", Some(&first));
        let third = commit("third", Some(&second));
        git.repo(&["update-ref", "refs/heads/master", &third])
            .unwrap();
        let remote = format!("file://{}", root.join("history").display());
        for (index, (local, expected, count)) in [
            (Some(first), UpdateRelation::Behind { remote_ahead: 2 }, 2),
            (Some(third.clone()), UpdateRelation::Identical, 0),
            (Some("a".repeat(40)), UpdateRelation::Unknown, 3),
            (None, UpdateRelation::Unknown, 3),
            (Some("--invalid".into()), UpdateRelation::Unknown, 3),
        ]
        .into_iter()
        .enumerate()
        {
            let work = root.join(index.to_string());
            fs::create_dir(&work).unwrap();
            let result = check_in(
                &BuildIdentity {
                    package_version: "0.1.1".into(),
                    commit_sha: local,
                    dirty: false,
                },
                &remote,
                &work,
            )
            .unwrap();
            assert_eq!(result.default_branch, "master");
            assert_eq!(result.head_sha, third);
            assert_eq!(result.relation, expected);
            assert_eq!(result.commits.len(), count);
            if index == 0 {
                assert_eq!(result.commits[0].message, "second\n\nDetailed body");
                assert_eq!(result.commits[1].sha, third);
            }
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn update_git_expired_deadline_does_not_launch_a_command() {
        let git = Git {
            work: PathBuf::new(),
            deadline: Instant::now(),
        };
        assert!(
            git.run(&["--version"])
                .unwrap_err()
                .to_string()
                .contains("timed out")
        );
    }
}

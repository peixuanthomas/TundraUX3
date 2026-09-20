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
    let remote = reqwest::Url::from_directory_path(root.join("history"))
        .expect("absolute repository path must convert to a file URL");
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
            remote.as_str(),
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

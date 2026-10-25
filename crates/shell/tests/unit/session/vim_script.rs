use super::*;
use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig, WatchdogRuntime};

struct TestDirectory(PathBuf);
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn until(
    host: &mut VimHost,
    predicate: impl Fn(&vim_runtime::VimPoll) -> bool,
) -> vim_runtime::VimPoll {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let poll = host.poll().unwrap();
        if predicate(&poll) {
            return poll;
        }
        assert!(Instant::now() < deadline, "Vim state: {:?}", poll.state);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "requires bundled real Vim, PTY and local channel access"]
fn script_export_owner_and_readonly_restore_survive_another_active_vim_document() {
    let directory = TestDirectory(std::env::temp_dir().join(format!(
        "tundra-vim-script-test-{}-{}",
        std::process::id(),
        PageInstanceId::new().get()
    )));
    let (runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
        directory.0.join("reports"),
        directory.0.join("fallback"),
        directory.0.join("data"),
        "vim-script-test",
        env!("CARGO_PKG_VERSION"),
    ))
    .unwrap();
    let app = process
        .register_app(AppDescriptor::new(
            AppId::from_static("vim-script-test"),
            "Vim script test",
            env!("CARGO_PKG_VERSION"),
            AppCriticality::Optional,
        ))
        .unwrap();
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (144, 44),
        ShellHomeMode::User,
    );
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            source: identity::IdentitySource::LinuxCurrentProcess,
            session_id: "vim-script-test".into(),
            user_id: "current".into(),
            username: "current".into(),
            role: UserRole::User,
            started_at_epoch_ms: 0,
        })),
        Instant::now(),
    );
    while state.notification_dismiss_active_modal_without_response() {}
    state.open_script_editor();
    let owner = state.active_page_id();
    state.new_edit_document();
    let other = state.active_page_id();
    assert!(state.set_editor_vim_enabled(true));
    let program = std::env::var_os("TUNDRA_TEST_VIM")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/vim/vim")
        });
    let mut config = VimConfig::new(&program);
    config.runtime_path = Some(program.parent().unwrap().join("runtime"));
    config.user_config = false;
    config.initial_buffers = [owner, other]
        .into_iter()
        .map(|id| input_for_page(&mut state, id).unwrap())
        .collect();
    let mut adapter = EditorVimHost::new(app.clone());
    for buffer in &config.initial_buffers {
        let id = [owner, other]
            .into_iter()
            .find(|id| id.get() == buffer.host_id)
            .unwrap();
        adapter
            .policies
            .insert(id, (buffer.read_only, buffer.protected));
        adapter.imported.insert(id);
    }
    adapter.host = Some(VimHost::spawn(config, &app.task_group("vim")).unwrap());
    let ready = until(adapter.host.as_mut().unwrap(), |poll| {
        poll.state.ready && poll.state.buffers.len() == 2
    });
    for buffer in &ready.state.buffers {
        let id = [owner, other]
            .into_iter()
            .find(|id| Some(id.get()) == buffer.host_id)
            .unwrap();
        adapter.pages.insert(buffer.number, id);
    }
    let command = adapter
        .host
        .as_mut()
        .unwrap()
        .activate_buffer(owner.get())
        .unwrap();
    until(adapter.host.as_mut().unwrap(), |poll| {
        poll.state.last_command >= command
    });
    adapter
        .host
        .as_ref()
        .unwrap()
        .send_bytes(b"Go# edited\x1b")
        .unwrap();
    until(adapter.host.as_mut().unwrap(), |poll| {
        poll.state
            .buffers
            .iter()
            .any(|buffer| buffer.host_id == Some(owner.get()) && buffer.text.contains("# edited"))
    });
    let command = adapter
        .host
        .as_mut()
        .unwrap()
        .activate_buffer(other.get())
        .unwrap();
    until(adapter.host.as_mut().unwrap(), |poll| {
        poll.state.last_command >= command
    });
    adapter.selected = Some(other);
    state.editor_vim_terminal = Some(Arc::new(ui::CommandLineTerminalSnapshot::blank(80, 20)));
    state.with_page_context(owner, |state| {
        state.request_script_launch(true, &platform::mock::UnsupportedPlatform)
    });
    assert_eq!(state.editor_vim_script_export, Some(owner));
    let deadline = Instant::now() + Duration::from_secs(10);
    while state.pages.editor.get(owner).unwrap().script.vim_pending {
        adapter
            .poll_inner(&mut state, &platform::mock::UnsupportedPlatform)
            .unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(state.active_page_id(), other);
    assert!(state.pages.editor.get(owner).unwrap().script.vim_suspended);
    assert!(!state.pages.editor.get(other).unwrap().script.vim_suspended);
    assert!(state.app.storage_config().editor.vim_mode_enabled);
    assert!(
        state
            .with_page_context(owner, |state| state
                .app
                .editor_state()
                .unwrap()
                .source()
                .contains("# edited"))
            .unwrap()
    );
    for locked in [true, false] {
        state.with_page_context(owner, |state| {
            state.pages.editor.script.pending_launch = None;
            state.pages.editor.script.vim_suspended = locked;
            state.pages.editor.script.state.status = if locked {
                app::script::ScriptStatus::Paused
            } else {
                app::script::ScriptStatus::Finished
            };
            state.app.dispatch_at(
                app::AppCommand::Editor(app::editor::EditorCommand::SetExecutionReadOnly(locked)),
                Instant::now(),
            );
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            adapter
                .poll_inner(&mut state, &platform::mock::UnsupportedPlatform)
                .unwrap();
            let poll = adapter.host.as_mut().unwrap().poll().unwrap();
            if adapter.import.is_none()
                && poll
                    .state
                    .buffers
                    .iter()
                    .any(|buffer| buffer.host_id == Some(owner.get()) && buffer.read_only == locked)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Vim policy state: {:?}",
                poll.state
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(state.active_page_id(), other);
        assert!(state.app.storage_config().editor.vim_mode_enabled);
    }
    let command = adapter
        .host
        .as_mut()
        .unwrap()
        .activate_buffer(owner.get())
        .unwrap();
    until(adapter.host.as_mut().unwrap(), |poll| {
        poll.state.last_command >= command
    });
    adapter.host.as_ref().unwrap().send_bytes(b"u").unwrap();
    until(adapter.host.as_mut().unwrap(), |poll| {
        poll.state.buffers.iter().any(|buffer| {
            buffer.host_id == Some(owner.get()) && buffer.text.as_ref() == app::script::NEW_SCRIPT
        })
    });
    state.activate_editor_document_page(owner);
    state.close_page_instance(other);
    adapter
        .poll_inner(&mut state, &platform::mock::UnsupportedPlatform)
        .unwrap();
    adapter
        .host
        .as_ref()
        .unwrap()
        .send_bytes(b"Go# VIM_UNSAVED_SMOKE\x1b")
        .unwrap();
    until(adapter.host.as_mut().unwrap(), |poll| {
        poll.state.buffers.iter().any(|buffer| {
            buffer.host_id == Some(owner.get()) && buffer.text.contains("# VIM_UNSAVED_SMOKE")
        })
    });
    state.request_script_launch(true, &platform::mock::UnsupportedPlatform);
    let deadline = Instant::now() + Duration::from_secs(10);
    while state.pages.editor.script.vim_pending {
        adapter
            .poll_inner(&mut state, &platform::mock::UnsupportedPlatform)
            .unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let path = directory.0.join("same-page-script.sh");
    let mut editor = state.app.editor_state().unwrap().clone();
    let saved = editor.source().to_string();
    std::fs::write(&path, &saved).unwrap();
    editor.document.path = Some(path.clone());
    state.app.dispatch_at(
        app::AppCommand::SetEditorState(Some(editor)),
        Instant::now(),
    );
    state.pages.editor.script.pending_launch = None;
    for locked in [true, false] {
        state.pages.editor.script.vim_suspended = locked;
        state.pages.editor.script.state.status = if locked {
            app::script::ScriptStatus::Paused
        } else {
            app::script::ScriptStatus::Finished
        };
        state.app.dispatch_at(
            app::AppCommand::Editor(app::editor::EditorCommand::SetExecutionReadOnly(locked)),
            Instant::now(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            adapter
                .poll_inner(&mut state, &platform::mock::UnsupportedPlatform)
                .unwrap();
            let poll = adapter.host.as_mut().unwrap().poll().unwrap();
            if adapter.import.is_none()
                && poll
                    .state
                    .buffers
                    .iter()
                    .any(|buffer| buffer.host_id == Some(owner.get()) && buffer.read_only == locked)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Vim resume state: {:?}",
                poll.state
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            state.app.editor_state().unwrap().access(),
            app::editor::EditorAccess::Editable
        );
        assert_eq!(state.app.editor_state().unwrap().is_read_only(), locked);
    }
    let options = directory.0.join("resumed-vim-options");
    adapter
        .host
        .as_ref()
        .unwrap()
        .send_bytes(
            format!(
                ":call writefile([string(&l:readonly), string(&l:modifiable)], '{}')\r",
                options.display().to_string().replace('\'', "''")
            )
            .as_bytes(),
        )
        .unwrap();
    until(adapter.host.as_mut().unwrap(), |_| {
        std::fs::read_to_string(&options).is_ok_and(|value| value == "0\n1\n")
    });
    adapter
        .host
        .as_ref()
        .unwrap()
        .send_bytes(b"GA_EDITABLE\x1b:write\r")
        .unwrap();
    let mut written = until(adapter.host.as_mut().unwrap(), |poll| {
        !poll.save_requests.is_empty()
    });
    let request = written.save_requests.remove(0);
    let expected = format!("{}_EDITABLE\n", saved.trim_end_matches('\n'));
    assert_eq!(request.bytes, expected.as_bytes());
    assert!(!request.force);
    std::fs::write(&path, request.bytes).unwrap();
    adapter
        .host
        .as_mut()
        .unwrap()
        .respond_save(
            request.id,
            Ok(vim_runtime::SaveResult { path: path.clone() }),
        )
        .unwrap();
    until(adapter.host.as_mut().unwrap(), |poll| {
        poll.state.buffers.iter().all(|buffer| !buffer.modified)
    });
    assert_eq!(std::fs::read_to_string(path).unwrap(), expected);
    adapter
        .poll_inner(&mut state, &platform::mock::UnsupportedPlatform)
        .unwrap();
    assert_eq!(
        state.app.editor_state().unwrap().access(),
        app::editor::EditorAccess::Editable
    );
    assert!(!state.app.editor_state().unwrap().is_read_only());
    drop(adapter);
    runtime.shutdown().unwrap();
}

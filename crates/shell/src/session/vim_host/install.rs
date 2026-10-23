use super::*;

enum InstallEvent {
    Progress(app::update::VimInstallProgress),
    Finished(Result<(), app::update::UpdateError>),
}

pub(super) struct VimInstaller {
    page: PageInstanceId,
    cancel: app::update::UpdateCancellation,
    events: mpsc::Receiver<InstallEvent>,
    _worker: ManagedThreadHandle<()>,
}

impl Drop for VimInstaller {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl EditorVimHost {
    pub(super) fn start_install(
        &mut self,
        state: &mut ShellSession,
        platform: &dyn Platform,
    ) -> io::Result<()> {
        let directory = app::update::installed_vim_directory(platform).map_err(io::Error::other)?;
        let (tx, events) = mpsc::channel();
        let cancel = app::update::UpdateCancellation::default();
        let worker_cancel = cancel.clone();
        let language = state.language.clone();
        let worker = self
            .watchdog
            .task_group("vim-install")
            .spawn_thread_with_cancellation(
                TaskSpec::one_shot(TaskId::new("download-engine").unwrap()),
                move |watchdog_cancel| {
                    let _language = i18n::enter_snapshot(language.clone());
                    let cancel = worker_cancel.with_watchdog(watchdog_cancel);
                    let result = app::update::install_vim_engine(
                        &directory,
                        &mut |progress| {
                            let _ = tx.send(InstallEvent::Progress(progress));
                        },
                        &cancel,
                    );
                    let _ = tx.send(InstallEvent::Finished(result));
                },
            )
            .map_err(io::Error::other)?;
        let page = state.content_page_id();
        state.editor_vim_install = Some((
            page,
            ui::EditorVimInstallViewModel {
                progress: Default::default(),
                cancelling: false,
            },
        ));
        self.installer = Some(VimInstaller {
            page,
            cancel,
            events,
            _worker: worker,
        });
        Ok(())
    }

    pub(super) fn poll_install(&mut self, state: &mut ShellSession) -> io::Result<bool> {
        let Some(installer) = &self.installer else {
            return Ok(false);
        };
        if state.page_screen(installer.page).is_none()
            || state
                .editor_vim_install
                .as_ref()
                .is_some_and(|(_, view)| view.cancelling)
        {
            installer.cancel.cancel();
        }
        loop {
            match installer.events.try_recv() {
                Ok(InstallEvent::Progress(progress)) => {
                    if let Some((_, view)) = &mut state.editor_vim_install {
                        view.progress = progress;
                    }
                }
                Ok(InstallEvent::Finished(result)) => {
                    let cancelled = installer.cancel.is_cancelled();
                    self.installer = None;
                    state.editor_vim_install = None;
                    if cancelled || result.as_ref().is_err_and(|error| error.is_cancelled()) {
                        state.set_editor_vim_enabled(false);
                        state.notify_status(i18n::msg!("vim-install-cancelled"));
                        return Ok(false);
                    }
                    result.map_err(io::Error::other)?;
                    return Ok(false);
                }
                Err(mpsc::TryRecvError::Empty) => return Ok(true),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.installer = None;
                    state.editor_vim_install = None;
                    return Err(io::Error::other(i18n::tr!("vim-install-worker-failed")));
                }
            }
        }
    }
}

//! Fedora's fixed tundraux3 update workflow. No caller-supplied package or command targets.
mod journal;
mod transport;

use crate::installation::{RpmIdentity, UpdateBackend};
use crate::service::ServiceError;
use crate::updates::{
    PackageChange, PackageVersion, UpdateCheck, UpdatePreview, UpdateProgress, UpdateResult,
    same_rpm_version,
};
use journal::Journal;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use transport::{DbusTransport, Operation, Report, Transport};

const TARGET: &str = super::installation::PACKAGE_NAME;

/// Requesting cancellation never kills a process or claims that cancellation succeeded.
#[derive(Debug, Clone, Default)]
pub struct UpdateCancellation {
    requested: Arc<AtomicBool>,
    allowed: Arc<AtomicBool>,
}
impl UpdateCancellation {
    pub fn request(&self) -> bool {
        if !self.allowed.load(Ordering::Acquire) {
            return false;
        }
        self.requested.store(true, Ordering::Release);
        true
    }
    fn reset(&self) {
        self.requested.store(false, Ordering::Release);
        self.allowed.store(false, Ordering::Release);
    }
}

pub struct RpmUpdates {
    controller: Controller<DbusTransport>,
}
impl RpmUpdates {
    pub fn current() -> Result<Self, ServiceError> {
        let installation = crate::installation::current_installation();
        if installation.backend != UpdateBackend::SystemRpm {
            return Err(ServiceError::Unsupported);
        }
        let installed = installation.rpm.ok_or(ServiceError::Unsupported)?;
        Ok(Self {
            controller: Controller {
                transport: DbusTransport::new()?,
                installed,
                candidate: None,
                preview: None,
                cancellation: UpdateCancellation::default(),
                journal: Journal::current()?,
            },
        })
    }
    pub fn cancellation(&self) -> UpdateCancellation {
        self.controller.cancellation.clone()
    }
    pub fn check(
        &mut self,
        progress: &mut dyn FnMut(UpdateProgress),
    ) -> Result<UpdateCheck, ServiceError> {
        self.controller.check(progress)
    }
    pub fn preview(
        &mut self,
        progress: &mut dyn FnMut(UpdateProgress),
    ) -> Result<UpdatePreview, ServiceError> {
        self.controller.preview(progress)
    }
    pub fn execute(
        &mut self,
        progress: &mut dyn FnMut(UpdateProgress),
    ) -> Result<UpdateResult, ServiceError> {
        self.controller.execute(progress)
    }
    pub fn query(
        &mut self,
        progress: &mut dyn FnMut(UpdateProgress),
    ) -> Result<Option<UpdateResult>, ServiceError> {
        self.controller.query(progress)
    }
}

struct Controller<T> {
    transport: T,
    installed: RpmIdentity,
    candidate: Option<PackageVersion>,
    preview: Option<UpdatePreview>,
    cancellation: UpdateCancellation,
    journal: Journal,
}
impl<T: Transport> Controller<T> {
    fn run(
        &mut self,
        operation: Operation,
        progress: &mut dyn FnMut(UpdateProgress),
    ) -> Result<Report, ServiceError> {
        let result = self
            .transport
            .run(operation, &self.cancellation, progress, &mut |_| Ok(()));
        self.cancellation.allowed.store(false, Ordering::Release);
        result
    }
    fn repositories(
        &mut self,
        progress: &mut dyn FnMut(UpdateProgress),
    ) -> Result<BTreeSet<String>, ServiceError> {
        let report = self.run(Operation::Repositories, progress)?;
        report.require_success()?;
        Ok(report.repositories)
    }
    fn check(
        &mut self,
        progress: &mut dyn FnMut(UpdateProgress),
    ) -> Result<UpdateCheck, ServiceError> {
        self.cancellation.reset();
        self.preview = None;
        self.candidate = None;
        self.installed = self.transport.installed()?;
        if self.installed.name != TARGET {
            return Err(ServiceError::Unsupported);
        }
        let repositories = self.repositories(progress)?;
        let report = self.run(Operation::Updates, progress)?;
        report.require_success()?;
        let mut candidates = report.packages.into_iter().filter(|(_, package)| {
            package.name == TARGET && package.architecture == self.installed.architecture
        });
        if let Some((info, candidate)) = candidates.next() {
            if !matches!(info, 2..=8 | 26)
                || candidates.next().is_some()
                || !repositories.contains(&candidate.repository)
            {
                return Err(ServiceError::UntrustedTransaction);
            }
            if !same_rpm_version(&candidate.version, &self.installed.version) {
                self.candidate = Some(candidate);
            }
        }
        Ok(UpdateCheck {
            installed_version: self.installed.version.clone(),
            candidate: self.candidate.clone(),
        })
    }
    fn build_preview(
        &mut self,
        progress: &mut dyn FnMut(UpdateProgress),
    ) -> Result<UpdatePreview, ServiceError> {
        let candidate = self.candidate.clone().ok_or(ServiceError::Unsupported)?;
        let repositories = self.repositories(progress)?;
        let report = self.run(Operation::Simulate(candidate.id()), progress)?;
        report.require_success()?;
        let mut packages = BTreeMap::new();
        for (info, package) in report.packages {
            // Simulation must explicitly distinguish installations from updates.
            let installing = match info {
                11 => false,
                12 | 27 => true,
                18 => continue,
                _ => return Err(ServiceError::UntrustedTransaction),
            };
            if !repositories.contains(&package.repository) {
                return Err(ServiceError::UntrustedTransaction);
            }
            let key = (package.name.clone(), package.architecture.clone());
            if let Some(previous) = packages.insert(key, (installing, package.clone())) {
                if previous != (installing, package) {
                    return Err(ServiceError::UntrustedTransaction);
                }
            }
        }
        if !packages
            .values()
            .any(|(installing, package)| !installing && package == &candidate)
        {
            return Err(ServiceError::UntrustedTransaction);
        }
        let names = packages
            .keys()
            .map(|(name, _)| name.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let installed = self.run(Operation::Installed(names), progress)?;
        installed.require_success()?;
        let mut old = BTreeMap::new();
        for (info, package) in installed.packages {
            if info != 1 {
                return Err(ServiceError::UntrustedTransaction);
            }
            if old
                .insert((package.name, package.architecture), package.version)
                .is_some()
            {
                return Err(ServiceError::UntrustedTransaction);
            }
        }
        let mut changes = Vec::new();
        for (key, (installing, package)) in packages {
            let old_version = old.remove(&key);
            if installing == old_version.is_some() {
                return Err(ServiceError::UntrustedTransaction);
            }
            changes.push(PackageChange {
                package,
                old_version,
            });
        }
        changes.sort();
        Ok(UpdatePreview { changes })
    }
    fn preview(
        &mut self,
        progress: &mut dyn FnMut(UpdateProgress),
    ) -> Result<UpdatePreview, ServiceError> {
        self.cancellation.reset();
        self.preview = None;
        let preview = self.build_preview(progress)?;
        self.preview = Some(preview.clone());
        Ok(preview)
    }
    fn execute(
        &mut self,
        progress: &mut dyn FnMut(UpdateProgress),
    ) -> Result<UpdateResult, ServiceError> {
        self.cancellation.reset();
        let reviewed = self
            .preview
            .take()
            .ok_or(ServiceError::UntrustedTransaction)?;
        let candidate = self.candidate.clone().ok_or(ServiceError::Unsupported)?;
        if self.transport.installed()? != self.installed
            || self.build_preview(progress)? != reviewed
        {
            return Err(ServiceError::UntrustedTransaction);
        }
        // Persist intent before asking the service to change anything.
        progress(UpdateProgress {
            stage: crate::updates::UpdateStage::StartingTransaction,
            ..Default::default()
        });
        self.journal.begin(&candidate)?;
        let report = self.transport.run(
            Operation::Execute(candidate.id()),
            &self.cancellation,
            progress,
            &mut |path| self.journal.record_transaction(path),
        );
        self.cancellation.allowed.store(false, Ordering::Release);
        let result = match report {
            Ok(report) if report.exit == 3 => UpdateResult::Cancelled,
            Ok(report) if report.exit == 1 && report.error.is_none() => {
                match self.transport.installed() {
                    Ok(installed)
                        if same_rpm_version(&installed.version, &candidate.version)
                            && installed.name == TARGET
                            && installed.architecture == candidate.architecture =>
                    {
                        UpdateResult::Installed {
                            version: installed.version,
                            system_restart_recommended: report.restart,
                        }
                    }
                    _ => UpdateResult::Unknown {
                        expected_version: candidate.version.clone(),
                    },
                }
            }
            Ok(report) => UpdateResult::Failed(
                report
                    .require_success()
                    .err()
                    .unwrap_or(ServiceError::Unknown),
            ),
            Err(
                error @ (ServiceError::Busy
                | ServiceError::PermissionDenied
                | ServiceError::Unsupported
                | ServiceError::AuthorizationCancelled),
            ) => UpdateResult::Failed(error),
            Err(_) => UpdateResult::Unknown {
                expected_version: candidate.version.clone(),
            },
        };
        if self.journal.finish(&result).is_err() {
            return Ok(UpdateResult::Unknown {
                expected_version: candidate.version,
            });
        }
        Ok(result)
    }
    fn query(
        &mut self,
        progress: &mut dyn FnMut(UpdateProgress),
    ) -> Result<Option<UpdateResult>, ServiceError> {
        let Some(record) = self.journal.read()? else {
            return Ok(None);
        };
        let installed = self.transport.installed()?;
        let report = self.run(Operation::History, progress)?;
        report.require_success()?;
        let matched = report.history.iter().any(|entry| entry.matches(&record));
        let result = if matched
            && installed.name == TARGET
            && same_rpm_version(&installed.version, &record.expected.version)
            && installed.architecture == record.expected.architecture
        {
            UpdateResult::Installed {
                version: installed.version,
                system_restart_recommended: false,
            }
        } else {
            UpdateResult::Unknown {
                expected_version: record.expected.version.clone(),
            }
        };
        self.journal.finish(&result)?;
        Ok(Some(result))
    }
}

#[cfg(test)]
mod tests;
